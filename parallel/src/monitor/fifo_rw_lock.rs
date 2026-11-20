use std::sync::{Condvar, Mutex};

pub struct FifoRwLock {
    mutex: Mutex<Info>,
    can_read: Condvar,
    can_write: Condvar,
}

#[derive(Default)]
struct Info {
    writing: bool,
    reading: usize,
    // each writer a consecutive seq number
    last_done_writer: usize,
    last_writer: usize,
}

impl Info {
    fn can_read(&self, read_turn: usize) -> bool {
        !self.writing && read_turn <= self.last_done_writer
    }

    fn can_write(&self, write_turn: usize) -> bool {
        self.reading == 0 && write_turn == self.last_done_writer + 1
    }

    fn has_undone_writer(&self) -> bool {
        self.last_done_writer < self.last_writer
    }
}

impl FifoRwLock {
    pub fn new() -> Self {
        Self {
            mutex: Mutex::new(Info::default()),
            can_read: Condvar::new(),
            can_write: Condvar::new(),
        }
    }

    pub fn read_lock(&self) -> Guard<'_> {
        let mut mutex = self.mutex.lock().unwrap();
        let read_turn = mutex.last_writer;
        while !(mutex.can_read(read_turn)) {
            mutex = self.can_read.wait(mutex).unwrap();
        }
        mutex.reading += 1;
        Guard {
            l: self,
            for_read: true,
        }
    }

    fn read_unlock(&self) {
        let mut mutex = self.mutex.lock().unwrap();
        mutex.reading -= 1;
        if mutex.reading == 0 && mutex.has_undone_writer() {
            self.can_write.notify_all(); // since randomly qued on condvar
        }
    }

    pub fn write_lock(&self) -> Guard<'_> {
        let mut mutex = self.mutex.lock().unwrap();
        mutex.last_writer += 1;
        let write_turn = mutex.last_writer;
        while !mutex.can_write(write_turn) {
            mutex = self.can_write.wait(mutex).unwrap();
        }
        mutex.writing = true;
        Guard {
            l: self,
            for_read: false,
        }
    }

    fn write_unlock(&self) {
        let mut mutex = self.mutex.lock().unwrap();
        mutex.writing = false;
        mutex.last_done_writer += 1;

        self.can_read.notify_all();
        self.can_write.notify_all();
    }
}

impl Default for FifoRwLock {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use = "drop to unlock"]
pub struct Guard<'a> {
    l: &'a FifoRwLock,
    for_read: bool,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if self.for_read {
            self.l.read_unlock();
        } else {
            self.l.write_unlock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn readers_overlap() {
        let l = FifoRwLock::new();
        let _g1 = l.read_lock();
        let _g2 = l.read_lock();
        assert_eq!(l.mutex.lock().unwrap().reading, 2);
    }

    #[test]
    fn write_unlock_clears_writing() {
        let l = FifoRwLock::new();
        drop(l.write_lock());
        let info = l.mutex.lock().unwrap();
        assert!(!info.writing, "still marked as writing after unlock");
        assert_eq!(info.last_done_writer, 1);
    }

    #[test]
    fn blocked_writer_gets_woken() {
        let l = Arc::new(FifoRwLock::new());
        let g = l.write_lock();
        let (tx, rx) = mpsc::channel();
        let w = {
            let (l, tx) = (l.clone(), tx);
            std::thread::spawn(move || {
                let _g = l.write_lock();
                let _ = tx.send(());
            })
        };
        // wait until the second writer has queued itself and parked
        while l.mutex.lock().unwrap().last_writer < 2 {
            std::thread::yield_now();
        }
        drop(g);
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "queued writer never woke up after the first released"
        );
        w.join().unwrap();
    }
}
