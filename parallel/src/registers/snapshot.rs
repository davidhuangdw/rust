use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};
use std::sync::atomic::Ordering::{Acquire, Release};

/// Fixed-size of registers
pub struct Snapshot<T: Clone> {
    reg: Vec<Atomic<T>>,
}

impl<T: Clone> Snapshot<T> {
    pub fn from_slice(s: &[T]) -> Self {
        Self {
            reg: s.iter().map(|v| Atomic::new(v.clone())).collect(),
        }
    }

    pub fn update(&self, i: usize, v: &T) {
        let g = epoch::pin();
        let old = self.reg[i].swap(Owned::new(v.clone()), Release, &g);
        if !old.is_null() {
            unsafe {
                g.defer_destroy(old);
            }
        }
    }

    /// snapshot by Double collect, versioned ptr by crossbeam_epoch
    /// Obstruction-free: continuous writers can make this spin forever.
    pub fn get_snapshot(&self) -> Vec<T> {
        loop {
            let guard = epoch::pin();
            let pre: Vec<Shared<T>> = self.reg.iter().map(|a| a.load(Acquire, &guard)).collect();
            if (0..pre.len()).all(|i| self.reg[i].load(Acquire, &guard) == pre[i]) {
                return pre
                    .iter()
                    .map(|p| unsafe { p.as_ref().unwrap().clone() })
                    .collect();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflects_updates() {
        let s = Snapshot::from_slice(&[0, 0, 0]);
        s.update(1, &7);
        assert_eq!(s.get_snapshot(), [0, 7, 0]);
    }

    #[test]
    fn concurrent_snapshots_never_rewind() {
        const N: usize = 3;
        const M: u64 = 300;
        let s = Snapshot::from_slice(&[0u64; N]);
        std::thread::scope(|sc| {
            for i in 0..N {
                let s = &s;
                sc.spawn(move || {
                    for v in 1..=M {
                        s.update(i, &v);
                    }
                });
            }
            sc.spawn(|| {
                let mut prev = vec![0u64; N];
                for _ in 0..2000 {
                    let cur = s.get_snapshot();
                    assert!(
                        cur.iter().zip(&prev).all(|(c, p)| c >= p),
                        "snapshot {cur:?} rewound from {prev:?}"
                    );
                    prev = cur;
                }
            });
        });
        assert_eq!(s.get_snapshot(), vec![M; N]);
    }
}
