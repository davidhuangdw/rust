# rust

Classic data structures, algorithms and concurrency primitives in Rust, one module per topic. Cargo workspace with two crates:

| Crate | Contents |
| --- | --- |
| `basic` | data structures and algorithms |
| `parallel` | multi-thread algorithms |

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
| `mutex` | N-thread Lamport bakery lock; Two-thread Peterson lock; |

## Layout

```
Cargo.toml        workspace
basic/src/<topic>/<algorithm>.rs
parallel/src/<topic>/<algorithm>.rs
```

Tests live next to their implementation in `#[cfg(test)] mod tests`.
