use std::{
    array,
    hint::spin_loop,
    sync::atomic::{
        AtomicBool, AtomicUsize,
        Ordering::{Acquire, Release},
    },
};

pub struct Peterson {
    vis: [AtomicBool; 2],
    victim: AtomicUsize,
}

impl Peterson {
    pub fn new() -> Self {
        Self {
            vis: array::from_fn(|_| AtomicBool::new(false)),
            victim: AtomicUsize::new(0),
        }
    }

    pub fn lock(&self, id: usize) -> Guard<'_> {
        assert!(id < 2);
        let oth = id ^ 1;
        self.vis[id].store(true, Release);
        self.victim.store(id, Release);
        while self.vis[oth].load(Acquire) && self.victim.load(Acquire) == id {
            spin_loop();
        }
        Guard { p: self, id }
    }
    fn unlock(&self, id: usize) {
        self.vis[id].store(false, Release);
    }
}

impl Default for Peterson {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use = "dropping the guard releases the lock"]
pub struct Guard<'a> {
    p: &'a Peterson,
    id: usize,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.p.unlock(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{bump, exclusive};
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn mutual_exclusion() {
        let p = Peterson::new();
        exclusive(2, 1000, |id, c| {
            let _g = p.lock(id);
            bump(c);
        });
    }

    #[test]
    fn guard_releases_flag() {
        let p = Peterson::new();
        drop(p.lock(0));
        assert!(!p.vis[0].load(Relaxed));
    }
}
