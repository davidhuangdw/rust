use std::{cell::RefCell, rc::Rc};

use rand::{RngExt, rng};

type NodeRef<T> = Rc<RefCell<Node<T>>>;
type Link<T> = Option<NodeRef<T>>;

pub struct SkipList<T> {
    head: NodeRef<T>,
    h: usize,
}

struct Node<T> {
    val: Option<T>,
    nexts: Vec<Link<T>>,
}

impl<T> Node<T> {
    fn new(val: Option<T>, height: usize) -> Self {
        Self {
            val,
            nexts: vec![None; height],
        }
    }
}

impl<T: Ord> SkipList<T> {
    pub fn new(height: usize) -> Self {
        assert!(height >= 1);
        Self {
            h: height,
            head: Rc::new(RefCell::new(Node::new(None, height))),
        }
    }

    pub fn contains(&self, v: &T) -> bool {
        self.walk(v).1.is_some()
    }

    pub fn insert(&mut self, v: T) -> bool {
        let (pres, hit) = self.walk(&v);
        if hit.is_some() {
            return false;
        }
        let mut r = rng();
        let h = 1 + (1..self.h).take_while(|_| r.random_bool(0.5)).count();
        let node = Rc::new(RefCell::new(Node::new(Some(v), h)));
        for (i, pre) in pres.iter().take(h).enumerate() {
            let mut pre_b = pre.borrow_mut();
            node.borrow_mut().nexts[i] = pre_b.nexts[i].take();
            pre_b.nexts[i] = Some(node.clone());
        }
        true
    }

    pub fn remove(&mut self, v: &T) -> bool {
        let (pres, Some(target)) = self.walk(v) else {
            return false;
        };
        let h = target.borrow().nexts.len();
        for (i, pre) in pres.iter().take(h).enumerate() {
            pre.borrow_mut().nexts[i] = target.borrow_mut().nexts[i].take();
        }
        true
    }

    // Returns (pre-nodes of each level `0..h`, matching node(==v))
    fn walk(&self, v: &T) -> (Vec<NodeRef<T>>, Link<T>) {
        let mut cur = self.head.clone();
        let mut pres: Vec<_> = (0..self.h)
            .rev()
            .map(|h| {
                while let Some(ref node) = cur.clone().borrow().nexts[h]
                    && node.borrow().val.as_ref() < Some(v)
                {
                    cur = node.clone();
                }
                cur.clone()
            })
            .collect();
        pres.reverse(); // careful: shouldn't rev().collect() to keep closure-fun order
        let hit = pres[0].borrow().nexts[0]
            .clone()
            .filter(|n| n.borrow().val.as_ref() == Some(v));
        (pres, hit)
    }
}

impl<T> Drop for SkipList<T> {
    fn drop(&mut self) {
        let mut cur = self.head.borrow_mut().nexts[0].take();
        while let Some(node) = cur {
            let mut b = node.borrow_mut();
            cur = b.nexts[0].take();
            b.nexts.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn edge_cases() {
        let mut sl = SkipList::new(4);
        assert!(!sl.contains(&1));
        assert!(!sl.remove(&1));

        assert!(sl.insert(1));
        assert!(!sl.insert(1));
        assert!(sl.contains(&1));

        assert!(sl.remove(&1));
        assert!(!sl.remove(&1));
        assert!(!sl.contains(&1));
        assert!(sl.insert(1));
    }

    #[test]
    fn matches_btreeset_random() {
        let mut sl = SkipList::new(8);
        let mut set = BTreeSet::new();
        let mut seed = 1u64;
        for _ in 0..500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let op = (seed >> 32) % 3;
            let val = ((seed >> 16) % 64) as i32;
            match op {
                0 => assert_eq!(set.insert(val), sl.insert(val)),
                1 => assert_eq!(set.remove(&val), sl.remove(&val)),
                _ => assert_eq!(set.contains(&val), sl.contains(&val)),
            }
        }
        for v in 0..64 {
            assert_eq!(set.contains(&v), sl.contains(&v));
        }
    }
}
