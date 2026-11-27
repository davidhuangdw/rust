// longest-prefix s[i..i+k] <-> longest(i+k-1)

use std::iter::repeat_n;

// return LCP-len of each s[i:] to s
pub fn z_func(s: &[char]) -> Vec<usize> {
    let n = s.len();
    let (mut l, mut r) = (0, 0);
    let mut z: Vec<usize> = repeat_n(0, n).collect();
    for i in 1..n {
        let mut ir = if i < r { r.min(i + z[i - l]) } else { i };
        while ir < n && s[ir] == s[ir - i] {
            ir += 1;
        }
        if ir > r {
            (l, r) = (i, ir);
        }
        z[i] = ir - i;
    }
    z
}

// search = LCP(z_func) of 'pat+target'
fn search_chars(pat: &[char], target: &[char]) -> Vec<usize> {
    let full = [pat, &['#'], target].concat();
    let z = z_func(full.as_slice());
    let m = pat.len();
    (0..target.len()).filter(|i| z[m + 1 + i] >= m).collect()
}

// search each full-match' start-index
pub fn search(pat: &str, target: &str) -> Vec<usize> {
    let pat: Vec<char> = pat.chars().collect();
    let target: Vec<char> = target.chars().collect();
    search_chars(&pat, &target)
}

#[cfg(test)]
mod tests {
    use crate::string::test_util::strs;

    use super::*;

    /// Cross-checked against the definition: longest common prefix of `s` and `s[i..]`.
    #[test]
    fn z_func_matches_naive() {
        for s in strs(&['a', 'b', 'c'], 6) {
            let chars: Vec<char> = s.chars().collect();
            let want: Vec<usize> = (0..chars.len())
                .map(|i| {
                    if i == 0 {
                        0
                    } else {
                        chars
                            .iter()
                            .zip(&chars[i..])
                            .take_while(|(a, b)| a == b)
                            .count()
                    }
                })
                .collect();
            assert_eq!(want, z_func(&chars), "input {s:?}");
        }
    }

    /// Cross-checked against a naive `windows` scan.
    #[test]
    fn search_matches_naive() {
        let strs = strs(&['a', 'b', 'c'], 5);
        for p in strs.iter().filter(|p| (1..=4).contains(&p.len())) {
            let p_chars: Vec<char> = p.chars().collect();
            for s in &strs {
                let s_chars: Vec<char> = s.chars().collect();
                let want: Vec<usize> = s_chars
                    .windows(p_chars.len())
                    .enumerate()
                    .filter_map(|(i, w)| (w == p_chars.as_slice()).then_some(i))
                    .collect();
                assert_eq!(want, search(p, s), "pattern {p:?} in {s:?}");
            }
        }
    }

    #[test]
    fn edge_cases() {
        assert_eq!(Vec::<usize>::new(), z_func(&[]));
        assert_eq!(vec![0], z_func(&['a']));
        assert_eq!(vec![0], search("a", "a"));
        assert_eq!(Vec::<usize>::new(), search("abc", "ab"));
        assert_eq!(Vec::<usize>::new(), search("ab", ""));
        assert_eq!(vec![1, 2], search("aa", "baaa")); // overlapping matches
        assert_eq!(vec![2], search("世界", "你好世界")); // char index, not byte offset
    }
}
