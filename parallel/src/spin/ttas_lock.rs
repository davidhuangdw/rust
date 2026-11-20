use std::{
    hint::spin_loop,
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Relaxed, Release},
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

    pub fn lock(&self) -> Guard<'_> {
        // while self.busy.load(Relaxed) || self.busy.swap(true, Acquire) {}
        let mut backoff = 1;
        loop {
            while self.busy.load(Relaxed) {
                spin_loop();
            }
            if !self.busy.swap(true, Acquire) {
                return Guard { l: self };
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
pub struct Guard<'a> {
    l: &'a TtasLock,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.l.busy.store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{bump, exclusive};

    #[test]
    fn lock_marks_busy() {
        let l = TtasLock::new();
        let _g = l.lock();
        assert!(l.busy.load(Relaxed), "lock() returned without acquiring");
    }

    #[test]
    fn mutual_exclusion() {
        let l = TtasLock::new();
        exclusive(4, 500, |_, c| {
            let _g = l.lock();
            bump(c);
        });
    }
}
