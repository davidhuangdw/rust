use std::{
    iter::repeat_n,
    sync::atomic::Ordering::{Acquire, Relaxed, Release},
};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

pub struct WaitFreeSnapshot<T: Eq + Clone + Default> {
    reg: Vec<Atomic<Info<T>>>,
}

pub struct Info<T: Eq> {
    val: T,
    prev_scan: Vec<T>, // record scan() before update
}

impl<T: Eq + Clone + Default> WaitFreeSnapshot<T> {
    pub fn new(n: usize) -> Self {
        Self {
            reg: (0..n)
                .map(|_| {
                    Atomic::new(Info {
                        val: T::default(),
                        prev_scan: (0..n).map(|_| T::default()).collect(),
                    })
                })
                .collect(),
        }
    }

    pub fn update(&self, i: usize, v: &T) {
        let g = epoch::pin();
        let pre_scan = self.scan();
        let old = self.reg[i].swap(
            Owned::new(Info {
                prev_scan: pre_scan,
                val: v.clone(),
            }),
            Release,
            &g,
        );
        unsafe { g.defer_destroy(old) };
    }

    pub fn scan(&self) -> Vec<T> {
        let g = epoch::pin();
        let n = self.reg.len();
        let mut updated: Vec<bool> = repeat_n(false, n).collect();
        let mut vals: Vec<T> = (0..n).map(|i| self.load(i, &g).0.clone()).collect();
        'retry: loop {
            for i in 0..n {
                let (v, prev_scan) = self.load(i, &g);
                if vals[i] == *v {
                    continue;
                }
                vals[i] = v.clone();
                if updated[i] {
                    // secondly updated, prev_scan fully inside scan()
                    return prev_scan.iter().map(|v| v.clone()).collect();
                }
                updated[i] = true;
                continue 'retry;
            }
            return vals;
        }
    }

    fn load<'a>(&'a self, i: usize, g: &'a Guard) -> (&'a T, &'a Vec<T>) {
        let reg = unsafe { self.reg[i].load(Acquire, g).deref() };
        (&reg.val, &reg.prev_scan)
    }
}

impl<T: Eq + Clone + Default> Drop for WaitFreeSnapshot<T> {
    fn drop(&mut self) {
        let g = epoch::pin();
        for p in &self.reg {
            let old = p.swap(Shared::null(), Relaxed, &g);
            unsafe { g.defer_destroy(old) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering::Relaxed},
        thread,
    };

    #[test]
    fn basic() {
        let s = WaitFreeSnapshot::<usize>::new(3);
        assert_eq!(s.scan(), [0, 0, 0]);
        s.update(1, &5);
        assert_eq!(s.scan(), [0, 5, 0]);
        s.update(0, &9);
        assert_eq!(s.scan(), [9, 5, 0]);
    }

    /// A single writer bumps reg0 then reg1 to the same k, so the only reachable
    /// states are `a == b` or `a == b + 1`; any other pair is a torn snapshot.
    #[test]
    fn concurrent_scan_is_consistent() {
        const K: usize = 5000;
        let s = WaitFreeSnapshot::<usize>::new(2);
        let done = AtomicBool::new(false);
        thread::scope(|sc| {
            sc.spawn(|| {
                for k in 1..=K {
                    s.update(0, &k);
                    s.update(1, &k);
                }
                done.store(true, Relaxed);
            });
            for _ in 0..3 {
                sc.spawn(|| {
                    while !done.load(Relaxed) {
                        let snap = s.scan();
                        let (a, b) = (snap[0], snap[1]);
                        assert!(a == b || a == b + 1, "torn snapshot: {a} {b}");
                    }
                });
            }
        });
    }
}
