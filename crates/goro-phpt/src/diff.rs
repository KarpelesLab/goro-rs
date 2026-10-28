//! A small line diff for failure reports (`-v`).

use crate::expect::{Kind, Matcher};

/// Formats a unified-style diff of expected vs actual output. Lines are
/// compared with the EXPECTF/EXPECTREGEX pattern of each expected line when
/// the kind allows, so that only lines that really differ are marked.
pub fn diff(kind: Kind, expected: &[u8], actual: &[u8], context: usize, max_lines: usize) -> String {
    let e: Vec<&[u8]> = split(expected);
    let a: Vec<&[u8]> = split(actual);
    // One compiled pattern per expected line that has one.
    let pats: Vec<Option<Matcher>> = e
        .iter()
        .map(|l| match kind {
            Kind::Exact => None,
            Kind::Format if !l.contains(&b'%') => None,
            _ => Matcher::new(kind, l).ok(),
        })
        .collect();
    let eq = |i: usize, line: &[u8]| match &pats[i] {
        Some(m) => e[i] == line || m.is_match(line),
        None => e[i] == line,
    };
    // Strip the common prefix and suffix so the quadratic part stays small.
    let mut pre = 0;
    while pre < e.len() && pre < a.len() && eq(pre, a[pre]) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < e.len() - pre
        && suf < a.len() - pre
        && eq(e.len() - 1 - suf, a[a.len() - 1 - suf])
    {
        suf += 1;
    }
    let em = &e[pre..e.len() - suf];
    let am = &a[pre..a.len() - suf];
    let mut ops: Vec<(char, usize, usize)> = Vec::new(); // (op, e index, a index)
    for i in 0..pre {
        ops.push((' ', i, i));
    }
    if em.len().saturating_mul(am.len()) <= 4_000_000 {
        // LCS table.
        let (n, m) = (em.len(), am.len());
        let mut t = vec![0u32; (n + 1) * (m + 1)];
        let idx = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                t[idx(i, j)] = if eq(pre + i, am[j]) {
                    t[idx(i + 1, j + 1)] + 1
                } else {
                    t[idx(i + 1, j)].max(t[idx(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && eq(pre + i, am[j]) {
                ops.push((' ', pre + i, pre + j));
                i += 1;
                j += 1;
            } else if i < n && (j == m || t[idx(i + 1, j)] >= t[idx(i, j + 1)]) {
                ops.push(('-', pre + i, 0));
                i += 1;
            } else {
                ops.push(('+', 0, pre + j));
                j += 1;
            }
        }
    } else {
        for i in 0..em.len() {
            ops.push(('-', pre + i, 0));
        }
        for j in 0..am.len() {
            ops.push(('+', 0, pre + j));
        }
    }
    for k in 0..suf {
        ops.push((' ', e.len() - suf + k, a.len() - suf + k));
    }
    // Print changed lines with context.
    let mut keep = vec![false; ops.len()];
    for (k, op) in ops.iter().enumerate() {
        if op.0 != ' ' {
            let lo = k.saturating_sub(context);
            let hi = (k + context + 1).min(ops.len());
            keep[lo..hi].iter_mut().for_each(|x| *x = true);
        }
    }
    let mut out = String::new();
    let mut printed = 0;
    let mut gap = false;
    for (k, &(op, ei, ai)) in ops.iter().enumerate() {
        if !keep[k] {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push_str("@@\n");
        }
        gap = false;
        if printed >= max_lines {
            out.push_str("... (diff truncated)\n");
            break;
        }
        let line = if op == '+' { a[ai] } else { e[ei] };
        let num = if op == '+' { ai + 1 } else { ei + 1 };
        out.push_str(&format!("{num:04}{op} {}\n", String::from_utf8_lossy(line)));
        printed += 1;
    }
    out
}

fn split(b: &[u8]) -> Vec<&[u8]> {
    if b.is_empty() {
        return Vec::new();
    }
    b.split(|&c| c == b'\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_changes() {
        let d = diff(Kind::Exact, b"a\nb\nc", b"a\nx\nc", 1, 100);
        assert_eq!(d, "0001  a\n0002- b\n0002+ x\n0003  c\n");
    }

    #[test]
    fn pattern_lines_match() {
        let d = diff(Kind::Format, b"int(%d)\nfoo", b"int(3)\nbar", 0, 100);
        assert_eq!(d, "0002- foo\n0002+ bar\n");
    }

    #[test]
    fn empty_sides() {
        assert_eq!(diff(Kind::Exact, b"", b"x", 3, 100), "0001+ x\n");
        assert_eq!(diff(Kind::Exact, b"x", b"", 3, 100), "0001- x\n");
    }
}
