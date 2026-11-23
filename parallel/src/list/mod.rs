pub mod lock_free_list;
pub mod optimistic_sorted_list;
pub mod simple_sorted_list;

use std::sync::{Mutex, atomic::Ordering::Relaxed};

use crossbeam_epoch::{self as epoch, Atomic, Shared};

/// lock-based Node
pub(crate) struct Node<T: Ord> {
    pub(crate) val: Option<T>, // immutable; None only for the sentinel
    pub(crate) next: Atomic<Node<T>>,
    pub(crate) mutex: Mutex<()>, // writers/removers take it
}

impl<T: Ord> Node<T> {
    pub(crate) fn new(val: Option<T>, next: Shared<'_, Node<T>>) -> Self {
        Self {
            val,
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
