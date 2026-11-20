use std::{
    marker::PhantomData,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    static THREAD_ID: usize = NEXT_ID.fetch_add(1, Relaxed);
}

fn thread_id() -> usize {
    THREAD_ID.with(|id| *id)
}

pub struct ReentrantLock {
    info: Mutex<Info>,
    cond: Condvar,
}

#[derive(Default)]
struct Info {
    owner: usize,
    count: usize,
}

impl ReentrantLock {
    pub fn new() -> Self {
        Self {
            info: Mutex::new(Info::default()),
            cond: Condvar::new(),
        }
    }

    pub fn lock(&self) -> Guard<'_> {
        let me = thread_id();
        let mut mutex = self.info.lock().unwrap();
        while !(mutex.count == 0 || mutex.owner == me) {
            mutex = self.cond.wait(mutex).unwrap();
        }
        mutex.owner = me;
        mutex.count += 1;
        Guard {
            l: self,
            _not_send: PhantomData,
        }
    }

    fn unlock(&self) {
        let mut mutex = self.info.lock().unwrap();
        mutex.count -= 1;
        if mutex.count == 0 {
            self.cond.notify_one();
        }
    }
}

impl Default for ReentrantLock {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use = "drop to unlock"]
pub struct Guard<'a> {
    l: &'a ReentrantLock,
    _not_send: PhantomData<*const ()>,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.l.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{bump, exclusive};

    #[test]
    fn reenters_without_deadlock() {
        let l = ReentrantLock::new();
        let g1 = l.lock();
        let g2 = l.lock();
        assert_eq!(l.info.lock().unwrap().count, 2);
        drop(g2);
        assert_eq!(l.info.lock().unwrap().count, 1);
        drop(g1);
        assert_eq!(l.info.lock().unwrap().count, 0);
    }

    #[test]
    fn mutual_exclusion() {
        let l = ReentrantLock::new();
        exclusive(4, 200, |_, c| {
            let _g = l.lock();
            bump(c);
        });
    }
}
