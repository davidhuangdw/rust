use std::{
    hint::spin_loop,
    sync::atomic::{
        AtomicBool, AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

pub struct ArrayLock {
    slots: Vec<AtomicBool>,
    next: AtomicUsize,
}

impl ArrayLock {
    pub fn new(n: usize) -> Self {
        // assume: concurrent threads <= n
        let slots = (0..n).map(|i| AtomicBool::new(i == 0)).collect();
        let next = AtomicUsize::new(0);
        Self { slots, next }
    }

    pub fn lock(&self) -> Guard<'_> {
        let me = self.next.fetch_add(1, Relaxed) % self.slots.len();
        let slot = &self.slots[me];

        while !slot.load(Relaxed) || !slot.swap(false, Acquire) {
            spin_loop();
        }
        Guard { l: self, me }
    }

    fn unlock(&self, g: &Guard) {
        let my_nxt = (g.me + 1) % self.slots.len();
        self.slots[my_nxt].store(true, Release);
    }
}

#[must_use = "drop to unlock"]
pub struct Guard<'a> {
    l: &'a ArrayLock,
    me: usize,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.l.unlock(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{bump, exclusive};

    #[test]
    fn lock_unlock() {
        let l = ArrayLock::new(1);
        let _g = l.lock();
    }

    #[test]
    fn mutual_exclusion() {
        let l = ArrayLock::new(4);
        exclusive(4, 500, |_, c| {
            let _g = l.lock();
            bump(c);
        });
    }
}
