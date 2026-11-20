pub mod monitor;
pub mod mutex;
pub mod registers;
pub mod spin;

#[cfg(test)]
pub(crate) mod test_util {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    /// Non-atomic read-modify-write: a lost update means two threads were
    /// inside the critical section at once.
    pub fn bump(cnt: &AtomicUsize) {
        let v = cnt.load(Relaxed);
        std::thread::yield_now();
        cnt.store(v + 1, Relaxed);
    }

    /// Run `f` from `n` threads, `m` times each, handing it the thread index and
    /// a counter. `f` must keep the lock held while calling [`bump`].
    pub fn exclusive<F>(n: usize, m: usize, f: F)
    where
        F: Fn(usize, &AtomicUsize) + Copy + Send + Sync,
    {
        let cnt = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for id in 0..n {
                let (cnt, f) = (&cnt, f);
                s.spawn(move || {
                    for _ in 0..m {
                        f(id, cnt);
                    }
                });
            }
        });
        assert_eq!(cnt.load(Relaxed), n * m, "two threads inside at once");
    }
}
