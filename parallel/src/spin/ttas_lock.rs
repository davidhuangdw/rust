use std::{
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            Ordering::{Acquire, Relaxed, Release},
        },
    },
};

const MAX_BACKOFF: u32 = 1 << 6;

#[derive(Default)]
pub struct TtasLock {
    busy: AtomicBool,
}

impl TtasLock {
    pub fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
        }
    }

    pub fn lock(self: &Arc<Self>) -> Guard {
        // while self.busy.load(Relaxed) || self.busy.swap(true, Acquire) {}
        let mut backoff = 1;
        loop {
            while self.busy.load(Relaxed) {
                spin_loop();
            }
            if !self.busy.swap(true, Acquire) {
                return Guard { l: self.clone() };
            }

            //backoff
            for _ in 0..backoff {
                spin_loop();
            }
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}

#[must_use = "dropping the guard releases the lock"]
pub struct Guard {
    l: Arc<TtasLock>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.l.busy.store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    fn lock() -> Arc<TtasLock> {
        Arc::new(TtasLock::new())
    }

    #[test]
    fn lock_marks_busy() {
        let l = lock();
        let _g = l.lock();
        assert!(l.busy.load(Relaxed), "lock() returned without acquiring");
    }

    #[test]
    fn mutual_exclusion() {
        super::super::test_util::exclusive(lock(), TtasLock::lock, 4, 500);
    }
}
