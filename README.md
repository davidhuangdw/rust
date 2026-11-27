# rust

Classic and parallel data structures and algorithms in Rust, organized as a Cargo workspace with two crates:

| Crate | Contents |
| --- | --- |
| `basic` | data structures and algorithms |
| `parallel` | concurrent data structures & algorithms |

```sh
cargo test
cargo doc --no-deps --open
```

## Modules (`basic`)

| Module | Contents |
| --- | --- |
| `bst` | Treap, multiset |
| `graph` | LCA, shortest paths (Dijkstra, Floyd, Johnson), topological order |
| `interval` | Segment tree, binary indexed tree |
| `linked_list` | Singly/doubly linked, array-backed, tuple-based list |
| `misc` | Binary heap, monotonic queue, union-find |
| `sort` | Quick sort, k-th of two/many sorted arrays, 2/3/4-sum |
| `string` | KMP, Manacher, min rotation, trie, Z-algorithm |

## Modules (`parallel`)

| Module | Contents |
| --- | --- |
| `list` | Hand-over-hand, optimistic, lazy, and lock-free (Harris/Michael) sorted lists |
| `monitor` | Bounded queue (condvar), reentrant lock, FIFO reader-writer lock |
| `mutex` | Lamport bakery lock (N-thread), Peterson lock (2-thread) |
| `que` | Michael–Scott lock-free queue; dual rendezvous sync-queue |
| `registers` | Atomic snapshots: obstruction-free (two-pass validation) and wait-free (HS collect) |
| `spin` | TTAS lock, CLH queue lock (FIFO), array queue lock (FIFO) |
| `stack` | Lock-free (Treiber) stack |

## Lock-free notes

- **Snapshot validation.** Take two passes of `load` (they need not observe the same global order). If they match, the values in the *innermost* interval — from the last read of pass 1 to the first read of pass 2 — are unchanged, so the condition is checked there.
- **One CAS decides one atomic method.** Each mutating operation is a bijection with a single successful `CAS`:
  ```rust
  loop { if pre_cond.all() { cas() } else { retry } }
  ```
  The catch: `cas(last_load, new)` must be valid at the instant it succeeds, yet there is a **gap between the snapshot interval and the CAS instant**.
- **Closing the gap:**
  - *Immutable fields* — never need reloading.
  - *Monotonicity* — use a strong CAS whose match includes the precondition (a necessary condition):
    - `tail.next.CAS(null, ..)` succeeding proves `tail` is still the tail.
    - condition = "mark bit set", and once set it never rolls back.
  - *Tagged pointers* — bundle the condition and the pointer so the CAS is itself the strong check: `CAS(mark removed)` then try-forward.
  - *Avoid ABA* — pointer nodes + epoch-based reclamation guarantee a node is not recycled, so a stale pointer can't alias a fresh one.

## Layout

```
Cargo.toml        workspace
basic/src/<topic>/<algorithm>.rs
parallel/src/<topic>/<algorithm>.rs
```

Tests live next to their implementation in `#[cfg(test)] mod tests`.
