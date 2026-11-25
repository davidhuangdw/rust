use std::sync::{
    MutexGuard,
    atomic::{
        AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

use crate::list::{Node, retire_all};

pub struct LazySortedList<T: Ord> {
    head: Atomic<Node<T>>,
    size: AtomicUsize,
}

impl<T: Ord> LazySortedList<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::new(Node::new(None, Shared::null())),
            size: AtomicUsize::new(0),
        }
    }

    pub fn contains(&self, v: &T) -> bool {
        let g = epoch::pin();
        let cur_p = self.walk(v, &g).1;
        has_val(cur_p, v)
    }

    pub fn len(&self) -> usize {
        self.size.load(Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&self, v: T) -> bool {
        let g = epoch::pin();
        loop {
            let (pre, cur) = self.walk(&v, &g);
            if has_val(cur, &v) {
                return false;
            }
            // reload after lock
            let (valid, _pre, _cur) = still_adjacent_after_lock(pre, cur, &g);
            if !valid {
                continue;
            }
            unsafe { pre.deref() }
                .next
                .store(Owned::new(Node::new(Some(v), cur)), Release);
            self.size.fetch_add(1, Relaxed);
            return true;
        }
    }

    pub fn remove(&self, v: &T) -> bool {
        let g = epoch::pin();
        loop {
            let (pre, cur) = self.walk(v, &g);
            if !has_val(cur, &v) {
                return false;
            }
            // reload after lock
            let (valid, _pre, _cur) = still_adjacent_after_lock(pre, cur, &g);
            if !valid {
                continue;
            }
            let cur_node = unsafe { cur.deref() };
            let nxt = cur_node.next.load(Acquire, &g);

            cur_node.removed.store(true, Release);
            unsafe { pre.deref() }.next.store(nxt, Release);
            self.size.fetch_sub(1, Relaxed);
            unsafe {
                g.defer_destroy(cur);
            }
            return true;
        }
    }

    fn walk<'a>(&'a self, v: &T, g: &'a Guard) -> (Shared<'a, Node<T>>, Shared<'a, Node<T>>) {
        let mut pre = self.head.load(Acquire, g);
        let mut cur = unsafe { pre.deref() }.next.load(Acquire, g);
        while let Some(node) = unsafe { cur.as_ref() }
            && node.val.as_ref() < Some(v)
        {
            (pre, cur) = (cur, node.next.load(Acquire, g));
        }
        (pre, cur)
    }
}

fn has_val<T: Ord>(p: Shared<Node<T>>, v: &T) -> bool {
    unsafe { p.as_ref() }.is_some_and(|n| n.val_is(v))
}

impl<T: Ord> Default for LazySortedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> Drop for LazySortedList<T> {
    fn drop(&mut self) {
        retire_all(&self.head);
    }
}

fn still_adjacent_after_lock<'a, T: Ord>(
    pre: Shared<'a, Node<T>>,
    cur: Shared<'a, Node<T>>,
    g: &'a Guard,
) -> (bool, Option<MutexGuard<'a, ()>>, Option<MutexGuard<'a, ()>>) {
    let pre_l = unsafe { pre.as_ref() }.map(|n| n.mutex.lock().unwrap());
    let cur_l = unsafe { cur.as_ref() }.map(|n| n.mutex.lock().unwrap());

    let pre_node = unsafe { pre.deref() };
    let valid = !pre_node.removed.load(Acquire)
        && pre_node.next.load(Acquire, g) == cur
        && unsafe { cur.as_ref() }.is_none_or(|c| !c.removed.load(Acquire));
    (valid, pre_l, cur_l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list::{SortedList, check_add_contains_remove, check_concurrent_adds_then_removes};

    /// Reachable values in order, skipping logically removed nodes.
    fn values<T: Ord + Clone>(l: &LazySortedList<T>) -> Vec<T> {
        let g = epoch::pin();
        let mut out = vec![];
        let mut cur = l.head.load(Acquire, &g);
        while let Some(node) = unsafe { cur.as_ref() } {
            if !node.removed.load(Acquire) {
                out.extend(node.val.clone());
            }
            cur = node.next.load(Acquire, &g);
        }
        out
    }

    impl SortedList for LazySortedList<usize> {
        fn insert(&self, val: usize) -> bool {
            LazySortedList::insert(self, val)
        }
        fn remove(&self, val: &usize) -> bool {
            LazySortedList::remove(self, val)
        }
        fn contains(&self, val: &usize) -> bool {
            LazySortedList::contains(self, val)
        }
        fn values(&self) -> Vec<usize> {
            values(self)
        }
    }

    #[test]
    fn add_contains_remove() {
        check_add_contains_remove::<LazySortedList<usize>>();
    }

    #[test]
    fn concurrent_adds_then_removes() {
        check_concurrent_adds_then_removes::<LazySortedList<usize>>();
    }
}
