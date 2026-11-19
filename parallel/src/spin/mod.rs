pub mod array_lock;
pub mod clh_lock;
pub mod ttas_lock;

#[cfg(test)]
pub(crate) mod test_util {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    };

    /// Hammer `lock` from `n` threads whose critical section does a non-atomic
    /// read-modify-write: a lost update means two threads were inside at once.
    pub fn exclusive<L: Send + Sync + 'static, G>(
        l: Arc<L>,
        lock: fn(&Arc<L>) -> G,
        n: usize,
        m: usize,
    ) {
        let cnt = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|s| {
            for _ in 0..n {
                let (l, cnt) = (l.clone(), cnt.clone());
                s.spawn(move || {
                    for _ in 0..m {
                        let _g = lock(&l);
                        let v = cnt.load(Relaxed);
                        std::thread::yield_now();
                        cnt.store(v + 1, Relaxed);
                    }
                });
            }
        });
        assert_eq!(cnt.load(Relaxed), n * m, "two threads inside at once");
    }
}
