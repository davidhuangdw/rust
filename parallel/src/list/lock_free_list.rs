use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};
use std::{
    cmp::Ordering,
    sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release},
};

const UNMARKED: usize = 0;
const MARKED: usize = 1;

/// Harris/Michael lock-free sorted list
///
/// The low bit of `next` marks a node as logically removed; always unmask with
/// `with_tag(UNMARKED)` before dereferencing. Epoch pinning prevents ABA.
pub struct LockFreeList<T: Ord> {
    head: Atomic<Node<T>>,
}

impl<T: Ord> LockFreeList<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        let g = epoch::pin();
        let mut cur = self.head.load(Acquire, &g);
        while let Some(node) = unsafe { cur.as_ref() } {
            let next = node.next.load(Acquire, &g);
            match node.key.cmp(val) {
                Ordering::Equal => return next.tag() == UNMARKED,
                Ordering::Greater => return false,
                Ordering::Less => cur = next.with_tag(UNMARKED),
            }
        }
        false
    }

    pub fn insert(&self, val: T) -> bool {
        let g = epoch::pin();
        let mut node = Owned::new(Node {
            key: val,
            next: Atomic::null(),
        });
        loop {
            let (pred, cur) = self.find(&node.key, &g);
            if unsafe { cur.as_ref() }.is_some_and(|n| n.key == node.key) {
                return false;
            }
            node.next.store(cur, Release);
            // Fails if `pred` was marked or changed.
            match pred.compare_exchange(cur, node, AcqRel, Acquire, &g) {
                Ok(_) => return true,
                Err(e) => node = e.new,
            }
        }
    }

    pub fn remove(&self, val: &T) -> bool {
        let g = epoch::pin();
        loop {
            let (pred, cur) = self.find(val, &g);
            let Some(node) = (unsafe { cur.as_ref() }).filter(|n| n.key == *val) else {
                return false;
            };

            let next = node.next.load(Acquire, &g);
            if next.tag() == MARKED
                || node
                    .next
                    .compare_exchange(next, next.with_tag(MARKED), AcqRel, Acquire, &g)
                    .is_err()
            {
                continue;
            }

            // Best-effort physical unlink; a later `find` cleans up if this fails.
            if pred
                .compare_exchange(cur, next.with_tag(UNMARKED), AcqRel, Acquire, &g)
                .is_ok()
            {
                unsafe { g.defer_destroy(cur) };
            }
            return true;
        }
    }

    /// First unmarked node with `key >= val`, splicing marked nodes out on the way.
    fn find<'g>(&'g self, val: &T, g: &'g Guard) -> (&'g Atomic<Node<T>>, Shared<'g, Node<T>>) {
        'retry: loop {
            let mut pred = &self.head;
            let mut cur = pred.load(Acquire, g);
            while let Some(node) = unsafe { cur.as_ref() } {
                let next = node.next.load(Acquire, g);
                if next.tag() == MARKED {
                    match pred.compare_exchange(cur, next.with_tag(UNMARKED), AcqRel, Acquire, g) {
                        Ok(_) => unsafe { g.defer_destroy(cur) },
                        Err(_) => continue 'retry,
                    }
                    cur = next.with_tag(UNMARKED);
                    continue;
                }
                if node.key >= *val {
                    return (pred, cur);
                }
                pred = &node.next;
                cur = next;
            }
            return (pred, cur);
        }
    }
}

impl<T: Ord> Default for LockFreeList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> Drop for LockFreeList<T> {
    fn drop(&mut self) {
        let g = epoch::pin();
        let mut cur = self.head.load(Relaxed, &g);
        while let Some(node) = unsafe { cur.as_ref() } {
            let next = node.next.load(Relaxed, &g).with_tag(UNMARKED);
            unsafe { drop(cur.into_owned()) };
            cur = next;
        }
    }
}

struct Node<T> {
    key: T,
    next: Atomic<Node<T>>,
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    /// Reachable values in order, skipping logically removed nodes.
    fn values<T: Ord + Clone>(l: &LockFreeList<T>) -> Vec<T> {
        let g = epoch::pin();
        let mut out = vec![];
        let mut cur = l.head.load(Acquire, &g);
        while let Some(node) = unsafe { cur.as_ref() } {
            let next = node.next.load(Acquire, &g);
            if next.tag() == UNMARKED {
                out.push(node.key.clone());
            }
            cur = next.with_tag(UNMARKED);
        }
        out
    }

    impl SortedList for LockFreeList<usize> {
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
            values(self)
        }
    }

    #[test]
    fn add_contains_remove() {
        check_add_contains_remove::<LockFreeList<usize>>();
    }

    #[test]
    fn concurrent_adds_then_removes() {
        check_concurrent_adds_then_removes::<LockFreeList<usize>>();
    }

    #[test]
    fn concurrent_add_remove_churn() {
        const P: usize = 4;
        const M: usize = 300;
        let l = LockFreeList::new();
        std::thread::scope(|s| {
            // churn: insert then remove the odd values, so those nodes get unlinked
            for _ in 0..P {
                let l = &l;
                s.spawn(move || {
                    for i in 0..M {
                        l.insert(2 * i + 1);
                        l.remove(&(2 * i + 1));
                    }
                });
            }
            // stable: insert the even values sitting right next to the churn
            for _ in 0..P {
                let l = &l;
                s.spawn(move || {
                    for i in 0..M {
                        l.insert(2 * i);
                    }
                });
            }
        });
        let evens: Vec<usize> = values(&l).into_iter().filter(|x| x % 2 == 0).collect();
        assert_eq!(evens, (0..M).map(|i| 2 * i).collect::<Vec<_>>());
    }
}
