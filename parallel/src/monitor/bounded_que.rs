use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
};

pub struct BoundedQue<T> {
    mutex: Mutex<Info<T>>,
    has_item: Condvar,
    has_room: Condvar,
}

impl<T> BoundedQue<T> {
    pub fn new(limit: usize) -> Self {
        assert!(limit > 0);
        Self {
            mutex: Mutex::new(Info::new(limit)),
            has_item: Condvar::new(),
            has_room: Condvar::new(),
        }
    }

    pub fn push(&self, v: T) {
        let mut mutex = self.mutex.lock().unwrap();
        while !mutex.has_room() {
            mutex = self.has_room.wait(mutex).unwrap();
        }
        mutex.que.push_back(v);
        self.has_item.notify_one();
    }

    pub fn pop(&self) -> T {
        let mut mutex = self.mutex.lock().unwrap();
        while !mutex.has_item() {
            mutex = self.has_item.wait(mutex).unwrap();
        }
        let v = mutex.que.pop_front().unwrap();
        self.has_room.notify_one();
        v
    }
}

struct Info<T> {
    que: VecDeque<T>,
    limit: usize,
}

impl<T> Info<T> {
    fn new(limit: usize) -> Self {
        Self {
            que: VecDeque::new(),
            limit,
        }
    }
    fn has_item(&self) -> bool {
        !self.que.is_empty()
    }
    fn has_room(&self) -> bool {
        self.que.len() < self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo() {
        let q = BoundedQue::new(2);
        q.push(1);
        q.push(2);
        assert_eq!((q.pop(), q.pop()), (1, 2));
    }

    #[test]
    fn hands_off_between_threads() {
        const M: usize = 1000;
        let q = BoundedQue::new(4);
        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..M {
                    q.push(i);
                }
            });
            s.spawn(|| {
                for i in 0..M {
                    assert_eq!(q.pop(), i);
                }
            });
        });
    }

    #[test]
    #[should_panic]
    fn zero_limit_is_rejected() {
        // limit 0 would block every `push` forever.
        BoundedQue::<u8>::new(0);
    }
}
