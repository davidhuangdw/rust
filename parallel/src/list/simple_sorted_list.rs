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
    use super::{super::*, *};

    impl SortedList for SimpleSortedList<usize> {
        fn insert(&self, val: usize) -> bool {
            self.insert(val)
        }
        fn remove(&self, val: &usize) -> bool {
            self.remove(val)
        }
        fn contains(&self, val: &usize) -> bool {
            self.contains(val)
        }
        fn values(&self) -> Vec<usize> {
            values(&self.head)
        }
        fn len(&self) -> usize {
            self.len()
        }
    }

    #[test]
    fn add_contains_remove() {
        check_add_contains_remove::<SimpleSortedList<usize>>();
    }

    #[test]
    fn concurrent_adds_then_removes() {
        check_concurrent_adds_then_removes::<SimpleSortedList<usize>>();
    }
}
