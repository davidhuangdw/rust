use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};
use std::{
    mem::forget,
    sync::atomic::Ordering::{Acquire, Relaxed, Release},
};

/// Harris/Michael lock-free sorted list
///
/// A node's link is an immutable `Status { removed, next }` behind an `Atomic`, so
/// "marked?" and "successor?" come from a single load — the book's
/// `AtomicMarkableReference`, emulated by publishing a fresh cell on every write.
/// Marking and splicing therefore CAS the *same* word, which is what keeps a
/// concurrent insert from landing behind a node that is being removed. The cell
/// pointer doubles as a version: old cells stay alive while pinned, so addresses
/// are never reused and CAS cannot hit ABA.
///
/// Reclamation rule: the winner of a CAS retires the cell it replaced. Note that
/// `crossbeam_epoch::Atomic::compare_exchange` returns the value it *wrote* on
/// success (unlike `std`), so the handle for the new cell is the returned one and
/// the cell to retire is the `current` argument.
pub struct LockFreeList<T: Ord> {
    head: Atomic<Status<T>>,
}

impl<T: Ord> LockFreeList<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::new(Status::new(false, Shared::null())),
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        let g = epoch::pin();
        let mut cur = load_status(&self.head, &g).3;
        while let Some(cur_node) = cur
            && cur_node.val < *val
        {
            cur = load_status(&cur_node.s, &g).3;
        }
        let Some(cur_node) = cur else { return false };
        cur_node.val == *val && !load_status(&cur_node.s, &g).1
    }

    pub fn insert(&self, val: T) -> bool {
        let g = epoch::pin();
        let own = Owned::new(Node::new(val, Shared::null()));
        loop {
            let (pre_s, pre_s_ptr, cur_ptr, cur) = self.walk(&own.val, &g);
            if cur.is_some_and(|n| n.val == own.val) {
                return false;
            }
            own.set_next(cur_ptr, &g);

            let node = Shared::from(&*own as *const Node<T>);
            let new = Owned::new(Status::new(false, node));
            match pre_s.compare_exchange(pre_s_ptr, new, Release, Acquire, &g) {
                Ok(_) => {
                    unsafe { g.defer_destroy(pre_s_ptr) }; // the cell we replaced
                    forget(own); // ownership now belongs to the list
                    return true;
                }
                Err(_) => continue, // the link moved, walk again
            }
        }
    }

    pub fn remove(&self, val: &T) -> bool {
        let g = epoch::pin();
        loop {
            let (pre_s, pre_s_ptr, cur_ptr, cur) = self.walk(val, &g);
            let Some(cur_node) = cur else { return false };
            let (cur_s_ptr, cur_removed, nxt_ptr, _) = load_status(&cur_node.s, &g);
            if cur_node.val != *val {
                return false;
            }
            if cur_removed {
                continue;
            }

            // mark: the new cell carries both the flag and the successor
            let new = Owned::new(Status::new(true, nxt_ptr));
            let marked = match cur_node
                .s
                .compare_exchange(cur_s_ptr, new, Release, Acquire, &g)
            {
                Ok(marked) => marked,
                Err(_) => continue, // someone marked it first
            };
            unsafe { g.defer_destroy(cur_s_ptr) }; // the unmarked cell we replaced

            // unlink at best effort
            let new = Owned::new(Status::new(false, nxt_ptr));
            if pre_s
                .compare_exchange(pre_s_ptr, new, Release, Acquire, &g)
                .is_ok()
            {
                unsafe {
                    g.defer_destroy(pre_s_ptr);
                    g.defer_destroy(marked);
                    g.defer_destroy(cur_ptr);
                }
            }
            return true;
        }
    }

    /// First node with `val >= v`, splicing marked nodes out on the way (lock-free).
    /// Returns `(pre's link, that link's cell, the node it points at, that node)`;
    fn walk<'a>(&'a self, val: &T, g: &'a Guard) -> Walk<'a, T> {
        'retry: loop {
            let mut pre_s = &self.head;
            let (mut pre_s_ptr, _, mut cur_ptr, mut cur) = load_status(&self.head, g);
            while let Some(cur_node) = cur {
                let (cur_s_ptr, cur_removed, nxt_ptr, nxt) = load_status(&cur_node.s, g);

                if cur_removed {
                    // help forward
                    let new = Owned::new(Status::new(false, nxt_ptr));
                    match pre_s.compare_exchange(pre_s_ptr, new, Release, Relaxed, g) {
                        Ok(s) => {
                            unsafe {
                                g.defer_destroy(pre_s_ptr);
                                g.defer_destroy(cur_s_ptr);
                                g.defer_destroy(cur_ptr);
                            }
                            (pre_s_ptr, cur_ptr, cur) = (s, nxt_ptr, nxt);
                            continue;
                        }
                        Err(_) => continue 'retry, // pre might be removed too
                    }
                }

                if cur_node.val >= *val {
                    break;
                }
                (pre_s, pre_s_ptr, cur_ptr, cur) = (&cur_node.s, cur_s_ptr, nxt_ptr, nxt);
            }
            return (pre_s, pre_s_ptr, cur_ptr, cur);
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
        let mut s_ptr = self.head.load(Relaxed, &g);
        loop {
            let next = unsafe { s_ptr.deref() }.next.load(Relaxed, &g);
            unsafe { drop(s_ptr.into_owned()) };
            if next.is_null() {
                break;
            }
            s_ptr = unsafe { next.deref() }.s.load(Relaxed, &g);
            unsafe { drop(next.into_owned()) };
        }
    }
}

/// `(pre's link, that link's cell, the node it points at, that node)`.
type Walk<'a, T> = (
    &'a Atomic<Status<T>>,
    Shared<'a, Status<T>>,
    Shared<'a, Node<T>>,
    Option<&'a Node<T>>,
);

/// `(cell, removed, next node pointer, next node)` read from one link.
type Loaded<'a, T> = (
    Shared<'a, Status<T>>,
    bool,
    Shared<'a, Node<T>>,
    Option<&'a Node<T>>,
);

struct Node<T: Ord> {
    val: T,
    s: Atomic<Status<T>>,
}

impl<T: Ord> Node<T> {
    fn new(val: T, next: Shared<Node<T>>) -> Self {
        Self {
            val,
            s: Atomic::new(Status::new(false, next)),
        }
    }

    fn set_next(&self, next: Shared<Node<T>>, g: &Guard) {
        let old = self
            .s
            .swap(Owned::new(Status::new(false, next)), Release, g);
        unsafe { drop(old.into_owned()) };
    }
}

fn load_status<'a, T: Ord>(s: &Atomic<Status<T>>, g: &'a Guard) -> Loaded<'a, T> {
    let s_ptr = s.load(Acquire, g);
    let s = unsafe { s_ptr.deref() };
    let nxt = s.next.load(Acquire, g);
    let nxt_node = unsafe { nxt.as_ref() };
    (s_ptr, s.removed, nxt, nxt_node)
}

/// cell: `{removed, next}` to CAS
struct Status<T: Ord> {
    removed: bool,
    next: Atomic<Node<T>>, // immutable
}

impl<T: Ord> Status<T> {
    fn new(removed: bool, next: Shared<Node<T>>) -> Self {
        Self {
            removed,
            next: Atomic::from(next),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reachable values in order, skipping logically removed nodes.
    fn values<T: Ord + Clone>(l: &LockFreeList<T>) -> Vec<T> {
        let g = epoch::pin();
        let mut out = vec![];
        let mut cur = load_status(&l.head, &g).3;
        while let Some(node) = cur {
            let (_, removed, _, nxt) = load_status(&node.s, &g);
            if !removed {
                out.push(node.val.clone());
            }
            cur = nxt;
        }
        out
    }

    #[test]
    fn add_contains_remove() {
        let l = LockFreeList::new();
        assert!(values(&l).is_empty());

        for v in [10, 20, 30] {
            l.insert(v);
        }
        l.insert(5); // head
        l.insert(25); // middle
        l.insert(40); // tail
        assert_eq!(values(&l), [5, 10, 20, 25, 30, 40]);
        assert!(!l.insert(5));

        assert!(l.contains(&25));
        assert!(!l.contains(&26));

        assert!(l.remove(&5)); // head
        assert!(l.remove(&25)); // middle
        assert!(l.remove(&40)); // tail
        assert!(!l.remove(&999)); // absent
        assert!(!l.remove(&5)); // already gone
        assert_eq!(values(&l), [10, 20, 30]);
        assert_eq!(values(&l).len(), 3);
        assert!(!l.contains(&5));
    }

    #[test]
    fn concurrent_adds_then_removes() {
        const P: usize = 4;
        const M: usize = 400;
        let l = LockFreeList::new();

        // every thread inserts the same range: a lost insert or a double-accepted
        // duplicate both show up in the result
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
        assert_eq!(values(&l), (0..M).collect::<Vec<_>>());
        assert_eq!(values(&l).len(), M);

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
            values(&l),
            (0..M).filter(|i| i % 2 == 1).collect::<Vec<_>>()
        );
        assert_eq!(values(&l).len(), M / 2);
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
