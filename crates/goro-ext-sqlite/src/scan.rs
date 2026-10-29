//! Lexical scanning of SQL text: where the first statement ends (the
//! "tail" of `sqlite3_prepare`), and the statement's parameters.
//!
//! Token rules from sqlite.org (lang_keywords, lang_expr "Literal Values",
//! "Parameters", lang_comment): strings in `'…'` (doubled quote escapes),
//! identifiers in `"…"`, `[…]` or `` `…` ``, `--` and `/* */` comments.
//! Parameters: `?`, `?NNN`, `:AAAA`, `@AAAA`, `$AAAA` (the latter may hold
//! `::` separators and a `(…)` suffix). Numbering (lang_expr): a `?NNN`
//! takes number NNN; a bare `?` takes one more than the largest number so
//! far; a named parameter takes the next number the first time its name
//! appears and reuses it after.

/// One statement parameter, by its SQLite number (index + 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    /// The name with its sigil (`:id`), or `None` for `?`/`?NNN`.
    pub name: Option<String>,
    /// For an unnamed slot: the number graphitesql binds it from (its
    /// `Params::positional` index + 1). graphitesql numbers `?` from the
    /// largest `?`/`?NNN` seen, not counting named parameters.
    pub engine_pos: usize,
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Skips a quoted run starting at `i` (the opening quote); returns the
/// index after the closing quote (or the end).
fn skip_quoted(s: &[u8], i: usize, close: u8) -> usize {
    let mut j = i + 1;
    while j < s.len() {
        if s[j] == close {
            if close != b']' && j + 1 < s.len() && s[j + 1] == close {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    s.len()
}

/// Skips whitespace and comments from `i`.
fn skip_space(s: &[u8], mut i: usize) -> usize {
    loop {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        if s[i..].starts_with(b"--") {
            while i < s.len() && s[i] != b'\n' {
                i += 1;
            }
        } else if s[i..].starts_with(b"/*") {
            match find(&s[i + 2..], b"*/") {
                Some(p) => i += p + 4,
                None => i = s.len(),
            }
        } else {
            return i;
        }
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// A lexical token: what the splitter and the classifier need.
#[derive(Debug, PartialEq, Eq)]
enum Tok<'a> {
    Word(&'a [u8]),
    Semi,
    Other,
}

/// Iterates the tokens of `s` from `i`: (token, start, end).
fn next_tok(s: &[u8], i: usize) -> Option<(Tok<'_>, usize, usize)> {
    let i = skip_space(s, i);
    if i >= s.len() {
        return None;
    }
    let b = s[i];
    let end = match b {
        b'\'' | b'"' | b'`' => skip_quoted(s, i, b),
        b'[' => skip_quoted(s, i, b']'),
        b';' => return Some((Tok::Semi, i, i + 1)),
        _ if is_ident_byte(b) => {
            let mut j = i;
            while j < s.len() && (is_ident_byte(s[j]) || s[j] == b'$') {
                j += 1;
            }
            if b.is_ascii_digit() {
                return Some((Tok::Other, i, j));
            }
            return Some((Tok::Word(&s[i..j]), i, j));
        }
        _ => i + 1,
    };
    Some((Tok::Other, i, end))
}

/// The first statement of `sql`: `(start, end, tail)` where
/// `sql[start..end]` is the statement text (without its `;`) and `tail`
/// is where the rest begins. `start == end` when there is no statement
/// (only spaces, comments and semicolons). A `CREATE TRIGGER` body's
/// semicolons don't end the statement (they end at `END;`).
pub fn first_statement(sql: &str) -> (usize, usize, usize) {
    let s = sql.as_bytes();
    let mut i = 0;
    // Leading empty statements.
    let start = loop {
        match next_tok(s, i) {
            None => return (s.len(), s.len(), s.len()),
            Some((Tok::Semi, _, e)) => i = e,
            Some((_, st, _)) => break st,
        }
    };
    let mut words = 0usize;
    let mut is_trigger = false;
    let mut create = false;
    let mut last_word_end = false;
    let mut end = start;
    i = start;
    while let Some((t, _, e)) = next_tok(s, i) {
        match t {
            Tok::Semi => {
                if !is_trigger || last_word_end {
                    // Skip the run of semicolons/space that follows.
                    return (start, end, e);
                }
                last_word_end = false;
            }
            Tok::Word(w) => {
                if words == 0 && w.eq_ignore_ascii_case(b"create") {
                    create = true;
                } else if create && words <= 3 && w.eq_ignore_ascii_case(b"trigger") {
                    is_trigger = true;
                }
                last_word_end = w.eq_ignore_ascii_case(b"end");
                words += 1;
            }
            Tok::Other => {
                last_word_end = false;
                words += 1;
            }
        }
        end = e;
        i = e;
    }
    (start, end, s.len())
}

/// Splits a script into its statements (texts without the `;`).
pub fn split_statements(sql: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = sql;
    loop {
        let (st, en, tail) = first_statement(rest);
        if st == en {
            return out;
        }
        out.push(&rest[st..en]);
        rest = &rest[tail..];
    }
}

/// The parameters of one statement, by SQLite number.
pub fn scan_params(sql: &str) -> Vec<Param> {
    let s = sql.as_bytes();
    let mut params: Vec<Param> = Vec::new();
    let mut engine_max = 0usize;
    let mut i = 0;
    let set = |params: &mut Vec<Param>, n: usize, p: Param| {
        while params.len() < n {
            params.push(Param {
                name: None,
                engine_pos: params.len() + 1,
            });
        }
        params[n - 1] = p;
    };
    while i < s.len() {
        let b = s[i];
        match b {
            b'\'' | b'"' | b'`' => i = skip_quoted(s, i, b),
            b'[' => i = skip_quoted(s, i, b']'),
            b'-' if s[i..].starts_with(b"--") => i = skip_space(s, i),
            b'/' if s[i..].starts_with(b"/*") => i = skip_space(s, i),
            b'?' => {
                let mut j = i + 1;
                while j < s.len() && s[j].is_ascii_digit() {
                    j += 1;
                }
                let n = if j > i + 1 {
                    let n: usize = std::str::from_utf8(&s[i + 1..j])
                        .ok()
                        .and_then(|t| t.parse().ok())
                        .unwrap_or(0);
                    engine_max = engine_max.max(n);
                    n
                } else {
                    engine_max += 1;
                    params.len() + 1
                };
                if n > 0 {
                    let pos = if j > i + 1 { n } else { engine_max };
                    if n > params.len() || params[n - 1].name.is_none() {
                        set(
                            &mut params,
                            n,
                            Param {
                                name: None,
                                engine_pos: pos,
                            },
                        );
                    }
                }
                i = j;
            }
            b':' | b'@' | b'$' => {
                let mut j = i + 1;
                loop {
                    while j < s.len() && is_ident_byte(s[j]) {
                        j += 1;
                    }
                    if b == b'$' && s[j..].starts_with(b"::") {
                        j += 2;
                        continue;
                    }
                    break;
                }
                if b == b'$' && j < s.len() && s[j] == b'(' {
                    while j < s.len() && s[j] != b')' {
                        j += 1;
                    }
                    j = (j + 1).min(s.len());
                }
                if j > i + 1 {
                    let name = String::from_utf8_lossy(&s[i..j]).into_owned();
                    if !params.iter().any(|p| p.name.as_deref() == Some(&name)) {
                        params.push(Param {
                            name: Some(name),
                            engine_pos: 0,
                        });
                    }
                }
                i = j.max(i + 1);
            }
            _ if is_ident_byte(b) => {
                while i < s.len() && (is_ident_byte(s[i]) || s[i] == b'$') {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    params
}

/// The leading keyword of a statement (upper case), skipping comments.
pub fn first_keyword(sql: &str) -> String {
    match next_tok(sql.as_bytes(), 0) {
        Some((Tok::Word(w), _, _)) => String::from_utf8_lossy(w).to_ascii_uppercase(),
        _ => String::new(),
    }
}

/// Whether the statement text has a `RETURNING` keyword outside strings
/// and identifiers.
pub fn has_keyword(sql: &str, kw: &str) -> bool {
    let s = sql.as_bytes();
    let mut i = 0;
    while let Some((t, _, e)) = next_tok(s, i) {
        if let Tok::Word(w) = t
            && w.eq_ignore_ascii_case(kw.as_bytes())
        {
            return true;
        }
        i = e;
    }
    false
}

/// `'…'` quoting with doubled single quotes (SQLite's `%q`).
pub fn escape_string(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &b in s {
        if b == b'\'' {
            out.push(b'\'');
        }
        out.push(b);
    }
    out
}

/// An identifier in double quotes, `"` doubled.
pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first() {
        assert_eq!(first_statement("SELECT 1; SELECT 2"), (0, 8, 9));
        assert_eq!(first_statement(" ;; "), (4, 4, 4));
        let sql = "CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1; SELECT 2; END; SELECT 3";
        let (s, e, _) = first_statement(sql);
        assert_eq!(
            &sql[s..e],
            "CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1; SELECT 2; END"
        );
        assert_eq!(
            split_statements("a;'b;c';--x;\n d"),
            vec!["a", "'b;c'", "d"]
        );
    }

    #[test]
    fn params() {
        let p = scan_params("select :a, :b, ?, ':c', ?5, ?, :a");
        assert_eq!(p.len(), 6);
        assert_eq!(p[0].name.as_deref(), Some(":a"));
        assert_eq!(p[1].name.as_deref(), Some(":b"));
        assert_eq!(
            p[2],
            Param {
                name: None,
                engine_pos: 1
            }
        );
        assert_eq!(
            p[4],
            Param {
                name: None,
                engine_pos: 5
            }
        );
        assert_eq!(
            p[5],
            Param {
                name: None,
                engine_pos: 6
            }
        );
        assert!(scan_params("select '?' -- ?\n").is_empty());
    }
}
