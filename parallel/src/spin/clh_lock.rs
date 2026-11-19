use std::{
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{
            AtomicBool, AtomicPtr,
            Ordering::{AcqRel, Acquire, Relaxed, Release},
        },
    },
};

// a FIFO lock: each node spin on its pre
pub struct ClhLock {
    tail: AtomicPtr<Node>,
}

impl ClhLock {
    pub fn new() -> Self {
        Self {
            tail: AtomicPtr::new(Node::new_ptr()),
        }
    }

    pub fn lock(self: &Arc<ClhLock>) -> Guard {
        let me = Node::new_ptr();
        unsafe { (*me).locked.store(true, Relaxed) };
        // AcqRel: Release publishes our `locked = true` to whoever picks `me` up next,
        // Acquire lets us see the predecessor's `locked = true` (not its stale false).
        let pre = self.tail.swap(me, AcqRel);
        // Acquire pairs with the predecessor's Release unlock.
        while unsafe { (*pre).locked.load(Acquire) } {
            spin_loop();
        }
        Guard { me, pre }
    }
}

impl Default for ClhLock {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use = "drop to release lock"]
pub struct Guard {
    me: *mut Node,
    pre: *mut Node,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // unlock
        unsafe {
            (*self.me).locked.store(false, Release);
            // pre is refered only by the guard
            drop(Box::from_raw(self.pre));
        };
    }
}

struct Node {
    locked: AtomicBool,
}

impl Node {
    pub fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }
    pub fn new_ptr() -> *mut Self {
        Box::into_raw(Box::new(Node::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn mutual_exclusion() {
        super::super::test_util::exclusive(Arc::new(ClhLock::new()), ClhLock::lock, 4, 500);
    }

    #[test]
    fn default_has_tail_sentinel() {
        assert!(
            !ClhLock::default().tail.load(Relaxed).is_null(),
            "Default left `tail` null; the first lock() would dereference null"
        );
    }

    #[test]
    fn locks_do_not_share_nodes() {
        let a = Arc::new(ClhLock::new());
        let b = Arc::new(ClhLock::new());
        let _ga = a.lock();
        let _gb = b.lock();
        assert_ne!(
            a.tail.load(Relaxed),
            b.tail.load(Relaxed),
            "both locks queued the same thread-local node"
        );
    }
}
