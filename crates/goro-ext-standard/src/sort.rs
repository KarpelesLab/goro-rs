//! PHP's sort algorithm.
//!
//! The order of the comparisons PHP's sort makes is observable (comparison
//! callbacks with side effects, inconsistent comparators, non-transitive
//! comparisons of mixed types), so this reproduces it exactly. The
//! algorithm was reconstructed from the comparator calls `usort()` makes on
//! PHP 8.5 (fixtures tests/fixtures/sort_trace.txt; every permutation up to
//! six elements and random inputs up to 5000 match call for call):
//!
//! - up to 16 elements, an insertion sort: two elements are compared once;
//!   three to five start with a three-element decision tree then insert the
//!   rest scanning backwards; from six elements on, each new element is
//!   compared with the last sorted one, then (once six or more are sorted)
//!   with every second one going backwards, the skipped one deciding last;
//! - beyond 16 elements, a quicksort: the pivot is the median of the first,
//!   middle and last elements (of five elements spread over the range from
//!   1024 elements on), moved next to the first element; the partition
//!   scans inwards from both ends; the smaller side is sorted first.
//!
//! Since PHP 8.0 sorting is stable (RFC wiki.php.net/rfc/stable_sorting):
//! elements comparing equal keep their original order. [`stable_sort_by`]
//! adds that tie-break; [`sort_by`] is the raw algorithm.

use std::cmp::Ordering;

fn gt<T, F: FnMut(&T, &T) -> Ordering>(v: &[T], a: usize, b: usize, cmp: &mut F) -> bool {
    cmp(&v[a], &v[b]) == Ordering::Greater
}

/// The three-element decision tree, on positions `p`.
fn sort3<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], p: [usize; 3], cmp: &mut F) {
    let [x, y, z] = p;
    if !gt(v, x, y, cmp) {
        if !gt(v, y, z, cmp) {
            return;
        }
        v.swap(y, z);
        if gt(v, x, y, cmp) {
            v.swap(x, y);
        }
    } else {
        if cmp(&v[z], &v[y]) == Ordering::Less {
            v.swap(x, z);
            return;
        }
        v.swap(x, y);
        if gt(v, y, z, cmp) {
            v.swap(y, z);
        }
    }
}

/// Sorts the elements at positions `p` (two to five of them, not
/// necessarily contiguous).
fn small_sort<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], p: &[usize], cmp: &mut F) {
    match p.len() {
        0 | 1 => {}
        2 => {
            if gt(v, p[0], p[1], cmp) {
                v.swap(p[0], p[1]);
            }
        }
        _ => {
            sort3(v, [p[0], p[1], p[2]], cmp);
            for i in 3..p.len() {
                // Linear insertion: the element walks down while its left
                // neighbour is greater.
                let mut j = i;
                while j > 0 && gt(v, p[j - 1], p[j], cmp) {
                    v.swap(p[j - 1], p[j]);
                    j -= 1;
                }
            }
        }
    }
}

/// Insertion sort of `v[lo..hi]`, at most 16 elements in PHP's use.
fn insertion_sort<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], lo: usize, hi: usize, cmp: &mut F) {
    let n = hi - lo;
    if n <= 5 {
        let p: [usize; 5] = std::array::from_fn(|k| lo + k);
        small_sort(v, &p[..n], cmp);
        return;
    }
    for i in lo + 1..hi {
        if !gt(v, i - 1, i, cmp) {
            continue;
        }
        let pos = if i - lo < 6 {
            let mut j = i - 1;
            while j > lo && gt(v, j - 1, i, cmp) {
                j -= 1;
            }
            j
        } else {
            // Every second element going back, then the skipped one.
            let mut j = i as isize - 1;
            let lo_i = lo as isize;
            loop {
                j -= 2;
                if j < lo_i {
                    if j + 1 >= lo_i {
                        let s = (j + 1) as usize;
                        break if cmp(&v[i], &v[s]) == Ordering::Less {
                            s
                        } else {
                            s + 1
                        };
                    }
                    break lo;
                }
                if !gt(v, j as usize, i, cmp) {
                    let s = (j + 1) as usize;
                    break if gt(v, s, i, cmp) { s } else { s + 1 };
                }
            }
        };
        v[pos..=i].rotate_right(1);
    }
}

fn quick_sort<T, F: FnMut(&T, &T) -> Ordering>(
    v: &mut [T],
    mut lo: usize,
    mut hi: usize,
    cmp: &mut F,
) {
    while hi - lo > 16 {
        let n = hi - lo;
        let mid = lo + n / 2;
        if n >= 1024 {
            let d = n / 4;
            small_sort(v, &[lo, lo + d, mid, mid + d, hi - 1], cmp);
        } else {
            sort3(v, [lo, mid, hi - 1], cmp);
        }
        v.swap(lo + 1, mid);
        let pv = lo + 1;
        let mut i = lo + 2;
        let mut j = hi - 2;
        loop {
            let mut stop = false;
            while gt(v, pv, i, cmp) {
                i += 1;
                if i > j {
                    stop = true;
                    break;
                }
            }
            if stop || i >= j {
                break;
            }
            while gt(v, j, pv, cmp) {
                j -= 1;
                if j <= i {
                    stop = true;
                    break;
                }
            }
            if stop {
                break;
            }
            v.swap(i, j);
            i += 1;
            j -= 1;
            if i > j {
                break;
            }
        }
        let p = i - 1;
        v.swap(pv, p);
        if p - lo < hi - (p + 1) {
            quick_sort(v, lo, p, cmp);
            lo = p + 1;
        } else {
            quick_sort(v, p + 1, hi, cmp);
            hi = p;
        }
    }
    insertion_sort(v, lo, hi, cmp);
}

/// Sorts `v` with PHP's algorithm, making the same comparisons in the same
/// order. Not stable by itself: see [`stable_sort_by`].
pub fn sort_by<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], mut cmp: F) {
    let n = v.len();
    if n > 1 {
        quick_sort(v, 0, n, &mut cmp);
    }
}

/// Sorts `v` as PHP 8 does: [`sort_by`] with equal elements ordered by
/// their original position. `cmp` sees the elements themselves.
pub fn stable_sort_by<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], mut cmp: F) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let mut order: Vec<usize> = (0..n).collect();
    sort_by(&mut order, |&a, &b| cmp(&v[a], &v[b]).then(a.cmp(&b)));
    // Apply the permutation in place: position k receives order[k].
    let mut done = vec![false; n];
    for start in 0..n {
        if done[start] {
            continue;
        }
        let mut k = start;
        loop {
            done[k] = true;
            let src = order[k];
            if src == start {
                break;
            }
            v.swap(k, src);
            k = src;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(input: &[i32]) -> (Vec<(i32, i32)>, Vec<i32>) {
        let mut v = input.to_vec();
        let mut calls = Vec::new();
        sort_by(&mut v, |a, b| {
            calls.push((*a, *b));
            a.cmp(b)
        });
        (calls, v)
    }

    #[test]
    fn small_traces() {
        // Observed on PHP 8.5 (usort with a logging comparator).
        assert_eq!(trace(&[3, 2, 1]).0, [(3, 2), (1, 2)]);
        assert_eq!(trace(&[2, 3, 1]).0, [(2, 3), (3, 1), (2, 1)]);
        assert_eq!(
            trace(&[4, 3, 2, 1]).0,
            [(4, 3), (2, 3), (4, 1), (3, 1), (2, 1)]
        );
        assert_eq!(
            trace(&[7, 6, 5, 4, 3, 2, 1]).0[15..],
            [(7, 1), (5, 1), (3, 1), (1, 2)]
        );
    }

    #[test]
    fn stable() {
        let mut v = vec![(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd'), (1, 'e')];
        stable_sort_by(&mut v, |a, b| a.0.cmp(&b.0));
        assert_eq!(v, [(0, 'b'), (0, 'd'), (1, 'a'), (1, 'c'), (1, 'e')]);
        let mut big: Vec<(usize, usize)> = (0..3000).map(|i| ((i * 7919) % 13, i)).collect();
        stable_sort_by(&mut big, |a, b| a.0.cmp(&b.0));
        assert!(
            big.windows(2)
                .all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1))
        );
    }
}
