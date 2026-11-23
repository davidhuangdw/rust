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
| `graph` | LCA, topological order |
| `interval` | Segment tree, binary indexed tree |
| `linked_list` | Singly/doubly linked, array-backed, tuple-based list |
| `misc` | Binary heap, monotonic queue, union-find |
| `sort` | Quick sort, k-th of two/many sorted arrays, 2/3/4-sum |
| `string` | KMP, Manacher, min rotation, trie |

## Modules (`parallel`)

| Module | Contents |
| --- | --- |
| `list` | Hand-over-hand, optimistic, and lock-free (Harris/Michael) sorted lists |
| `monitor` | Bounded queue (condvar), reentrant lock, FIFO reader-writer lock |
| `mutex` | Lamport bakery lock (N-thread), Peterson lock (2-thread) |
| `que` | Michael–Scott lock-free queue |
| `registers` | Atomic snapshot (N-register, obstruction-free) |
| `spin` | TTAS lock, CLH queue lock (FIFO), array queue lock (FIFO) |

## Layout

```
Cargo.toml        workspace
basic/src/<topic>/<algorithm>.rs
parallel/src/<topic>/<algorithm>.rs
```

Tests live next to their implementation in `#[cfg(test)] mod tests`.
