use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

// head -> .. -> tail
// invariant:
//      head_ptr is always sentinel
//      nodes on chain are all of the same req type
// need to keep:
//      when append: tail_ptr.nxt == null (help forward)
//      when pop: tail won't be poped <=> tail==first (help forward)
//
// empty <=> head_nxt is null
// empty or same req: head_nxt.is_null_or(|n| n.req == req)
// insert/remove: if empty or same req: or { append(req-node) } else {pop(firt_node) }

pub struct DualSyncQue<T> {
    head: Atomic<Node<T>>, // sentinel at head
    tail: Atomic<Node<T>>, // last waiting-item
}

struct Node<T> {
    req: bool, //immutable
    val: Atomic<T>,
    next: Atomic<Node<T>>,
}

impl<T> DualSyncQue<T> {
    pub fn new() -> Self {
        let head = Atomic::new(Node::new(true, None, Shared::null()));
        let tail = head.clone();
        Self { head, tail }
    }

    pub fn push_back(&self, v: T) {
        self.exchange(Some(v));
    }
    pub fn pop_front(&self) -> T {
        self.exchange(None).unwrap()
    }

    fn exchange(&self, v: Option<T>) -> Option<T> {
        let g = epoch::pin();
        let req = v.is_none(); // pop if true, push if false
        let mut v = v;
        loop {
            let head = self.head.load(Acquire, &g);
            let head_nxt = unsafe { head.deref() }.next.load(Acquire, &g);

            // ensure tail is the last node, otherwise help forward
            let tail = self.tail.load(Acquire, &g);
            let tail_nxt = unsafe { tail.deref() }.next.load(Acquire, &g);
            if !tail_nxt.is_null() {
                self.tail
                    .compare_exchange(tail, tail_nxt, AcqRel, Relaxed, &g)
                    .ok();
                continue;
            }

            if unsafe { head_nxt.as_ref() }.is_none_or(|n| n.req == req) {
                // append & spin-wait node.val
                let node = Owned::new(Node::new(req, v.take(), Shared::null())).into_shared(&g);
                match unsafe { tail.deref() }
                    .next
                    .compare_exchange(tail_nxt, node, AcqRel, Relaxed, &g)
                {
                    Ok(_) => return self.spin_wait_node_val(node, req, &g),
                    Err(_) => {
                        // fail and move back v
                        let node = unsafe { node.into_owned().into_box() };
                        v = take_val(&node.val, &g);
                    }
                }
            } else {
                // pop & offer node.val
                let first = unsafe { head_nxt.deref() };
                let first_nxt = first.next.load(Acquire, &g);
                if unsafe { head.deref() }
                    .next
                    .compare_exchange(head_nxt, first_nxt, AcqRel, Relaxed, &g)
                    .is_ok()
                {
                    // ensure: tail!= to-pop(first)
                    let new_tail = if first_nxt.is_null() { head } else { first_nxt };
                    self.tail
                        .compare_exchange(head_nxt, new_tail, AcqRel, Relaxed, &g)
                        .ok();

                    unsafe { g.defer_destroy(head_nxt) };
                    // exchange node.val to notify the other side
                    return if req {
                        take_val(&first.val, &g)
                    } else {
                        first
                            .val
                            .store(Owned::new(v.take().expect("push carries a value")), Release);
                        None
                    };
                }
            }
        }
    }

    fn spin_wait_node_val(&self, node: Shared<Node<T>>, req: bool, g: &Guard) -> Option<T> {
        let node = unsafe { node.deref() };
        let mut val = node.val.load(Acquire, g);
        while val.is_null() == req {
            std::hint::spin_loop();
            val = node.val.load(Acquire, g);
        }
        if !req {
            return None;
        }
        return take_val(&node.val, &g);
    }
}

fn take_val<T>(slot: &Atomic<T>, g: &Guard) -> Option<T> {
    let taken = slot.swap(Shared::null(), AcqRel, g);
    (!taken.is_null()).then(|| unsafe { *taken.into_owned().into_box() })
}

impl<T> Drop for DualSyncQue<T> {
    fn drop(&mut self) {
        let g = epoch::pin();
        let mut cur = self.head.load(Relaxed, &g);
        while let Some(node) = unsafe { cur.as_ref() } {
            let next = node.next.load(Relaxed, &g);
            let _ = take_val(&node.val, &g);
            unsafe { drop(cur.into_owned()) };
            cur = next;
        }
    }
}

impl<T> Node<T> {
    pub fn new(req: bool, val: Option<T>, next: Shared<Node<T>>) -> Self {
        let val = if let Some(v) = val {
            Atomic::new(v)
        } else {
            Atomic::null()
        };
        Self {
            req,
            val,
            next: Atomic::from(next),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, sync::mpsc, thread, time::Duration};

    #[test]
    fn push_pop_rendezvous() {
        let q = Arc::new(DualSyncQue::new());
        let (tx, rx) = mpsc::channel();

        let pop_q = q.clone();
        let _ = thread::spawn(move || tx.send(pop_q.pop_front()).ok());
        let push_q = q.clone();
        let _ = thread::spawn(move || push_q.push_back(7));

        // A waiting push and pop must complete the handoff.
        assert_eq!(rx.recv_timeout(Duration::from_millis(500)), Ok(7));
    }

    #[test]
    fn fifo_order() {
        const N: usize = 4;
        let q = Arc::new(DualSyncQue::new());
        let (tx, rx) = mpsc::channel();

        let pop_q = q.clone();
        let _ = thread::spawn(move || {
            for _ in 0..N {
                if tx.send(pop_q.pop_front()).is_err() {
                    return;
                }
            }
        });
        let push_q = q.clone();
        let _ = thread::spawn(move || {
            for i in 0..N {
                push_q.push_back(i);
            }
        });

        let got: Vec<usize> = (0..N)
            .map(|_| {
                rx.recv_timeout(Duration::from_millis(500))
                    .expect("deadlock")
            })
            .collect();
        assert_eq!(got, (0..N).collect::<Vec<_>>());
    }
}
