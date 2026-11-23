use std::sync::atomic::{
    AtomicUsize,
    Ordering::{Acquire, Relaxed, Release},
};

use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};

pub struct LockFreeQue<T> {
    head: Atomic<Node<T>>, // sentinal at head
    tail: Atomic<Node<T>>,
    size: AtomicUsize,
}

impl<T> LockFreeQue<T> {
    pub fn new() -> Self {
        let sentinal = Node::default(); // sentinal to avoid atomic-update both head&tail when push empty
        let head = Atomic::new(sentinal);
        let tail = head.clone(); // both point at the same sentinel
        Self {
            head,
            tail,
            size: AtomicUsize::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.size.load(Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        let g = epoch::pin();
        let head = self.head.load(Acquire, &g);
        let next = unsafe { head.deref() }.next.load(Relaxed, &g);
        next.is_null()
    }

    pub fn push_back(&self, val: T) {
        let mut node = Owned::new(Node::new(Some(val)));
        loop {
            let g = epoch::pin();
            let tail = self.tail.load(Acquire, &g);
            let tail_node = unsafe { tail.deref() };
            let tail_next = tail_node.next.load(Acquire, &g);

            // ensure tail_next==null, otherwise help forward
            if !tail_next.is_null() {
                self.tail
                    .compare_exchange(tail, tail_next, Release, Relaxed, &g)
                    .ok();
                continue;
            }

            // try append to tail
            let node_shared = Shared::from(&*node as *const Node<T>);
            match tail_node
                .next
                .compare_exchange(tail_next, node, Release, Relaxed, &g)
            {
                Ok(_) => {
                    self.size.fetch_add(1, Relaxed);
                    self.tail
                        .compare_exchange(tail, node_shared, Release, Relaxed, &g)
                        .ok();
                    return;
                }
                Err(e) => node = e.new,
            }
        }
    }

    pub fn pop_front(&self) -> Option<T> {
        loop {
            let g = epoch::pin();
            let head = self.head.load(Acquire, &g);
            let mut head_next = unsafe { head.deref() }.next.load(Acquire, &g);

            if head_next.is_null() {
                return None;
            }

            // ensure tail != head(to pop)
            let tail = self.tail.load(Acquire, &g);
            if tail == head {
                self.tail
                    .compare_exchange(tail, head_next, Release, Relaxed, &g)
                    .ok();
            }

            // try fowrad head to pop
            if self
                .head
                .compare_exchange(head, head_next, Release, Relaxed, &g)
                .is_ok()
            {
                unsafe { g.defer_destroy(head) };
                self.size.fetch_sub(1, Relaxed);
                // head_next become the sentinal-head, ok to take
                return unsafe { head_next.deref_mut() }.val.take();
            }
        }
    }
}

impl<T> Default for LockFreeQue<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct Node<T> {
    val: Option<T>, // None only for sentinel
    next: Atomic<Node<T>>,
}

impl<T> Node<T> {
    pub fn new(val: Option<T>) -> Self {
        Self {
            val,
            next: Atomic::null(),
        }
    }
}

impl<T> Default for Node<T> {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo() {
        let q = LockFreeQue::new();
        for i in 0..3 {
            q.push_back(i);
        }
        assert_eq!(q.len(), 3);
        assert!(!q.is_empty());
        assert_eq!(
            (q.pop_front(), q.pop_front(), q.pop_front()),
            (Some(0), Some(1), Some(2))
        );
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
        assert_eq!(q.pop_front(), None);
    }

    #[test]
    fn single_producer_single_consumer() {
        const M: usize = 1000;
        let q = LockFreeQue::new();
        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..M {
                    q.push_back(i);
                }
            });
            s.spawn(|| {
                for i in 0..M {
                    // `pop_front` is non-blocking: `None` just means the queue
                    // was momentarily empty, so retry instead of failing.
                    let v = loop {
                        if let Some(v) = q.pop_front() {
                            break v;
                        }
                        std::hint::spin_loop();
                    };
                    assert_eq!(v, i, "out of order / lost element");
                }
            });
        });
        assert!(q.is_empty());
    }

    #[test]
    fn mpmc_keeps_each_producer_in_order() {
        use std::sync::atomic::AtomicUsize;
        const P: usize = 4;
        const M: usize = 500;
        let q = LockFreeQue::new();
        let left = AtomicUsize::new(P * M);
        std::thread::scope(|s| {
            for p in 0..P {
                let q = &q;
                s.spawn(move || {
                    for i in 0..M {
                        q.push_back((p, i));
                    }
                });
            }
            for _ in 0..P {
                let (q, left) = (&q, &left);
                s.spawn(move || {
                    let mut last = [None; P];
                    loop {
                        match q.pop_front() {
                            Some((p, i)) => {
                                assert!(last[p].is_none_or(|l| l < i), "producer {p} reordered");
                                last[p] = Some(i);
                                if left.fetch_sub(1, Relaxed) == 1 {
                                    break;
                                }
                            }
                            None => {
                                if left.load(Relaxed) == 0 {
                                    break;
                                }
                                std::hint::spin_loop();
                            }
                        }
                    }
                });
            }
        });
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }
}
