//! OpenSSL configuration file parser (NCONF, `conf_def.c` semantics) and the
//! php-src `struct php_x509_request` option parsing.

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Conf {
    /// section -> ordered (name, value) list
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

impl Conf {
    fn section_mut(&mut self, name: &str) -> &mut Vec<(String, String)> {
        if let Some(i) = self.sections.iter().position(|(n, _)| n == name) {
            return &mut self.sections[i].1;
        }
        self.sections.push((name.to_string(), Vec::new()));
        &mut self.sections.last_mut().unwrap().1
    }

    pub fn section(&self, name: &str) -> Option<&Vec<(String, String)>> {
        self.sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn lookup(&self, section: &str, name: &str) -> Option<String> {
        self.section(section)?
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }

    /// NCONF_get_string(): falls back to the "default" section and, for the
    /// "ENV" section, to the environment.
    pub fn get_string(&self, group: Option<&str>, name: &str) -> Option<String> {
        if let Some(g) = group {
            if let Some(v) = self.lookup(g, name) {
                return Some(v);
            }
            if g == "ENV" {
                if let Ok(v) = std::env::var(name) {
                    return Some(v);
                }
            }
        }
        self.lookup("default", name)
    }

    /// NCONF_get_number() (errors ignored -> 0).
    pub fn get_number(&self, group: Option<&str>, name: &str) -> i64 {
        let Some(s) = self.get_string(group, name) else {
            return 0;
        };
        let mut v: i64 = 0;
        for c in s.bytes() {
            if c.is_ascii_digit() {
                v = v.wrapping_mul(10).wrapping_add((c - b'0') as i64);
            } else {
                break;
            }
        }
        v
    }

    pub fn load_file(path: &str) -> Option<Conf> {
        let data = std::fs::read(path).ok()?;
        let mut c = Conf::default();
        c.sections.push(("default".to_string(), Vec::new()));
        c.parse(&String::from_utf8_lossy(&data), path, 0)?;
        Some(c)
    }

    fn parse(&mut self, text: &str, path: &str, depth: usize) -> Option<()> {
        let mut section = "default".to_string();
        let mut lines = text.split('\n').peekable();
        while let Some(raw) = lines.next() {
            let mut line = raw.trim_end_matches('\r').to_string();
            // continuation lines
            while line.ends_with('\\') && !line.ends_with("\\\\") {
                line.pop();
                match lines.next() {
                    Some(n) => line.push_str(n.trim_end_matches('\r')),
                    None => break,
                }
            }
            let l = strip_comment(&line);
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(rest) = t.strip_prefix('[') {
                let end = rest.find(']')?;
                section = rest[..end].trim().to_string();
                self.section_mut(&section);
                continue;
            }
            if let Some(rest) = t.strip_prefix(".include") {
                let p = rest.trim_start_matches([' ', '\t', '=']).trim();
                if depth < 10 {
                    let p = if p.starts_with('/') {
                        p.to_string()
                    } else {
                        let dir = std::path::Path::new(path)
                            .parent()
                            .map(|d| d.to_string_lossy().to_string())
                            .unwrap_or_default();
                        format!("{}/{}", dir, p)
                    };
                    if let Ok(md) = std::fs::metadata(&p) {
                        if md.is_dir() {
                            if let Ok(rd) = std::fs::read_dir(&p) {
                                let mut files: Vec<_> =
                                    rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                                files.sort();
                                for f in files {
                                    let fs = f.to_string_lossy().to_string();
                                    if fs.ends_with(".cnf") || fs.ends_with(".conf") {
                                        if let Ok(d) = std::fs::read(&f) {
                                            self.parse(
                                                &String::from_utf8_lossy(&d),
                                                &fs,
                                                depth + 1,
                                            );
                                        }
                                    }
                                }
                            }
                        } else if let Ok(d) = std::fs::read(&p) {
                            self.parse(&String::from_utf8_lossy(&d), &p, depth + 1);
                        }
                    }
                }
                continue;
            }
            if t.starts_with(".pragma") {
                continue;
            }
            let eq = t.find('=')?;
            let name_part = t[..eq].trim();
            let value_part = t[eq + 1..].trim();
            // "section::name = value" form
            let (sect, name) = match name_part.find("::") {
                Some(i) => (name_part[..i].to_string(), name_part[i + 2..].to_string()),
                None => (section.clone(), name_part.to_string()),
            };
            let value = self.expand(&sect, value_part);
            self.section_mut(&sect).push((name, value));
        }
        Some(())
    }

    /// Unquote/unescape and expand $var references.
    fn expand(&self, section: &str, v: &str) -> String {
        let chars: Vec<char> = v.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match c {
                '"' | '\'' => {
                    let q = c;
                    i += 1;
                    while i < chars.len() && chars[i] != q {
                        if chars[i] == '\\' && i + 1 < chars.len() {
                            i += 1;
                        }
                        out.push(chars[i]);
                        i += 1;
                    }
                    i += 1;
                }
                '\\' => {
                    if i + 1 < chars.len() {
                        let n = chars[i + 1];
                        out.push(match n {
                            'n' => '\n',
                            'r' => '\r',
                            'b' => '\x08',
                            't' => '\t',
                            other => other,
                        });
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                '$' => {
                    let mut j = i + 1;
                    let braced = j < chars.len() && (chars[j] == '{' || chars[j] == '(');
                    let close = if braced {
                        if chars[j] == '{' { '}' } else { ')' }
                    } else {
                        ' '
                    };
                    if braced {
                        j += 1;
                    }
                    let start = j;
                    while j < chars.len()
                        && (chars[j].is_ascii_alphanumeric()
                            || chars[j] == '_'
                            || chars[j] == '.'
                            || chars[j] == ':')
                    {
                        j += 1;
                    }
                    let mut name: String = chars[start..j].iter().collect();
                    if braced {
                        if j < chars.len() && chars[j] == close {
                            j += 1;
                        }
                    } else {
                        // "::" may only appear between section and name
                        while name.ends_with(':') {
                            name.pop();
                            j -= 1;
                        }
                    }
                    let (sect, var) = match name.find("::") {
                        Some(k) => (name[..k].to_string(), name[k + 2..].to_string()),
                        None => (section.to_string(), name.clone()),
                    };
                    if let Some(val) = self.get_string(Some(&sect), &var) {
                        out.push_str(&val);
                    }
                    i = j;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        out.trim_end().to_string()
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
        if c == '\\' {
            esc = true;
            out.push(c);
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            out.push(c);
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            continue;
        }
        if c == '#' {
            break;
        }
        out.push(c);
    }
    out
}

/// Default configuration file (OPENSSL_CONF or <cert area>/openssl.cnf).
pub fn default_conf_filename() -> String {
    match std::env::var("OPENSSL_CONF") {
        Ok(v) if !v.is_empty() => v,
        _ => "/etc/ssl/openssl.cnf".to_string(),
    }
}

thread_local! {
    static CUSTOM_OIDS: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
}
