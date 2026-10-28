//! strip_tags(): a small state machine matching the behaviour seen in the
//! tests ext/standard/tests/strings/strip_tags*.phpt (and the bug*.phpt
//! tests using strip_tags) and observed on PHP 8.5 (fuzzed cases in the
//! fixture tests/fixtures/html.txt).
//!
//! Observed rules: a `<` followed by whitespace is text; inside a tag, `<`
//! opens a nested level that the next `>` closes (both left out of a kept
//! tag), and that count carries over into the text after a comment or PHP
//! block (a `>` there closes a level and is dropped); `<!` and `<?` start a
//! declaration or a PHP block even inside a tag; `<!--` starts a comment
//! ended by `-->`; quotes hide `<` and `>` in tags, declarations and PHP
//! blocks (a backslash escapes them in the last two); NUL bytes are dropped.

/// The `$allowed_tags` argument.
#[derive(Clone, Copy, Debug)]
pub enum AllowedTags<'a> {
    /// null or omitted.
    None,
    /// String form, e.g. `"<a><b>"`.
    Str(&'a [u8]),
    /// Array form: the elements converted to strings, e.g. `["a", "b"]`.
    List(&'a [&'a [u8]]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    Tag,
    Php,
    Bang,
    Comment,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

fn allowed_string(allowed: AllowedTags) -> Option<Vec<u8>> {
    match allowed {
        AllowedTags::None => None,
        AllowedTags::Str(s) => Some(s.to_ascii_lowercase()),
        AllowedTags::List(l) => {
            let mut v = Vec::new();
            for t in l {
                v.push(b'<');
                v.extend_from_slice(&t.to_ascii_lowercase());
                v.push(b'>');
            }
            Some(v)
        }
    }
}

/// Whether the tag in `buf` (starting with `<`, without the final `>`) is
/// in the allowed list: its name, lowercased, without a leading `/` or a
/// trailing `/`, up to whitespace.
fn tag_allowed(buf: &[u8], allowed: &[u8]) -> bool {
    let mut norm = vec![b'<'];
    let mut i = 1;
    if buf.get(i) == Some(&b'/') {
        i += 1;
    }
    while i < buf.len() {
        let c = buf[i];
        if is_space(c) || (c == b'/' && i + 1 == buf.len()) {
            break;
        }
        norm.push(c.to_ascii_lowercase());
        i += 1;
    }
    norm.push(b'>');
    allowed.windows(norm.len()).any(|w| w == &norm[..])
}

/// Whether the `?` at `s[q]` starts `?xml:`, an XML namespace tag.
fn xml_ns(s: &[u8], q: usize) -> bool {
    s[q + 1..]
        .get(..4)
        .is_some_and(|w| w.eq_ignore_ascii_case(b"xml:"))
}

pub fn strip_tags(s: &[u8], allowed: AllowedTags) -> Vec<u8> {
    let allowed = allowed_string(allowed);
    let mut out = Vec::with_capacity(s.len());
    let mut state = State::Text;
    let mut buf: Vec<u8> = Vec::new();
    let mut depth = 0usize;
    let mut quote: u8 = 0;
    // In a "<?xml:" block, "/>" ends it too (test bug45485.phpt), and so
    // does any ">" when it started inside a tag (observed on PHP 8.5).
    let mut xml = false;
    let mut xml_in_tag = false;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let prev = if i > 0 { s[i - 1] } else { 0 };
        let next = s.get(i + 1).copied();
        match state {
            State::Text => match c {
                0 => {}
                b'<' => {
                    if next.is_some_and(is_space) {
                        out.push(c);
                    } else if next == Some(b'!') {
                        state = State::Bang;
                        quote = 0;
                        i += 1;
                    } else if next == Some(b'?') {
                        state = State::Php;
                        xml = xml_ns(s, i + 1);
                        xml_in_tag = false;
                        quote = 0;
                        i += 1;
                    } else {
                        state = State::Tag;
                        buf.clear();
                        buf.push(c);
                        quote = 0;
                    }
                }
                b'>' if depth > 0 => depth -= 1,
                _ => out.push(c),
            },
            State::Tag => {
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                    buf.push(c);
                } else {
                    match c {
                        b'"' | b'\'' => {
                            quote = c;
                            buf.push(c);
                        }
                        // A nested "<" (not followed by whitespace) and the
                        // ">" closing it are left out of the tag's text
                        // (observed on PHP 8.5: "<a <b> c>" keeps "<a b c>").
                        b'<' => {
                            if next.is_some_and(is_space) {
                                buf.push(c);
                            } else {
                                depth += 1;
                            }
                        }
                        b'!' if prev == b'<' => {
                            state = State::Bang;
                        }
                        b'?' if prev == b'<' => {
                            state = State::Php;
                            xml = xml_ns(s, i);
                            xml_in_tag = true;
                        }
                        b'>' => {
                            if depth > 0 {
                                depth -= 1;
                            } else {
                                if let Some(al) = &allowed
                                    && tag_allowed(&buf, al)
                                {
                                    out.extend_from_slice(&buf);
                                    out.push(b'>');
                                }
                                state = State::Text;
                            }
                        }
                        _ => buf.push(c),
                    }
                }
            }
            State::Php => {
                let escaped = prev == b'\\';
                if quote != 0 {
                    if c == quote && !escaped {
                        quote = 0;
                    }
                } else {
                    match c {
                        b'"' | b'\'' if !escaped => quote = c,
                        b'>' if depth > 0 => depth -= 1,
                        b'>' if prev == b'?' || (xml && (prev == b'/' || xml_in_tag)) => {
                            state = State::Text
                        }
                        _ => {}
                    }
                }
            }
            State::Bang => {
                if quote != 0 {
                    if c == quote && prev != b'\\' {
                        quote = 0;
                    }
                } else if c == b'-' && next == Some(b'-') && prev == b'!' {
                    state = State::Comment;
                    i += 1;
                } else if (c == b'"' || c == b'\'') && prev != b'\\' {
                    quote = c;
                } else if c == b'>' {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        state = State::Text;
                    }
                }
            }
            State::Comment => {
                if c == b'>' && i >= 2 && s[i - 1] == b'-' && s[i - 2] == b'-' {
                    state = State::Text;
                }
            }
        }
        i += 1;
    }
    out
}
