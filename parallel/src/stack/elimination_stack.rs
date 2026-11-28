use std::{
    cell::UnsafeCell,
    hint::spin_loop,
    sync::atomic::Ordering::{Acquire, Relaxed, Release},
};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};
use rand::{RngExt, rng};

use crate::stack::lock_free_stack::LockFreeStack;

const EMPTY: usize = 0;
const WAITING: usize = 1;
const BUSY: usize = 2;

const DEFAULT_CAPACITY: usize = 8;
const SPINS: usize = 64;

struct Slot<T> {
    is_push: bool,
    val: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send> Sync for Slot<T> {}

fn take_slot<T>(ptr: Shared<'_, Slot<T>>, g: &Guard) -> Option<T> {
    let v = unsafe { (*ptr.deref().val.get()).take() };
    unsafe { g.defer_destroy(ptr) };
    v
}

pub struct Exchanger<T> {
    st: Atomic<Slot<T>>,
}

impl<T> Exchanger<T> {
    pub fn new() -> Self {
        Self { st: Atomic::null() }
    }

    pub fn exchange(&self, v: Option<T>) -> Result<Option<T>, Option<T>> {
        let g = epoch::pin();
        let is_push = v.is_some();
        let mut my_slot = Owned::new(Slot {
            is_push,
            val: UnsafeCell::new(v),
        });

        for _ in 0..=SPINS {
            let cur = self.st.load(Acquire, &g);
            match cur.tag() {
                EMPTY => {
                    let target = my_slot.with_tag(WAITING);
                    match self.st.compare_exchange(cur, target, Release, Relaxed, &g) {
                        Ok(a_ptr) => {
                            for _ in 0..SPINS {
                                let now = self.st.load(Acquire, &g);
                                if now.tag() == BUSY {
                                    self.st.store(Shared::null(), Release);
                                    return Ok(take_slot(now, &g));
                                }
                                spin_loop();
                            }
                            return match self.st.compare_exchange(
                                a_ptr,
                                Shared::null(),
                                Acquire,
                                Relaxed,
                                &g,
                            ) {
                                Ok(_) => Err(take_slot(a_ptr, &g)),
                                Err(e) => {
                                    self.st.store(Shared::null(), Release);
                                    Ok(take_slot(e.current, &g))
                                }
                            };
                        }
                        Err(e) => my_slot = e.new,
                    }
                }
                WAITING => {
                    if unsafe { cur.deref() }.is_push == is_push {
                        return Err(my_slot.into_box().val.into_inner());
                    }
                    let target = my_slot.with_tag(BUSY);
                    match self.st.compare_exchange(cur, target, Release, Relaxed, &g) {
                        Ok(_) => return Ok(take_slot(cur, &g)),
                        Err(e) => my_slot = e.new,
                    }
                }
                _ => spin_loop(),
            }
        }
        Err(my_slot.into_box().val.into_inner())
    }
}

impl<T> Drop for Exchanger<T> {
    fn drop(&mut self) {
        let g = epoch::pin();
        let p = self.st.load(Relaxed, &g);
        if !p.is_null() {
            unsafe { drop(p.into_owned()) };
        }
    }
}

pub struct EliminationArray<T> {
    exchangers: Box<[Exchanger<T>]>,
}

impl<T> EliminationArray<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 1);
        Self {
            exchangers: (0..capacity).map(|_| Exchanger::new()).collect(),
        }
    }

    pub fn visit(&self, v: Option<T>) -> Result<Option<T>, Option<T>> {
        let idx = rng().random_range(0..self.exchangers.len());
        self.exchangers[idx].exchange(v)
    }
}

pub struct EliminationStack<T> {
    stack: LockFreeStack<T>,
    elim: EliminationArray<T>,
}

impl<T> EliminationStack<T> {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            stack: LockFreeStack::new(),
            elim: EliminationArray::new(capacity),
        }
    }

    pub fn push(&self, mut val: T) {
        let g = epoch::pin();
        loop {
            match self.stack.try_push(val, &g) {
                Ok(()) => return,
                Err(v) => val = v,
            }
            match self.elim.visit(Some(val)) {
                Ok(None) => return,
                Ok(Some(v)) | Err(Some(v)) => val = v,
                Err(None) => unreachable!(),
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        let g = epoch::pin();
        loop {
            if let Ok(res) = self.stack.try_pop(&g) {
                return res;
            }
            if let Ok(res) = self.elim.visit(None) {
                return res;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn lifo() {
        let s = EliminationStack::new();
        assert_eq!(s.pop(), None);
        for i in 0..5 {
            s.push(i);
        }
        for i in [4, 3, 2, 1, 0] {
            assert_eq!(s.pop(), Some(i));
        }
        assert_eq!(s.pop(), None);
    }

    #[test]
    fn concurrent_push_pop() {
        const P: usize = 4;
        const M: usize = 2000;
        let s = EliminationStack::with_capacity(4);
        let popped = AtomicUsize::new(0);
        std::thread::scope(|sc| {
            for t in 0..P {
                let s = &s;
                let popped = &popped;
                sc.spawn(move || {
                    for i in 0..M {
                        s.push(t * M + i);
                        if s.pop().is_some() {
                            popped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        while s.pop().is_some() {
            popped.fetch_add(1, Ordering::Relaxed);
        }
        assert_eq!(popped.load(Ordering::Relaxed), P * M);
    }
}
