//! PDO's SQL placeholder scanner (PHP manual: PDO::prepare, "Prepared
//! statements"; RFC "PDO driver specific SQL parsers" for the quoting
//! rules and the `??` escape of PHP 8.4): finds `?` and `:name`
//! placeholders outside string literals, quoted identifiers and
//! comments, and rewrites a query for a driver that takes another
//! placeholder style or none (emulated prepares).
//!
//! pdo_sqlite prepares both styles natively, so this serves drivers
//! without named placeholders (and emulation) through [`rewrite`].

/// A placeholder found in a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholder {
    /// Byte range in the query.
    pub start: usize,
    pub end: usize,
    /// `:name` (with the colon), or `None` for `?`.
    pub name: Option<Vec<u8>>,
}

/// The quoting rules of a driver's SQL dialect.
#[derive(Clone, Copy, Debug)]
pub struct Dialect {
    /// `"…"` is a quoted identifier/string (no placeholders inside).
    pub double_quotes: bool,
    /// `` `…` `` quotes (MySQL, SQLite).
    pub backticks: bool,
    /// `[…]` quotes (SQLite, SQL Server).
    pub brackets: bool,
    /// Backslash escapes inside strings (MySQL).
    pub backslash: bool,
    /// `#` starts a comment (MySQL).
    pub hash_comments: bool,
    /// `$tag$…$tag$` strings (PostgreSQL).
    pub dollar_quotes: bool,
}

pub const SQLITE: Dialect = Dialect {
    double_quotes: true,
    backticks: true,
    brackets: true,
    backslash: false,
    hash_comments: false,
    dollar_quotes: false,
};

/// What scanning found: the placeholders and the `??` escapes (each a
/// position of the first `?`, to be written as one `?`).
#[derive(Debug, Default)]
pub struct Scan {
    pub placeholders: Vec<Placeholder>,
    pub escapes: Vec<usize>,
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Scans `sql` for placeholders.
pub fn scan(sql: &[u8], d: &Dialect) -> Scan {
    let mut out = Scan::default();
    let mut i = 0;
    let n = sql.len();
    let skip_quoted = |i: usize, q: u8| -> usize {
        let mut j = i + 1;
        while j < n {
            if d.backslash && sql[j] == b'\\' {
                j += 2;
                continue;
            }
            if sql[j] == q {
                if j + 1 < n && sql[j + 1] == q {
                    j += 2;
                    continue;
                }
                return j + 1;
            }
            j += 1;
        }
        n
    };
    while i < n {
        let b = sql[i];
        match b {
            b'\'' => i = skip_quoted(i, b'\''),
            b'"' if d.double_quotes => i = skip_quoted(i, b'"'),
            b'`' if d.backticks => i = skip_quoted(i, b'`'),
            b'[' if d.brackets => {
                i = match sql[i..].iter().position(|&c| c == b']') {
                    Some(p) => i + p + 1,
                    None => n,
                }
            }
            b'-' if sql.get(i + 1) == Some(&b'-') => {
                i = match sql[i..].iter().position(|&c| c == b'\n') {
                    Some(p) => i + p + 1,
                    None => n,
                }
            }
            b'#' if d.hash_comments => {
                i = match sql[i..].iter().position(|&c| c == b'\n') {
                    Some(p) => i + p + 1,
                    None => n,
                }
            }
            b'/' if sql.get(i + 1) == Some(&b'*') => {
                i = match sql[i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(p) => i + 2 + p + 2,
                    None => n,
                }
            }
            b'$' if d.dollar_quotes => {
                // $tag$ … $tag$
                let mut j = i + 1;
                while j < n && is_name_byte(sql[j]) && sql[j] != b'$' {
                    j += 1;
                }
                if j < n && sql[j] == b'$' {
                    let tag = &sql[i..=j];
                    i = match sql[j + 1..].windows(tag.len()).position(|w| w == tag) {
                        Some(p) => j + 1 + p + tag.len(),
                        None => n,
                    };
                } else {
                    i += 1;
                }
            }
            b'?' => {
                if sql.get(i + 1) == Some(&b'?') {
                    out.escapes.push(i);
                    i += 2;
                } else {
                    out.placeholders.push(Placeholder {
                        start: i,
                        end: i + 1,
                        name: None,
                    });
                    i += 1;
                }
            }
            b':' => {
                if sql.get(i + 1) == Some(&b':') {
                    // A `::` cast.
                    i += 2;
                    while i < n && sql[i] == b':' {
                        i += 1;
                    }
                    continue;
                }
                let mut j = i + 1;
                while j < n && is_name_byte(sql[j]) {
                    j += 1;
                }
                if j > i + 1 {
                    out.placeholders.push(Placeholder {
                        start: i,
                        end: j,
                        name: Some(sql[i..j].to_vec()),
                    });
                }
                i = j.max(i + 1);
            }
            _ => i += 1,
        }
    }
    out
}

/// Why a query can't be rewritten.
#[derive(Debug, PartialEq, Eq)]
pub enum RewriteError {
    /// "mixed named and positional parameters".
    Mixed,
}

/// The query with its placeholders replaced: `subst(i, p)` gives the
/// text of the i-th placeholder (a driver placeholder or a quoted
/// value); `??` escapes become `?`.
pub fn rewrite(
    sql: &[u8],
    d: &Dialect,
    mut subst: impl FnMut(usize, &Placeholder) -> Vec<u8>,
) -> Result<Vec<u8>, RewriteError> {
    let s = scan(sql, d);
    let named = s.placeholders.iter().any(|p| p.name.is_some());
    let positional = s.placeholders.iter().any(|p| p.name.is_none());
    if named && positional {
        return Err(RewriteError::Mixed);
    }
    let mut out = Vec::with_capacity(sql.len());
    let mut last = 0;
    let mut esc = s.escapes.iter().peekable();
    for (i, p) in s.placeholders.iter().enumerate() {
        while let Some(&&e) = esc.peek() {
            if e >= p.start {
                break;
            }
            out.extend_from_slice(&sql[last..e]);
            out.push(b'?');
            last = e + 2;
            esc.next();
        }
        out.extend_from_slice(&sql[last..p.start]);
        out.extend(subst(i, p));
        last = p.end;
    }
    for &e in esc {
        out.extend_from_slice(&sql[last..e]);
        out.push(b'?');
        last = e + 2;
    }
    out.extend_from_slice(&sql[last..]);
    Ok(out)
}
