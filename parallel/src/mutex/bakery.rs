use std::{
    iter::repeat_with,
    sync::{
        Arc,
        atomic::{
            AtomicBool, AtomicUsize,
            Ordering::{Acquire, Release},
        },
    },
};

pub struct Bakery {
    n: usize,
    vis: Vec<AtomicBool>,
    tick: Vec<AtomicUsize>,
}

impl Bakery {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            vis: repeat_with(|| AtomicBool::new(false)).take(n).collect(),
            tick: repeat_with(|| AtomicUsize::new(0)).take(n).collect(),
        }
    }

    fn must_wait(&self, id: usize, me: usize) -> bool {
        (0..self.n).any(|i| {
            i != id && self.vis[i].load(Acquire) && (self.tick[i].load(Acquire), i) < (me, id)
        })
    }

    pub fn lock(self: &Arc<Self>, id: usize) -> Guard {
        self.vis[id].store(true, Release);
        let me = (0..self.n)
            .filter(|&i| i != id)
            .map(|i| self.tick[i].load(Acquire))
            .max()
            .unwrap_or(0)
            + 1;
        self.tick[id].store(me, Release);
        while self.must_wait(id, me) {
            std::hint::spin_loop();
        }
        Guard {
            bakery: Arc::clone(self),
            id,
        }
    }
}

#[must_use = "dropping the guard releases the lock"]
pub struct Guard {
    bakery: Arc<Bakery>,
    id: usize,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.bakery.vis[self.id].store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn mutual_exclusion() {
        const N: usize = 4;
        const M: usize = 1000;
        let (bk, cnt) = (Arc::new(Bakery::new(N)), Arc::new(AtomicUsize::new(0)));
        std::thread::scope(|s| {
            for id in 0..N {
                let (bk, cnt) = (bk.clone(), cnt.clone());
                s.spawn(move || {
                    for _ in 0..M {
                        let _g = bk.lock(id);
                        let v = cnt.load(Relaxed);
                        std::thread::yield_now();
                        cnt.store(v + 1, Relaxed);
                    }
                });
            }
        });
        assert_eq!(cnt.load(Relaxed), N * M);
    }

    #[test]
    fn guard_releases_slot() {
        let bk = Arc::new(Bakery::new(1));
        drop(bk.lock(0));
        assert!(!bk.vis[0].load(Relaxed));
    }

    #[test]
    fn orders_by_ticket_then_id() {
        let bk = Bakery::new(2);
        // id 1 holds 11; id 0 is back in the doorway over its old ticket 10.
        bk.tick[0].store(10, Relaxed);
        bk.vis[0].store(true, Relaxed);
        bk.vis[1].store(true, Relaxed);
        bk.tick[1].store(11, Relaxed);
        assert!(bk.must_wait(1, 11)); // 10 < 11: id 1 yields
        // equal tickets: the lower id goes first
        bk.tick[0].store(11, Relaxed);
        assert!(!bk.must_wait(0, 11));
        assert!(bk.must_wait(1, 11));
    }
}
