use std::sync::{
    MutexGuard,
    atomic::{
        AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

use super::Node;

/// Hand-over-hand sorted list: every node has its own lock and
/// the walk holds two adjacent locks, so a locked node cannot be spliced away.
pub struct SimpleSortedList<T: Ord> {
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

impl<T: Ord> SimpleSortedList<T> {
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

    pub fn insert(&self, val: T) -> bool {
        let g = epoch::pin();
        let (pre, cur, _pre, _cur) = self.walk(&val, &g);
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
        let (pre, cur, _pre, _cur) = self.walk(val, &g);
        if !Self::is_there(cur, val) {
            return false;
        }
        // unlink first, then drop
        let next = unsafe { cur.deref() }.next.load(Acquire, &g);
        unsafe { pre.deref() }.next.store(next, Release);
        unsafe { g.defer_destroy(cur) };
        self.size.fetch_sub(1, Relaxed);
        true
    }

    pub fn contains(&self, val: &T) -> bool {
        let g = epoch::pin();
        Self::is_there(self.scan(val, &g), val)
    }

    /// First node with `val >= v`, lock-free.
    fn scan<'a>(&self, v: &T, g: &'a Guard) -> Shared<'a, Node<T>> {
        let mut cur = unsafe { self.head.load(Acquire, g).deref() }
            .next
            .load(Acquire, g);
        while unsafe { cur.as_ref() }.is_some_and(|n| n.val.as_ref().is_some_and(|k| k < v)) {
            cur = unsafe { cur.deref() }.next.load(Acquire, g);
        }
        cur
    }

    fn is_there(cur: Shared<'_, Node<T>>, val: &T) -> bool {
        unsafe { cur.as_ref() }.is_some_and(|n| n.val_is(val))
    }

    /// Walk but keep the last two node locks; lock order is always list order.
    fn walk<'a>(&self, v: &T, g: &'a Guard) -> Walk<'a, T> {
        let mut pre = self.head.load(Acquire, g);
        let mut _pre = Some(unsafe { pre.deref() }.mutex.lock().unwrap());
        let mut cur = unsafe { pre.deref() }.next.load(Acquire, g);
        let mut _cur = unsafe { cur.as_ref() }.map(|x| x.mutex.lock().unwrap());
        while !cur.is_null() {
            let cur_node = unsafe { cur.deref() };
            if cur_node.val.as_ref() >= Some(v) {
                break;
            }
            pre = cur;
            _pre = _cur.take();
            cur = cur_node.next.load(Acquire, g);
            _cur = unsafe { cur.as_ref() }.map(|x| x.mutex.lock().unwrap());
        }
        (pre, cur, _pre.unwrap(), _cur)
    }
}

impl<T: Ord> Default for SimpleSortedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> Drop for SimpleSortedList<T> {
    fn drop(&mut self) {
        super::retire_all(&self.head);
    }
}

#[cfg(test)]
mod tests {
    use super::{super::values, *};

    #[test]
    fn add_contains_remove() {
        let l = SimpleSortedList::new();
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
        let l = SimpleSortedList::new();

        // every thread inserts the same range: lost updates and double-accepted
        // duplicates both show up in the result
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

        // then every thread removes the even values
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
