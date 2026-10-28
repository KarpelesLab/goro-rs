//! OpenSSL configuration files (OpenSSL documentation: config(5)):
//! `[section]` headers, `name = value` pairs, `#` comments, `\`
//! continuation lines, quoting, backslash escapes and `$var` /
//! `${section::var}` expansion.

#[derive(Debug, Clone, Default)]
pub struct Conf {
    /// (section, entries in order).
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

#[derive(Debug, Clone)]
pub struct ConfError {
    pub line: usize,
}

impl Conf {
    pub fn parse(text: &[u8]) -> Result<Conf, ConfError> {
        let text = String::from_utf8_lossy(text);
        let mut conf = Conf {
            sections: vec![("default".into(), Vec::new())],
        };
        let mut cur = 0usize;
        let mut pending = String::new();
        let mut start_line = 0;
        for (lineno, raw) in text.split('\n').enumerate() {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            if pending.is_empty() {
                start_line = lineno + 1;
            }
            // Continuation: an odd number of trailing backslashes.
            let trailing = raw.chars().rev().take_while(|&c| c == '\\').count();
            if trailing % 2 == 1 {
                pending.push_str(&raw[..raw.len() - 1]);
                continue;
            }
            pending.push_str(raw);
            let line = std::mem::take(&mut pending);
            let line = strip_comment(&line);
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(rest) = t.strip_prefix('[') {
                let Some(end) = rest.find(']') else {
                    return Err(ConfError { line: start_line });
                };
                let name = rest[..end].trim().to_string();
                cur = match conf.sections.iter().position(|(n, _)| *n == name) {
                    Some(i) => i,
                    None => {
                        conf.sections.push((name, Vec::new()));
                        conf.sections.len() - 1
                    }
                };
                continue;
            }
            let Some(eq) = t.find('=') else {
                return Err(ConfError { line: start_line });
            };
            let name = t[..eq].trim().to_string();
            let (sec, name) = match name.split_once("::") {
                Some((s, n)) => (Some(s.to_string()), n.to_string()),
                None => (None, name),
            };
            let value = conf.unescape(t[eq + 1..].trim(), &conf.sections[cur].0);
            let idx = match sec {
                Some(s) => match conf.sections.iter().position(|(n, _)| *n == s) {
                    Some(i) => i,
                    None => {
                        conf.sections.push((s, Vec::new()));
                        conf.sections.len() - 1
                    }
                },
                None => cur,
            };
            let entries = &mut conf.sections[idx].1;
            if let Some(e) = entries.iter_mut().find(|(n, _)| *n == name) {
                e.1 = value;
            } else {
                entries.push((name, value));
            }
        }
        Ok(conf)
    }

    fn unescape(&self, v: &str, section: &str) -> String {
        let mut out = String::new();
        let mut it = v.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '"' | '\'' => {
                    for d in it.by_ref() {
                        if d == c {
                            break;
                        }
                        out.push(d);
                    }
                }
                '\\' => match it.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('b') => out.push('\u{8}'),
                    Some(o) => out.push(o),
                    None => {}
                },
                '$' => {
                    let mut name = String::new();
                    if it.peek() == Some(&'{') || it.peek() == Some(&'(') {
                        let close = if it.next() == Some('{') { '}' } else { ')' };
                        for d in it.by_ref() {
                            if d == close {
                                break;
                            }
                            name.push(d);
                        }
                    } else {
                        while let Some(&d) = it.peek() {
                            if d.is_ascii_alphanumeric() || d == '_' || d == ':' {
                                name.push(d);
                                it.next();
                            } else {
                                break;
                            }
                        }
                    }
                    let (s, n) = match name.split_once("::") {
                        Some((s, n)) => (s.to_string(), n.to_string()),
                        None => (section.to_string(), name.clone()),
                    };
                    if let Some(val) = self.get(&s, &n).or_else(|| self.get("default", &n)) {
                        out.push_str(val);
                    } else if s == "ENV"
                        && let Ok(env) = std::env::var(&n)
                    {
                        out.push_str(&env);
                    }
                }
                _ => out.push(c),
            }
        }
        // Unquoted trailing whitespace is not significant.
        out
    }

    pub fn get(&self, section: &str, name: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n == section)
            .and_then(|(_, e)| e.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()))
    }

    pub fn section(&self, name: &str) -> Option<&[(String, String)]> {
        self.sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| e.as_slice())
    }

    /// A value from `section`, falling back to the default section.
    pub fn lookup(&self, section: &str, name: &str) -> Option<&str> {
        self.get(section, name)
            .or_else(|| self.get("default", name))
    }
}

fn strip_comment(l: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in l.chars() {
        if esc {
            out.push(c);
            esc = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                esc = true;
            }
            '"' | '\'' => {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
                out.push(c);
            }
            '#' if quote.is_none() => break,
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        let c = Conf::parse(
            b"a = 1\n[ req ]\ndefault_bits\t\t= 1024 # comment\nx = $a/b\n[s2]\ny = ${req::default_bits}\nz = \"q # x\"\n",
        )
        .unwrap();
        assert_eq!(c.get("req", "default_bits"), Some("1024"));
        assert_eq!(c.get("req", "x"), Some("1/b"));
        assert_eq!(c.get("s2", "y"), Some("1024"));
        assert_eq!(c.get("s2", "z"), Some("q # x"));
    }
}
