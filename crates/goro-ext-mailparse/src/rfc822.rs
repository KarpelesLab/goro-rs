//! php_mailparse_rfc822.c: the RFC 822 header tokenizer (the re2c scanner
//! of php_mailparse_rfc822.re, as generated), token recombination and the
//! address list parser.

/// Token kinds: 0 is an atom/word, otherwise the special character itself
/// (`"` quoted string, `(` comment, `<`, `>`, `:`, `;`, `,`, `@`...).
#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub token: u8,
    /// Offset of the value in the tokenized buffer; None for the empty
    /// word between `<` and `>` of a "<>" address.
    pub value: Option<usize>,
    pub valuelen: usize,
}

pub struct Tokenized {
    /// The header, NUL-truncated as the C code sees it.
    pub buffer: Vec<u8>,
    pub tokens: Vec<Token>,
}

impl Tokenized {
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn tok(&self, i: usize) -> u8 {
        self.tokens[i].token
    }

    fn value(&self, t: &Token) -> &[u8] {
        match t.value {
            Some(o) => {
                let end = (o + t.valuelen).min(self.buffer.len());
                &self.buffer[o.min(end)..end]
            }
            None => b"",
        }
    }

    /// The buffer from the token's value on (what `printf("%s", value)`
    /// prints).
    pub fn value_tail(&self, i: usize) -> &[u8] {
        match self.tokens[i].value {
            Some(o) => &self.buffer[o.min(self.buffer.len())..],
            None => b"",
        }
    }
}

pub const RECOMBINE_IGNORE_COMMENTS: u32 = 1;
pub const RECOMBINE_STRTOLOWER: u32 = 2;
pub const RECOMBINE_COMMENTS_TO_QUOTES: u32 = 4;
pub const RECOMBINE_SPACE_ATOMS: u32 = 8;
pub const RECOMBINE_INCLUDE_QUOTES: u32 = 16;
pub const RECOMBINE_COMMENTS_ONLY: u32 = 32;

/// The generated scanner's character classes (yybm): bit 128 = may appear
/// in a quoted string, 64 = white space, 32 = "other" (a word character).
fn class(c: u8) -> u8 {
    match c {
        0 => 0,
        b'\t' | b'\n' | b'\r' | b' ' => 192,
        b'"' => 0,
        b'!' | b'%' | b'(' | b')' | b',' | b'.' | b'/' | b':' | b';' | b'<' | b'=' | b'>'
        | b'?' | b'@' | b'[' | b']' => 128,
        _ => 160,
    }
}

fn is_atom_token(t: u8) -> bool {
    t == 0 || t == b'"' || t == b'('
}

/// php_mailparse_rfc822_tokenize(): the warnings it raises are returned
/// (the C code scans twice, counting then filling, so each one is raised
/// twice).
pub fn tokenize(header: &[u8], report_errors: bool) -> (Tokenized, Vec<&'static str>) {
    let end = header.iter().position(|&b| b == 0).unwrap_or(header.len());
    let buffer = header[..end].to_vec();
    let mut tokens = Vec::new();
    let mut errs: Vec<&'static str> = Vec::new();
    scan(&buffer, &mut tokens, &mut errs);
    let mut warnings = Vec::new();
    if report_errors {
        warnings.extend_from_slice(&errs);
        warnings.extend_from_slice(&errs);
    }
    (Tokenized { buffer, tokens }, warnings)
}

fn scan(buf: &[u8], tokens: &mut Vec<Token>, errs: &mut Vec<&'static str>) {
    // The buffer has two NUL terminators.
    let at = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    let atom = |tokens: &mut Vec<Token>, start: usize| {
        tokens.push(Token {
            token: at(start),
            value: Some(start),
            valuelen: 1,
        })
    };
    let mut in_bracket = false;
    let mut p = 0usize;
    loop {
        let start = p;
        let c = at(p);
        if class(c) & 64 != 0 {
            while class(at(p)) & 64 != 0 {
                p += 1;
            }
            continue;
        }
        match c {
            0 => return,
            b'!' | b'%' | b',' | b'.' | b'/' | b':' | b';' | b'=' | b'?' | b'@' | b'[' | b']' => {
                p += 1;
                atom(tokens, start);
            }
            b'"' => {
                p += 1;
                while class(at(p)) & 128 != 0 {
                    p += 1;
                }
                if at(p) != 0 {
                    // closing quote
                    p += 1;
                    tokens.push(Token {
                        token: b'"',
                        value: Some(start + 1),
                        valuelen: p - start - 2,
                    });
                } else {
                    // unterminated: the scanner falls into the comment
                    // rule, past the first NUL
                    p += 1;
                    if comment(buf, start, p, tokens, errs) {
                        return;
                    }
                }
            }
            b'(' => {
                p += 1;
                if comment(buf, start, p, tokens, errs) {
                    return;
                }
                p = tokens.last().map_or(p, |t| start + t.valuelen);
            }
            b')' => {
                p += 1;
                errs.push("token not valid in ground state");
            }
            b'\\' => {
                p += 1;
                if class(at(p)) & 32 != 0 {
                    while class(at(p)) & 32 != 0 {
                        p += 1;
                    }
                    tokens.push(Token {
                        token: 0,
                        value: Some(start),
                        valuelen: p - start,
                    });
                } else {
                    errs.push("token not valid in ground state");
                }
            }
            b'<' => {
                p += 1;
                if at(p) == b'>' {
                    p += 1;
                    atom(tokens, start);
                    tokens.push(Token {
                        token: 0,
                        value: None,
                        valuelen: 0,
                    });
                    atom(tokens, start + 1);
                } else if in_bracket {
                    errs.push("already in < bracket");
                } else {
                    in_bracket = true;
                    atom(tokens, start);
                }
            }
            b'>' => {
                p += 1;
                if !in_bracket {
                    errs.push("not in < bracket");
                } else {
                    in_bracket = false;
                    atom(tokens, start);
                }
            }
            _ => {
                p += 1;
                while class(at(p)) & 32 != 0 {
                    p += 1;
                }
                tokens.push(Token {
                    token: 0,
                    value: Some(start),
                    valuelen: p - start,
                });
            }
        }
    }
}

/// state_comment, the token starting at `start` and the scan at `p`. Pushes
/// the comment token (whose length gives where scanning resumes); true when
/// the end of the header was reached (scanning stops).
fn comment(
    buf: &[u8],
    start: usize,
    mut p: usize,
    tokens: &mut Vec<Token>,
    errs: &mut Vec<&'static str>,
) -> bool {
    let at = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    let mut depth = 1;
    let mut valuelen = 0;
    loop {
        let c = at(p);
        if c == 0 {
            errs.push("unexpected end of header");
            // fake a quoted string for this last token
            tokens.push(Token {
                token: b'"',
                value: Some(start),
                valuelen,
            });
            return true;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                p += 1;
                break;
            }
        } else if c == b'\\' && at(p + 1) != 0 {
            p += 1;
            valuelen += 1;
        }
        p += 1;
    }
    tokens.push(Token {
        token: b'(',
        value: Some(start),
        valuelen: p - start,
    });
    false
}

/// php_rfc822_recombine_tokens()
pub fn recombine(toks: &Tokenized, first: usize, n: usize, flags: u32) -> Vec<u8> {
    let upper = (first + n).min(toks.len());
    let mut ret = Vec::new();
    let mut last_was_atom = false;
    for i in first..upper {
        let t = &toks.tokens[i];
        let mut tok_equiv = t.token;
        if tok_equiv == b'(' && flags & RECOMBINE_COMMENTS_TO_QUOTES != 0 {
            tok_equiv = b'"';
        }
        if flags & RECOMBINE_IGNORE_COMMENTS != 0 && tok_equiv == b'(' {
            continue;
        }
        if flags & RECOMBINE_COMMENTS_ONLY != 0
            && tok_equiv != b'('
            && !(t.token == b'(' && flags & RECOMBINE_COMMENTS_TO_QUOTES != 0)
        {
            continue;
        }
        let this_is_atom = is_atom_token(t.token);
        if this_is_atom && last_was_atom && flags & RECOMBINE_SPACE_ATOMS != 0 {
            ret.push(b' ');
        }
        if flags & RECOMBINE_INCLUDE_QUOTES != 0 && tok_equiv == b'"' {
            ret.push(b'"');
        }
        let mut v = toks.value(t);
        if t.token == b'(' && flags & RECOMBINE_COMMENTS_TO_QUOTES != 0 && v.len() >= 2 {
            // without the parentheses
            v = &v[1..v.len() - 1];
        }
        ret.extend_from_slice(v);
        if flags & RECOMBINE_INCLUDE_QUOTES != 0 && tok_equiv == b'"' {
            ret.push(b'"');
        }
        last_was_atom = this_is_atom;
    }
    if flags & RECOMBINE_STRTOLOWER != 0 {
        ret.make_ascii_lowercase();
    }
    ret
}

#[derive(Clone, Debug, Default)]
pub struct Address {
    pub name: Option<Vec<u8>>,
    pub address: Option<Vec<u8>>,
    pub is_group: bool,
}

/// php_rfc822_parse_address_tokens()
pub fn parse_addresses(toks: &Tokenized) -> Vec<Address> {
    let n = toks.len();
    let tok = |i: usize| toks.tok(i);
    let mut addrs: Vec<Address> = Vec::new();
    let slot = |addrs: &mut Vec<Address>, i: usize| -> usize {
        if addrs.len() <= i {
            addrs.resize(i + 1, Address::default());
        }
        i
    };
    let mut start_tok = 0usize;
    let mut iaddr = 0usize;
    let mut in_group = false;
    let mut group_lbl_start = 0usize;
    let mut group_lbl_end = 0usize;
    let mut group_addrs: Vec<u8> = Vec::new();

    // address: mailbox / group
    'address: loop {
        if start_tok >= n {
            break 'address;
        }
        // look ahead to determine if we are dealing with a group
        let mut i = start_tok;
        while i < n && (tok(i) == 0 || tok(i) == b'"') {
            i += 1;
        }
        if i < n && tok(i) == b':' {
            in_group = true;
            group_lbl_start = start_tok;
            group_lbl_end = i;
            // the group's address includes the leading ":"
            start_tok = i;
        }

        // mailbox: addr-spec / phrase route-addr
        loop {
            if start_tok >= n {
                break 'address;
            }
            // skip spurious commas
            while start_tok < n && (tok(start_tok) == b',' || tok(start_tok) == b';') {
                start_tok += 1;
            }
            // a '<' before an '@': a route-addr, otherwise an addr-spec
            let mut i = start_tok;
            while i < n && tok(i) != b';' && tok(i) != b',' && tok(i) != b'<' {
                i += 1;
            }
            // start_tok..i is the display name part
            if !in_group && i > start_tok {
                let k = slot(&mut addrs, iaddr);
                match if i < n { tok(i) } else { 0 } {
                    b';' | b',' | b'<' => {
                        addrs[k].name = Some(recombine(
                            toks,
                            start_tok,
                            i - start_tok,
                            RECOMBINE_SPACE_ATOMS,
                        ));
                    }
                    _ => {
                        // only a display name with quoted strings or comments
                        let has_comments = (start_tok..i).any(|j| tok(j) == b'(');
                        let has_strings = (start_tok..i).any(|j| tok(j) == b'"');
                        if has_comments && !has_strings {
                            addrs[k].name = Some(recombine(
                                toks,
                                start_tok,
                                i - start_tok,
                                RECOMBINE_SPACE_ATOMS
                                    | RECOMBINE_COMMENTS_ONLY
                                    | RECOMBINE_COMMENTS_TO_QUOTES,
                            ));
                        } else if has_strings {
                            addrs[k].name = Some(recombine(
                                toks,
                                start_tok,
                                i - start_tok,
                                RECOMBINE_SPACE_ATOMS,
                            ));
                        }
                    }
                }
            }

            let addr_flags =
                RECOMBINE_SPACE_ATOMS | RECOMBINE_IGNORE_COMMENTS | RECOMBINE_INCLUDE_QUOTES;
            let address_value;
            if i < n && tok(i) == b'<' {
                // route-addr = "<" [route] addr-spec ">"
                let mut j = i;
                while j < n && tok(j) != b'>' {
                    j += 1;
                }
                // the address leaves the <> out
                let a_start = i + 1;
                let a_count = (j - i) as isize - 1;
                address_value = recombine(toks, a_start, a_count.max(0) as usize, addr_flags);
                start_tok = j + 1;
            } else {
                // addr-spec = local-part "@" domain
                let mut a_start = start_tok;
                let mut a_count = i - start_tok;
                if a_count > 0 && tok(a_start) == b'<' {
                    a_start += 1;
                    a_count -= 1;
                }
                address_value = recombine(toks, a_start, a_count, addr_flags);
                start_tok = i;
            }

            {
                let k = slot(&mut addrs, iaddr);
                // without a display name, the address is used
                if addrs[k].name.is_none() {
                    addrs[k].name = Some(address_value.clone());
                }
                if in_group {
                    if !group_addrs.is_empty() {
                        group_addrs.push(b',');
                    }
                    group_addrs.extend_from_slice(&address_value);
                } else {
                    addrs[k].address = Some(address_value);
                }
            }

            if !in_group {
                iaddr += 1;
                continue 'address;
            }
            // a ";" ends the group
            if (start_tok < n && tok(start_tok) == b';') || start_tok == n {
                let k = slot(&mut addrs, iaddr);
                group_addrs.push(b';');
                addrs[k].address = Some(std::mem::take(&mut group_addrs));
                addrs[k].name = Some(recombine(
                    toks,
                    group_lbl_start,
                    group_lbl_end - group_lbl_start,
                    RECOMBINE_SPACE_ATOMS,
                ));
                addrs[k].is_group = true;
                iaddr += 1;
                in_group = false;
                start_tok += 1;
                continue 'address;
            }
            // more mailboxes in this group
        }
    }
    addrs.resize(iaddr, Address::default());
    addrs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(char, String)> {
        let (t, _) = tokenize(s.as_bytes(), false);
        (0..t.len())
            .map(|i| {
                (
                    if t.tok(i) == 0 { 'w' } else { t.tok(i) as char },
                    String::from_utf8_lossy(&recombine(&t, i, 1, 0)).into_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn tokens() {
        assert_eq!(
            kinds("a (b (c)) \"q\" <x@y> <>"),
            vec![
                ('w', "a".into()),
                ('(', "(b (c))".into()),
                ('"', "q".into()),
                ('<', "<".into()),
                ('w', "x".into()),
                ('@', "@".into()),
                ('w', "y".into()),
                ('>', ">".into()),
                ('<', "<".into()),
                ('w', "".into()),
                ('>', ">".into()),
            ]
        );
        assert_eq!(kinds("\"abc"), vec![('"', "".into())]);
        assert_eq!(
            kinds("a\\b \\("),
            // a lone backslash is skipped, the open comment ends the header
            vec![('w', "a\\b".into()), ('"', "".into())]
        );
    }

    #[test]
    fn addresses() {
        let (t, _) = tokenize(
            b"\":sysmail\"@ Some-Group. Some-Org, Muhammed.(I am the greatest) Ali @(the)Vegas.WBA",
            false,
        );
        let a = parse_addresses(&t);
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[0].name.as_deref(),
            Some(&b":sysmail@Some-Group.Some-Org"[..])
        );
        assert_eq!(
            a[0].address.as_deref(),
            Some(&b"\":sysmail\"@Some-Group.Some-Org"[..])
        );
        assert_eq!(a[1].name.as_deref(), Some(&b"I am the greatest the"[..]));
        assert_eq!(
            a[1].address.as_deref(),
            Some(&b"Muhammed.Ali@Vegas.WBA"[..])
        );
    }
}
