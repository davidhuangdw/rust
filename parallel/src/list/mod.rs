pub mod lazy_list;
pub mod lock_free_list;
pub mod optimistic_sorted_list;
pub mod simple_sorted_list;

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering::Relaxed},
};

use crossbeam_epoch::{self as epoch, Atomic, Shared};

/// lock-based Node
pub(crate) struct Node<T: Ord> {
    pub(crate) val: Option<T>, // immutable; None only for the sentinel
    pub(crate) next: Atomic<Node<T>>,
    pub(crate) removed: AtomicBool, // only for lazy-list
    pub(crate) mutex: Mutex<()>,    // writers/removers take it
}

impl<T: Ord> Node<T> {
    pub(crate) fn new(val: Option<T>, next: Shared<'_, Node<T>>) -> Self {
        Self {
            val,
            removed: AtomicBool::new(false),
            next: Atomic::from(next),
            mutex: Mutex::new(()),
        }
    }

    pub(crate) fn val_is(&self, v: &T) -> bool {
        self.val.as_ref() == Some(v)
    }
}

/// Retire the whole chain; used by both `Drop` impls.
pub(crate) fn retire_all<T: Ord>(head: &Atomic<Node<T>>) {
    let g = epoch::pin();
    let mut cur = head.load(Relaxed, &g);
    while !cur.is_null() {
        let next = unsafe { cur.deref() }.next.load(Relaxed, &g);
        unsafe { g.defer_destroy(cur) };
        cur = next;
    }
}

#[cfg(test)]
pub(crate) fn values<T: Ord + Clone>(head: &Atomic<Node<T>>) -> Vec<T> {
    use std::sync::atomic::Ordering::Acquire;

    let g = epoch::pin();
    let mut out = vec![];
    let mut cur = head.load(Acquire, &g);
    while let Some(node) = unsafe { cur.as_ref() } {
        out.extend(node.val.clone()); // the sentinel's None contributes nothing
        cur = node.next.load(Acquire, &g);
    }
    out
}

#[cfg(test)]
pub(crate) trait SortedList: Default + Sync {
    fn insert(&self, val: usize) -> bool;
    fn remove(&self, val: &usize) -> bool;
    fn contains(&self, val: &usize) -> bool;
    fn values(&self) -> Vec<usize>;
    fn len(&self) -> usize {
        self.values().len()
    }
}

#[cfg(test)]
pub(crate) fn check_add_contains_remove<L: SortedList>() {
    let l = L::default();
    assert!(l.values().is_empty());
    assert_eq!(l.len(), 0);

    for v in [10, 20, 30, 5, 25, 40] {
        assert!(l.insert(v));
    }
    assert_eq!(l.values(), [5, 10, 20, 25, 30, 40]);
    assert!(!l.insert(5));

    assert!(l.contains(&25));
    assert!(!l.contains(&26));

    assert!(l.remove(&5)); // head
    assert!(l.remove(&25)); // middle
    assert!(l.remove(&40)); // tail
    assert!(!l.remove(&999)); // absent
    assert!(!l.remove(&5)); // already gone
    assert_eq!(l.values(), [10, 20, 30]);
    assert_eq!(l.len(), 3);
    assert!(!l.contains(&5));
}

#[cfg(test)]
pub(crate) fn check_concurrent_adds_then_removes<L: SortedList>() {
    const P: usize = 4;
    const M: usize = 400;
    let l = L::default();

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
    assert_eq!(l.values(), (0..M).collect::<Vec<_>>());
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
        l.values(),
        (0..M).filter(|i| i % 2 == 1).collect::<Vec<_>>()
    );
    assert_eq!(l.len(), M / 2);
}
