use std::{
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{
            AtomicBool, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
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

    pub fn lock(self: &Arc<ArrayLock>) -> Guard {
        let me = self.next.fetch_add(1, Relaxed) % self.slots.len();
        let slot = &self.slots[me];

        while !slot.load(Relaxed) || !slot.swap(false, Acquire) {
            spin_loop();
        }
        Guard {
            l: self.clone(),
            me,
        }
    }

    fn unlock(&self, g: &Guard) {
        let my_nxt = (g.me + 1) % self.slots.len();
        self.slots[my_nxt].store(true, Release);
    }
}

#[must_use = "drop to unlock"]
pub struct Guard {
    l: Arc<ArrayLock>,
    me: usize,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.l.unlock(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_unlock() {
        let l = Arc::new(ArrayLock::new(1));
        let _g = l.lock();
    }

    #[test]
    fn mutual_exclusion() {
        super::super::test_util::exclusive(Arc::new(ArrayLock::new(4)), ArrayLock::lock, 4, 500);
    }
}
