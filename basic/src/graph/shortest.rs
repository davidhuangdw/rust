use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

fn spfa_neg_height(adj: &[Vec<(usize, isize)>]) -> Option<Vec<isize>> {
    let n = adj.len();
    let mut h: Vec<isize> = vec![0; n];
    if n == 0 {
        return Some(h);
    };

    let mut que: VecDeque<usize> = (0..n).collect();
    let mut qued = vec![true; n];

    for _ in 0..n {
        let k = que.len();
        for _ in 0..k {
            let u = que.pop_front().unwrap();
            qued[u] = false;
            for &(v, w) in &adj[u] {
                if h[u] + w < h[v] {
                    h[v] = h[u] + w;
                    if !qued[v] {
                        qued[v] = true;
                        que.push_back(v);
                    }
                }
            }
        }
        if que.is_empty() {
            return Some(h);
        }
    }
    None
}

pub fn dijkstra(i: usize, adj: &[Vec<(usize, isize)>]) -> Vec<Option<isize>> {
    let mut d = vec![None; adj.len()];
    d[i] = Some(0);
    let mut hp = BinaryHeap::from([Reverse((0, i))]);

    while let Some(Reverse((du, u))) = hp.pop() {
        if d[u].is_some_and(|best| best < du) {
            continue;
        }
        for &(v, w) in &adj[u] {
            let nd = du + w;
            if d[v].is_none_or(|best| du + w < best) {
                d[v] = Some(nd);
                hp.push(Reverse((du + w, v)));
            }
        }
    }
    d
}

pub fn floyd(adj: &[Vec<(usize, isize)>]) -> Option<Vec<Vec<Option<isize>>>> {
    let n = adj.len();
    let mut d = vec![vec![None; n]; n];

    for i in 0..n {
        d[i][i] = Some(0);
        for &(j, w) in &adj[i] {
            d[i][j] = Some(d[i][j].map_or(w, |cur: isize| cur.min(w)));
        }
    }

    for k in 0..n {
        for i in 0..n {
            let Some(dik) = d[i][k] else { continue };
            for j in 0..n {
                let Some(dkj) = d[k][j] else { continue };
                d[i][j] = Some(d[i][j].map_or(dik + dkj, |cur| cur.min(dik + dkj)));
            }
        }
    }
    if (0..n).any(|i| d[i][i].is_some_and(|x| x < 0)) {
        return None;
    }
    Some(d)
}

// Johnson's algorithm for neg-weight all-pairs shorted-path
pub fn johnson_shortest_neg(adj: &[Vec<(usize, isize)>]) -> Option<Vec<Vec<Option<isize>>>> {
    let h = spfa_neg_height(adj)?;

    // w(u, v)' = w(u, v) + h[u] - h[v] >= 0
    let n = adj.len();
    let pos_adj: Vec<Vec<(usize, isize)>> = adj
        .iter()
        .enumerate()
        .map(|(u, edges)| edges.iter().map(|&(v, w)| (v, w + h[u] - h[v])).collect())
        .collect();
    Some(
        (0..n)
            .map(|i| {
                dijkstra(i, &pos_adj)
                    .iter()
                    .enumerate()
                    .map(|(j, w)| w.map(|x| x - (h[i] - h[j])))
                    .collect()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dijkstra_basic() {
        // shortest path via intermediate node, unreachable node, non-zero source, zero-weight cycle
        let adj = vec![vec![(1, 4), (2, 1)], vec![(2, 0)], vec![(1, 2)], vec![]];
        assert_eq!(vec![Some(0), Some(3), Some(1), None], dijkstra(0, &adj));
        assert_eq!(vec![None, Some(0), Some(0), None], dijkstra(1, &adj));
    }

    #[test]
    fn dijkstra_matches_floyd_random() {
        let mut seed = 7u64;
        for _ in 0..30 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = 1 + (seed % 6) as usize;
            let mut adj = vec![vec![]; n];
            for u in 0..n {
                for v in 0..n {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if seed % 3 == 0 {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        adj[u].push((v, (seed % 10) as isize));
                    }
                }
            }
            let want = floyd(&adj).unwrap();
            for (src, row) in want.iter().enumerate() {
                assert_eq!(*row, dijkstra(src, &adj), "src = {src}, adj = {adj:?}");
            }
        }
    }

    #[test]
    fn edge_cases() {
        // empty & single node
        assert_eq!(Some(vec![]), floyd(&[]));
        assert_eq!(Some(vec![]), johnson_shortest_neg(&[]));
        assert_eq!(Some(vec![vec![Some(0)]]), floyd(&[vec![]]));
        assert_eq!(Some(vec![vec![Some(0)]]), johnson_shortest_neg(&[vec![]]));

        // negative edges, no negative cycle
        let adj = vec![vec![(1, 4), (2, 5)], vec![(2, -3)], vec![]];
        let want = Some(vec![
            vec![Some(0), Some(4), Some(1)],
            vec![None, Some(0), Some(-3)],
            vec![None, None, Some(0)],
        ]);
        assert_eq!(want, floyd(&adj));
        assert_eq!(want, johnson_shortest_neg(&adj));

        // negative cycle
        let neg_cycle = vec![vec![(1, 1)], vec![(2, -3)], vec![(0, 1)]];
        assert_eq!(None, floyd(&neg_cycle));
        assert_eq!(None, johnson_shortest_neg(&neg_cycle));
    }

    #[test]
    fn floyd_matches_johnson_random() {
        let mut seed = 42u64;
        for _ in 0..30 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = 1 + (seed % 6) as usize;
            let mut adj = vec![vec![]; n];
            for u in 0..n {
                for v in 0..n {
                    if u == v {
                        continue;
                    }
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if seed % 3 == 0 {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let w = (seed % 16) as isize - 5;
                        adj[u].push((v, w));
                    }
                }
            }
            assert_eq!(johnson_shortest_neg(&adj), floyd(&adj), "adj = {adj:?}");
        }
    }
}
