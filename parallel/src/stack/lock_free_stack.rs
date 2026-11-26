use std::sync::atomic::{
    AtomicUsize,
    Ordering::{Acquire, Relaxed, Release},
};

use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};

pub struct LockFreeStack<T> {
    head: Atomic<Node<T>>,
    size: AtomicUsize,
}

struct Node<T> {
    val: T,
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub fn new(val: T, nxt: Shared<Node<T>>) -> Self {
        Self {
            val,
            next: Atomic::from(nxt),
        }
    }
}

impl<T> LockFreeStack<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, mut val: T) {
        let g = epoch::pin();
        loop {
            let head = self.head.load(Acquire, &g);
            let node = Owned::new(Node::new(val, head));
            match self.head.compare_exchange(head, node, Release, Relaxed, &g) {
                Ok(_) => {
                    self.size.fetch_add(1, Relaxed);
                    return;
                }
                Err(e) => val = e.new.into_box().val,
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        let g = epoch::pin();
        loop {
            let head = self.head.load(Acquire, &g);
            if head.is_null() {
                return None;
            }
            let head_nxt = unsafe { head.deref() }.next.load(Acquire, &g);
            if self
                .head
                .compare_exchange(head, head_nxt, Release, Relaxed, &g)
                .is_ok()
            {
                self.size.fetch_sub(1, Relaxed);
                return Some(unsafe { head.into_owned() }.into_box().val);
            }
        }
    }
}

impl<T> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        let g = epoch::pin();
        let mut p = self.head.load(Relaxed, &g);
        while !p.is_null() {
            let nxt = unsafe { p.deref() }.next.load(Relaxed, &g);
            unsafe { drop(p.into_owned()) };
            p = nxt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifo() {
        let s = LockFreeStack::new();
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
        const M: usize = 1000;
        let s = LockFreeStack::new();
        std::thread::scope(|sc| {
            for _ in 0..P {
                let s = &s;
                sc.spawn(move || {
                    for i in 0..M {
                        s.push(i);
                    }
                });
            }
        });
        let mut n = 0;
        while s.pop().is_some() {
            n += 1;
        }
        assert_eq!(n, P * M);
    }
}
