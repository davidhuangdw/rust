use std::sync::{
    MutexGuard,
    atomic::{
        AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

use super::Node;

/// Optimistic list (Herlihy–Shavit): the walk is lock-free; writers lock the
/// two adjacent nodes and then re-walk to validate the pair is still current.
pub struct OptimisticSortedList<T: Ord> {
    head: Atomic<Node<T>>, // sentinel at head
    size: AtomicUsize,
}

/// `(pre, cur, pre_guard, cur_guard)`: both nodes stay locked while this lives.
type Walk<'a, T> = (
    Shared<'a, Node<T>>,
    Shared<'a, Node<T>>,
    MutexGuard<'a, ()>,
    Option<MutexGuard<'a, ()>>,
);

impl<T: Ord> OptimisticSortedList<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::new(Node::new(None, Shared::null())),
            size: AtomicUsize::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.size.load(Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// First node with `val >= v`, lock-free.
    fn walk<'g>(&self, v: &T, g: &'g Guard) -> (Shared<'g, Node<T>>, Shared<'g, Node<T>>) {
        unsafe {
            let mut pre = self.head.load(Acquire, g);
            let mut cur = pre.deref().next.load(Acquire, g);
            while let Some(node) = cur.as_ref() {
                match node.val.as_ref() {
                    Some(k) if k < v => {
                        pre = cur;
                        cur = node.next.load(Acquire, g);
                    }
                    _ => break,
                }
            }
            (pre, cur)
        }
    }

    /// optimistic snapshot to target pair
    fn lock_pair<'g>(&self, v: &T, g: &'g Guard) -> Walk<'g, T> {
        loop {
            let (pre, cur) = self.walk(v, g);
            let pre_guard = unsafe { pre.deref() }.mutex.lock().unwrap();
            let cur_guard = unsafe { cur.as_ref() }.map(|n| n.mutex.lock().unwrap());
            if (pre, cur) == self.walk(v, g) {
                return (pre, cur, pre_guard, cur_guard);
            }
        }
    }

    fn is_there(cur: Shared<'_, Node<T>>, val: &T) -> bool {
        unsafe { cur.as_ref() }.is_some_and(|n| n.val_is(val))
    }

    pub fn contains(&self, val: &T) -> bool {
        let g = epoch::pin();
        Self::is_there(self.walk(val, &g).1, val)
    }

    pub fn insert(&self, val: T) -> bool {
        let g = epoch::pin();
        let (pre, cur, _pre, _cur) = self.lock_pair(&val, &g);
        if Self::is_there(cur, &val) {
            return false;
        }
        let node = Owned::new(Node::new(Some(val), cur));
        unsafe { pre.deref() }.next.store(node, Release);
        self.size.fetch_add(1, Relaxed);
        true
    }

    pub fn remove(&self, val: &T) -> bool {
        let g = epoch::pin();
        let (pre, cur, _pre, _cur) = self.lock_pair(val, &g);
        if !Self::is_there(cur, val) {
            return false;
        }
        // unlink first, then retire (see the note in `SimpleSortedList::remove`)
        let next = unsafe { cur.deref() }.next.load(Acquire, &g);
        unsafe { pre.deref() }.next.store(next, Release);
        unsafe { g.defer_destroy(cur) };
        self.size.fetch_sub(1, Relaxed);
        true
    }
}

impl<T: Ord> Default for OptimisticSortedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> Drop for OptimisticSortedList<T> {
    fn drop(&mut self) {
        super::retire_all(&self.head);
    }
}

#[cfg(test)]
mod tests {
    use super::{super::values, *};

    #[test]
    fn add_contains_remove() {
        let l = OptimisticSortedList::default();
        assert!(l.is_empty());

        for v in [10, 20, 30] {
            l.insert(v);
        }
        l.insert(5); // head
        l.insert(25); // middle
        l.insert(40); // tail
        assert_eq!(values(&l.head), [5, 10, 20, 25, 30, 40]);
        assert!(!l.insert(5));

        assert!(l.contains(&25));
        assert!(!l.contains(&26));

        assert!(l.remove(&5)); // head
        assert!(l.remove(&25)); // middle
        assert!(l.remove(&40)); // tail
        assert!(!l.remove(&999)); // absent
        assert_eq!(values(&l.head), [10, 20, 30]);
        assert_eq!(l.len(), 3);
    }

    #[test]
    fn concurrent_adds_then_removes() {
        const P: usize = 4;
        const M: usize = 400;
        let l = OptimisticSortedList::default();

        // every thread inserts the same range: a lost update or a missing validate
        // rewalk shows up as a wrong result
        std::thread::scope(|s| {
            for _ in 0..P {
                let l = &l;
                s.spawn(move || {
                    for i in 0..M {
                        l.insert(i);
                    }
                });
            }
        });
        assert_eq!(values(&l.head), (0..M).collect::<Vec<_>>());
        assert_eq!(l.len(), M);

        std::thread::scope(|s| {
            for _ in 0..P {
                let l = &l;
                s.spawn(move || {
                    for i in (0..M).step_by(2) {
                        l.remove(&i);
                    }
                });
            }
        });
        assert_eq!(
            values(&l.head),
            (0..M).filter(|i| i % 2 == 1).collect::<Vec<_>>()
        );
        assert_eq!(l.len(), M / 2);
    }
}
