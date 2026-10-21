//! Ports of Zend/zend_sort.c (hybrid insertion sort + quicksort) and
//! ext/standard/strnatcmp.c.
//!
//! PHP's sort functions are observable beyond their result: user comparators
//! see the exact sequence of comparisons, and non-transitive comparisons (mixed
//! types under SORT_REGULAR) produce algorithm-dependent orders. These ports
//! keep goro's behaviour identical to php-src.

/// `cmp(a, b) > 0` means a sorts after b (zend_sort only ever tests `> 0`).
fn sort2<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, cmp: &mut F) {
    if cmp(&v[a], &v[b]) > 0 {
        v.swap(a, b);
    }
}

fn sort3<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, c: usize, cmp: &mut F) {
    if !(cmp(&v[a], &v[b]) > 0) {
        if !(cmp(&v[b], &v[c]) > 0) {
            return;
        }
        v.swap(b, c);
        if cmp(&v[a], &v[b]) > 0 {
            v.swap(a, b);
        }
        return;
    }
    if !(cmp(&v[c], &v[b]) > 0) {
        v.swap(a, c);
        return;
    }
    v.swap(a, b);
    if cmp(&v[b], &v[c]) > 0 {
        v.swap(b, c);
    }
}

fn sort4<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, c: usize, d: usize, cmp: &mut F) {
    sort3(v, a, b, c, cmp);
    if cmp(&v[c], &v[d]) > 0 {
        v.swap(c, d);
        if cmp(&v[b], &v[c]) > 0 {
            v.swap(b, c);
            if cmp(&v[a], &v[b]) > 0 {
                v.swap(a, b);
            }
        }
    }
}

fn sort5<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], a: usize, b: usize, c: usize, d: usize, e: usize, cmp: &mut F) {
    sort4(v, a, b, c, d, cmp);
    if cmp(&v[d], &v[e]) > 0 {
        v.swap(d, e);
        if cmp(&v[c], &v[d]) > 0 {
            v.swap(c, d);
            if cmp(&v[b], &v[c]) > 0 {
                v.swap(b, c);
                if cmp(&v[a], &v[b]) > 0 {
                    v.swap(a, b);
                }
            }
        }
    }
}

/// zend_insert_sort()
pub fn zend_insert_sort<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], cmp: &mut F) {
    let n = v.len();
    match n {
        0 | 1 => {}
        2 => sort2(v, 0, 1, cmp),
        3 => sort3(v, 0, 1, 2, cmp),
        4 => sort4(v, 0, 1, 2, 3, cmp),
        5 => sort5(v, 0, 1, 2, 3, 4, cmp),
        _ => {
            let sentry = 6.min(n);
            for i in 1..sentry {
                let mut j = i - 1;
                if !(cmp(&v[j], &v[i]) > 0) {
                    continue;
                }
                while j != 0 {
                    j -= 1;
                    if !(cmp(&v[j], &v[i]) > 0) {
                        j += 1;
                        break;
                    }
                }
                let mut k = i;
                while k > j {
                    v.swap(k, k - 1);
                    k -= 1;
                }
            }
            for i in sentry..n {
                let mut j = i - 1;
                if !(cmp(&v[j], &v[i]) > 0) {
                    continue;
                }
                loop {
                    j -= 2;
                    if !(cmp(&v[j], &v[i]) > 0) {
                        j += 1;
                        if !(cmp(&v[j], &v[i]) > 0) {
                            j += 1;
                        }
                        break;
                    }
                    if j == 0 {
                        break;
                    }
                    if j == 1 {
                        j -= 1;
                        if cmp(&v[i], &v[j]) > 0 {
                            j += 1;
                        }
                        break;
                    }
                }
                let mut k = i;
                while k > j {
                    v.swap(k, k - 1);
                    k -= 1;
                }
            }
        }
    }
}

/// zend_sort(): hybrid insertion sort / quicksort.
pub fn zend_sort<T, F: FnMut(&T, &T) -> i32>(v: &mut [T], cmp: &mut F) {
    let mut base = 0usize;
    let mut nmemb = v.len();
    loop {
        if nmemb <= 16 {
            zend_insert_sort(&mut v[base..base + nmemb], cmp);
            return;
        }
        let start = base;
        let end = base + nmemb;
        let offset = nmemb >> 1;
        let pivot0 = start + offset;
        if (nmemb >> 10) != 0 {
            let delta = offset >> 1;
            sort5(v, start, start + delta, pivot0, pivot0 + delta, end - 1, cmp);
        } else {
            sort3(v, start, pivot0, end - 1, cmp);
        }
        v.swap(start + 1, pivot0);
        let pivot = start + 1;
        let mut i = pivot + 1;
        let mut j = end - 1;
        'outer: loop {
            while cmp(&v[pivot], &v[i]) > 0 {
                i += 1;
                if i == j {
                    break 'outer;
                }
            }
            j -= 1;
            if j == i {
                break 'outer;
            }
            while cmp(&v[j], &v[pivot]) > 0 {
                j -= 1;
                if j == i {
                    break 'outer;
                }
            }
            v.swap(i, j);
            i += 1;
            if i == j {
                break 'outer;
            }
        }
        v.swap(pivot, i - 1);
        if (i - 1) - start < end - i {
            zend_sort(&mut v[start..i - 1], cmp);
            base = i;
            nmemb = end - i;
        } else {
            zend_sort(&mut v[i..end], cmp);
            base = start;
            nmemb = (i - start) - 1;
        }
    }
}

/// Sort `items` (each tagged with its original position) with zend_sort, using
/// PHP's stable fallback (original order) when `cmp` returns 0.
pub fn zend_sort_stable<T, F: FnMut(&T, &T) -> i32>(items: Vec<T>, mut cmp: F) -> Vec<T> {
    let mut tagged: Vec<(usize, T)> = items.into_iter().enumerate().collect();
    if tagged.len() > 1 {
        zend_sort(&mut tagged, &mut |a: &(usize, T), b: &(usize, T)| {
            let r = cmp(&a.1, &b.1);
            if r != 0 {
                r
            } else if a.0 > b.0 {
                1
            } else if a.0 < b.0 {
                -1
            } else {
                0
            }
        });
    }
    tagged.into_iter().map(|(_, t)| t).collect()
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn compare_right(a: &[u8], ap: &mut usize, b: &[u8], bp: &mut usize) -> i32 {
    let mut bias = 0;
    loop {
        let a_end = *ap >= a.len() || !a[*ap].is_ascii_digit();
        let b_end = *bp >= b.len() || !b[*bp].is_ascii_digit();
        if a_end && b_end {
            return bias;
        } else if a_end {
            return -1;
        } else if b_end {
            return 1;
        } else if a[*ap] < b[*bp] {
            if bias == 0 {
                bias = -1;
            }
        } else if a[*ap] > b[*bp] && bias == 0 {
            bias = 1;
        }
        *ap += 1;
        *bp += 1;
    }
}

fn compare_left(a: &[u8], ap: &mut usize, b: &[u8], bp: &mut usize) -> i32 {
    loop {
        let a_end = *ap >= a.len() || !a[*ap].is_ascii_digit();
        let b_end = *bp >= b.len() || !b[*bp].is_ascii_digit();
        if a_end && b_end {
            return 0;
        } else if a_end {
            return -1;
        } else if b_end {
            return 1;
        } else if a[*ap] < b[*bp] {
            return -1;
        } else if a[*ap] > b[*bp] {
            return 1;
        }
        *ap += 1;
        *bp += 1;
    }
}

/// strnatcmp_ex()
pub fn strnatcmp_ex(a: &[u8], b: &[u8], fold_case: bool) -> i32 {
    if a.is_empty() || b.is_empty() {
        return if a.len() == b.len() { 0 } else if a.len() > b.len() { 1 } else { -1 };
    }
    let at = |i: usize| -> u8 { if i < a.len() { a[i] } else { 0 } };
    let bt = |i: usize| -> u8 { if i < b.len() { b[i] } else { 0 } };
    let mut ap = 0usize;
    let mut bp = 0usize;
    let mut ca = at(ap);
    let mut cb = bt(bp);
    while ca == b'0' && ap + 1 < a.len() && a[ap + 1].is_ascii_digit() {
        ap += 1;
        ca = at(ap);
    }
    while cb == b'0' && bp + 1 < b.len() && b[bp + 1].is_ascii_digit() {
        bp += 1;
        cb = bt(bp);
    }
    loop {
        while is_space(ca) {
            ap += 1;
            ca = at(ap);
        }
        while is_space(cb) {
            bp += 1;
            cb = bt(bp);
        }
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let fractional = ca == b'0' || cb == b'0';
            let result = if fractional {
                compare_left(a, &mut ap, b, &mut bp)
            } else {
                compare_right(a, &mut ap, b, &mut bp)
            };
            if result != 0 {
                return result;
            } else if ap >= a.len() && bp >= b.len() {
                return 0;
            } else if ap >= a.len() {
                return -1;
            } else if bp >= b.len() {
                return 1;
            }
            ca = at(ap);
            cb = bt(bp);
        }
        if fold_case {
            ca = ca.to_ascii_uppercase();
            cb = cb.to_ascii_uppercase();
        }
        if ca < cb {
            return -1;
        } else if ca > cb {
            return 1;
        }
        ap += 1;
        bp += 1;
        if ap >= a.len() && bp >= b.len() {
            return 0;
        } else if ap >= a.len() {
            return -1;
        } else if bp >= b.len() {
            return 1;
        }
        ca = at(ap);
        cb = bt(bp);
    }
}
