use std::{
    array,
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{
            AtomicBool, AtomicUsize,
            Ordering::{Acquire, Release},
        },
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

    pub fn lock(self: &Arc<Self>, id: usize) -> Guard {
        assert!(id < 2);
        let oth = id ^ 1;
        self.vis[id].store(true, Release);
        self.victim.store(id, Release);
        while self.vis[oth].load(Acquire) && self.victim.load(Acquire) == id {
            spin_loop();
        }
        Guard {
            p: self.clone(),
            id,
        }
    }
    fn unlock(&self, id: usize) {
        self.vis[id].store(false, Release);
    }
}

#[must_use = "dropping the guard releases the lock"]
pub struct Guard {
    p: Arc<Peterson>,
    id: usize,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.p.unlock(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn mutual_exclusion() {
        const M: usize = 1000;
        let (p, cnt) = (Arc::new(Peterson::new()), Arc::new(AtomicUsize::new(0)));
        std::thread::scope(|s| {
            for id in 0..2 {
                let (p, cnt) = (p.clone(), cnt.clone());
                s.spawn(move || {
                    for _ in 0..M {
                        let _g = p.lock(id);
                        let v = cnt.load(Relaxed);
                        std::thread::yield_now();
                        cnt.store(v + 1, Relaxed);
                    }
                });
            }
        });
        assert_eq!(cnt.load(Relaxed), 2 * M);
    }

    #[test]
    fn guard_releases_flag() {
        let p = Arc::new(Peterson::new());
        drop(p.lock(0));
        assert!(!p.vis[0].load(Relaxed));
    }
}
