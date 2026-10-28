//! mbregex (PHP manual: mb_ereg and friends) on goro-regex.
//!
//! PHP's mbregex uses Oniguruma's Ruby syntax; patterns here run on the
//! PCRE-compatible engine, which shares that syntax for everything the
//! tests use (groups, named groups, classes, POSIX brackets, quantifiers).
//! The options map as Oniguruma documents them: `m` makes `.` match a
//! newline, `s` makes `^`/`$` anchor at the string only, `p` is both, `i`
//! ignores case and `x` is extended. Patterns and subjects in another
//! encoding than UTF-8 are converted to UTF-8 to match, and offsets mapped
//! back (observed on PHP 8.5 and tests ext/mbstring/tests/mb_ereg*.phpt).

use goro_charset::Encoding;
use goro_regex::{Captures, CompileConfig, Flags, Limits, MatchOptions, Regex, Scratch};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::st;
use crate::func::Args;
use crate::vm::{R, Vm};

/// The name mb_regex_encoding() reports for an encoding Oniguruma
/// supports (test mb_regex_encoding_variation2.phpt; UTF-32 is UCS-4).
pub(crate) fn onig_name(e: Encoding) -> Option<&'static str> {
    use Encoding::*;
    Some(match e {
        Utf8 => "UTF-8",
        Ucs4 | Utf32 | Utf32Be => "UCS-4",
        Ucs4Le | Utf32Le => "UCS-4LE",
        Utf16 | Utf16Be => "UTF-16",
        Utf16Le => "UTF-16LE",
        Ascii => "ASCII",
        EucJp | EucJpWin | Cp51932 => "EUC-JP",
        Sjis | SjisWin | Cp932 => "SJIS",
        Iso8859_1 => "ISO-8859-1",
        Iso8859_2 => "ISO-8859-2",
        Iso8859_3 => "ISO-8859-3",
        Iso8859_4 => "ISO-8859-4",
        Iso8859_5 => "ISO-8859-5",
        Iso8859_6 => "ISO-8859-6",
        Iso8859_7 => "ISO-8859-7",
        Iso8859_8 => "ISO-8859-8",
        Iso8859_9 => "ISO-8859-9",
        Iso8859_10 => "ISO-8859-10",
        Iso8859_13 => "ISO-8859-13",
        Iso8859_14 => "ISO-8859-14",
        Iso8859_15 => "ISO-8859-15",
        Iso8859_16 => "ISO-8859-16",
        EucCn => "EUC-CN",
        EucTw => "EUC-TW",
        Big5 => "BIG5",
        EucKr => "EUC-KR",
        Koi8R => "KOI8R",
        _ => return None,
    })
}

pub(crate) fn mb_regex_encoding(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.opt_str(0) {
        None => {
            let e = st(vm).regex_enc;
            Ok(Value::from(onig_name(e).unwrap_or("UTF-8")))
        }
        // the byte2be... aliases are not Oniguruma's
        Some(n) if n.len() > 4 && n[..4].eq_ignore_ascii_case(b"byte") => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                n.to_string_lossy()
            ),
        ),
        Some(n) => match Encoding::from_name(&n).filter(|&e| onig_name(e).is_some()) {
            Some(e) => {
                st(vm).regex_enc = e;
                Ok(Value::Bool(true))
            }
            None => vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #1 ($encoding) must be a valid encoding, \"{}\" given",
                    n.to_string_lossy()
                ),
            ),
        },
    }
}

// ---------------------------------------------------------------- options

/// Parsed mbregex options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RxOpts {
    pub icase: bool,
    pub extended: bool,
    /// `m`: `.` matches a newline.
    pub dotall: bool,
    /// `s`: `^`/`$` only at the ends of the string.
    pub single: bool,
    pub syntax: u8,
}

impl Default for RxOpts {
    /// "pr" (observed on PHP 8.5: mb_regex_set_options()).
    fn default() -> Self {
        RxOpts {
            icase: false,
            extended: false,
            dotall: true,
            single: true,
            syntax: b'r',
        }
    }
}

impl RxOpts {
    /// The options of a string; the first unknown letter is an error.
    fn parse(s: &[u8]) -> Result<RxOpts, u8> {
        let mut o = RxOpts {
            icase: false,
            extended: false,
            dotall: false,
            single: false,
            syntax: b'r',
        };
        for &c in s {
            match c {
                b'i' => o.icase = true,
                b'x' => o.extended = true,
                b'm' => o.dotall = true,
                b's' => o.single = true,
                b'p' => {
                    o.dotall = true;
                    o.single = true;
                }
                b'j' | b'u' | b'g' | b'c' | b'r' | b'z' | b'b' | b'd' => o.syntax = c,
                b'l' | b'n' => {}
                _ => return Err(c),
            }
        }
        Ok(o)
    }

    /// The options as mb_regex_set_options() reports them.
    fn text(self) -> String {
        let mut s = String::new();
        if self.icase {
            s.push('i');
        }
        if self.extended {
            s.push('x');
        }
        match (self.dotall, self.single) {
            (true, true) => s.push('p'),
            (true, false) => s.push('m'),
            (false, true) => s.push('s'),
            _ => {}
        }
        s.push(self.syntax as char);
        s
    }

    fn flags(self) -> Flags {
        // Oniguruma's \w, \d, [[:alpha:]] are Unicode; names may repeat
        // (tests mb_ereg_named_subpatterns.phpt, mb_ereg_dupnames.phpt)
        let mut f = Flags::UTF | Flags::UCP | Flags::DUPNAMES;
        if self.icase {
            f |= Flags::CASELESS;
        }
        if self.extended {
            f |= Flags::EXTENDED;
        }
        if self.dotall {
            f |= Flags::DOTALL;
        }
        if self.single {
            f |= Flags::DOLLAR_ENDONLY;
        } else {
            f |= Flags::MULTILINE;
        }
        f
    }
}

#[derive(Default)]
pub(crate) struct RxState {
    opts: Option<RxOpts>,
    search: Option<Search>,
}

struct Search {
    /// The subject as given and as matched (UTF-8), with the offset map.
    text: Text,
    /// False when the string was invalid in the regex encoding.
    valid: bool,
    re: Option<Regex>,
    /// Byte position in the original string.
    pos: usize,
    regs: Option<Vec<Option<(usize, usize)>>>,
    names: Vec<(Vec<u8>, usize)>,
}

fn opts(vm: &mut Vm) -> RxOpts {
    vm.ext::<RxState>().opts.unwrap_or_default()
}

/// Options given as an argument, or the current ones.
fn opts_arg(vm: &mut Vm, a: &Args, i: usize) -> R<RxOpts> {
    match a.opt_str(i) {
        None => Ok(opts(vm)),
        Some(s) => parse_or_throw(vm, &s),
    }
}

/// Options from a string; an unknown letter throws (the message has no
/// function prefix: tests bug72164.phpt, bug76999.phpt).
fn parse_or_throw(vm: &mut Vm, s: &[u8]) -> R<RxOpts> {
    match RxOpts::parse(s) {
        Ok(o) => Ok(o),
        Err(c) => vm.throw(
            "ValueError",
            &format!("Option \"{}\" is not supported", c as char),
        ),
    }
}

pub(crate) fn mb_regex_set_options(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let old = opts(vm);
    if let Some(s) = a.opt_str(0) {
        let o = parse_or_throw(vm, &s)?;
        vm.ext::<RxState>().opts = Some(o);
        return Ok(Value::from(old.text().as_str()));
    }
    Ok(Value::from(old.text().as_str()))
}

// ---------------------------------------------------------------- text

/// A string in the regex encoding, as UTF-8 for matching.
struct Text {
    orig: Vec<u8>,
    utf8: Vec<u8>,
    /// (UTF-8 offset, original offset) at each character start, and the
    /// ends; empty when the string is UTF-8 already.
    map: Vec<(usize, usize)>,
}

impl Text {
    fn new(s: &[u8], enc: Encoding) -> Option<Text> {
        if !goro_charset::check_encoding(s, enc) {
            return None;
        }
        if enc == Encoding::Utf8 {
            return Some(Text {
                orig: s.to_vec(),
                utf8: s.to_vec(),
                map: Vec::new(),
            });
        }
        let bounds = goro_charset::ops::char_boundaries(s, enc);
        let mut utf8 = Vec::with_capacity(s.len());
        let mut map = Vec::with_capacity(bounds.len());
        let sub = goro_charset::Subst::default();
        for w in bounds.windows(2) {
            map.push((utf8.len(), w[0]));
            let c = goro_charset::convert(&s[w[0]..w[1]], Encoding::Utf8, enc, &sub);
            utf8.extend_from_slice(&c.bytes);
        }
        map.push((utf8.len(), s.len()));
        Some(Text {
            orig: s.to_vec(),
            utf8,
            map,
        })
    }

    /// The original offset of a UTF-8 offset.
    fn orig_off(&self, u: usize) -> usize {
        if self.map.is_empty() {
            return u;
        }
        let i = self.map.partition_point(|&(x, _)| x <= u);
        self.map[i.saturating_sub(1)].1
    }

    /// The UTF-8 offset of an original offset.
    fn utf8_off(&self, o: usize) -> usize {
        if self.map.is_empty() {
            return o;
        }
        let i = self.map.partition_point(|&(_, y)| y <= o);
        self.map[i.saturating_sub(1)].0
    }

    fn len(&self) -> usize {
        self.orig.len()
    }
}

/// Compiles a pattern given in the regex encoding; warns and gives none
/// when it is invalid.
fn compile(vm: &mut Vm, a: &Args, pat: &[u8], o: RxOpts) -> R<Option<Regex>> {
    let enc = st(vm).regex_enc;
    let Some(p) = Text::new(pat, enc) else {
        vm.fwarn(
            a,
            &format!("Pattern is not valid under {} encoding", enc.name()),
        )?;
        return Ok(None);
    };
    let cfg = CompileConfig {
        flags: o.flags(),
        ..Default::default()
    };
    match Regex::with_config(&p.utf8, &cfg) {
        // with named groups, plain parentheses do not capture (Oniguruma's
        // default; test mb_ereg_named_subpatterns.phpt)
        Ok(r) if !r.names().is_empty() => {
            let cfg = CompileConfig {
                flags: o.flags() | Flags::NO_AUTO_CAPTURE,
                ..Default::default()
            };
            Ok(Regex::with_config(&p.utf8, &cfg).ok().or(Some(r)))
        }
        Ok(r) => Ok(Some(r)),
        Err(e) => {
            vm.fwarn(a, &format!("mbregex compile err: {}", onig_message(&e)))?;
            Ok(None)
        }
    }
}

/// Oniguruma's wording for the common compile errors.
fn onig_message(e: &goro_regex::CompileError) -> String {
    use goro_regex::ErrorKind as K;
    match e.kind {
        K::MissingClosingParenthesis => "end pattern with unmatched parenthesis".into(),
        K::UnmatchedClosingParenthesis => "unmatched close parenthesis".into(),
        K::MissingSquareBracket => "premature end of char-class".into(),
        K::QuantifierInvalid => "target of repeat operator is not specified".into(),
        K::EndBackslash => "end pattern at escape".into(),
        _ => e.message(),
    }
}

struct Found {
    groups: Vec<Option<(usize, usize)>>,
}

/// Matches at or after the UTF-8 offset `start`; group offsets are in the
/// original string.
fn find(vm: &mut Vm, re: &Regex, t: &Text, start: usize, anchored: bool) -> Option<Found> {
    let mut scratch = Scratch::new();
    let mut caps = Captures::default();
    let limit = vm
        .get_ini(b"mbstring.regex_retry_limit")
        .map(|v| goro_runtime::convert::str_to_int(&v))
        .unwrap_or(1_000_000)
        .max(1) as u64;
    let limits = Limits::new(
        limit.saturating_mul(10).min(u32::MAX as u64) as u32,
        100_000,
    );
    let opts = MatchOptions {
        anchored,
        no_utf_check: true,
        ..Default::default()
    };
    match re.exec_with(&mut scratch, &t.utf8, start, opts, limits, &mut caps) {
        Ok(true) => {
            let n = re.capture_count() + 1;
            let groups = (0..n)
                .map(|i| caps.get(i).map(|(s, e)| (t.orig_off(s), t.orig_off(e))))
                .collect();
            Some(Found { groups })
        }
        _ => None,
    }
}

fn names_of(re: &Regex) -> Vec<(Vec<u8>, usize)> {
    re.names()
        .iter()
        .map(|(n, i)| (n.to_vec(), *i as usize))
        .collect()
}

/// The matches array: groups by number (false when unset), then by name.
/// The matches array: groups by number, then by name; an unset or empty
/// group is false (tests mb_ereg_named_subpatterns.phpt,
/// mb_ereg_variation6.phpt); a repeated name gives the group that matched
/// (test mb_ereg_dupnames.phpt).
fn regs_array(t: &[u8], groups: &[Option<(usize, usize)>], names: &[(Vec<u8>, usize)]) -> PhpArray {
    let val = |g: &Option<(usize, usize)>| match g {
        Some((s, e)) if s < e => Value::Str(PhpStr::from(t[*s..*e].to_vec())),
        _ => Value::Bool(false),
    };
    let mut out = PhpArray::new();
    for (i, g) in groups.iter().enumerate() {
        out.insert(Key::Int(i as i64), val(g));
    }
    let mut seen: Vec<&[u8]> = Vec::new();
    for (n, _) in names {
        if seen.contains(&n.as_slice()) {
            continue;
        }
        seen.push(n);
        let idx: Vec<usize> = names
            .iter()
            .filter(|(m, _)| m == n)
            .map(|(_, i)| *i)
            .collect();
        let pick = idx
            .iter()
            .rev()
            .find(|&&i| matches!(groups.get(i), Some(Some(_))))
            .or(idx.first());
        if let Some(g) = pick.and_then(|&i| groups.get(i)) {
            out.insert(Key::from_bytes(n), val(g));
        }
    }
    out
}

fn check_pattern(vm: &mut Vm, a: &Args, p: &[u8]) -> R<()> {
    if p.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($pattern) must not be empty");
    }
    Ok(())
}

fn ereg(vm: &mut Vm, a: &mut Args, icase: bool) -> R<Value> {
    let pat = a.str(0);
    check_pattern(vm, a, &pat)?;
    let mut o = opts(vm);
    o.icase |= icase;
    let Some(re) = compile(vm, a, &pat, o)? else {
        a.set_ref(2, Value::Array(PhpArray::new()));
        return Ok(Value::Bool(false));
    };
    let enc = st(vm).regex_enc;
    let Some(t) = Text::new(a.bytes(1), enc) else {
        a.set_ref(2, Value::Array(PhpArray::new()));
        return Ok(Value::Bool(false));
    };
    match find(vm, &re, &t, 0, false) {
        Some(f) => {
            a.set_ref(
                2,
                Value::Array(regs_array(&t.orig, &f.groups, &names_of(&re))),
            );
            Ok(Value::Bool(true))
        }
        None => {
            a.set_ref(2, Value::Array(PhpArray::new()));
            Ok(Value::Bool(false))
        }
    }
}

pub(crate) fn mb_ereg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ereg(vm, a, false)
}

pub(crate) fn mb_eregi(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ereg(vm, a, true)
}

pub(crate) fn mb_ereg_match(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let o = opts_arg(vm, a, 2)?;
    let Some(re) = compile(vm, a, &pat, o)? else {
        return Ok(Value::Bool(false));
    };
    let enc = st(vm).regex_enc;
    let Some(t) = Text::new(a.bytes(1), enc) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(find(vm, &re, &t, 0, true).is_some()))
}

/// Expands `\0`-`\9` and `\k<name>`/`\k'name'` in a replacement.
fn expand(rep: &[u8], t: &[u8], f: &Found, names: &[(Vec<u8>, usize)], out: &mut Vec<u8>) {
    let grp = |i: usize, out: &mut Vec<u8>| {
        if let Some(Some((s, e))) = f.groups.get(i) {
            out.extend_from_slice(&t[*s..*e]);
        }
    };
    let mut i = 0;
    while i < rep.len() {
        let c = rep[i];
        if c == b'\\' && i + 1 < rep.len() {
            let d = rep[i + 1];
            // \N names an existing group, and only when there are no
            // named groups (tests mb_ereg_replace-compat-09.phpt,
            // mb_ereg_replace_named_subpatterns.phpt)
            if d.is_ascii_digit() && names.is_empty() && ((d - b'0') as usize) < f.groups.len() {
                grp((d - b'0') as usize, out);
                i += 2;
                continue;
            }
            if d == b'k' && i + 2 < rep.len() && (rep[i + 2] == b'<' || rep[i + 2] == b'\'') {
                let close = if rep[i + 2] == b'<' { b'>' } else { b'\'' };
                if let Some(end) = rep[i + 3..].iter().position(|&x| x == close) {
                    let name = &rep[i + 3..i + 3 + end];
                    let idx = if !name.is_empty() && name.iter().all(|x| x.is_ascii_digit()) {
                        std::str::from_utf8(name)
                            .ok()
                            .filter(|n| names.is_empty() && !(n.len() > 1 && n.starts_with('0')))
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|&i| i < f.groups.len())
                    } else {
                        names.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
                    };
                    if let Some(idx) = idx {
                        grp(idx, out);
                        i += 4 + end;
                        continue;
                    }
                }
            }
            // "\\" is a backslash, and the second one still starts an
            // escape: "\\\\1" gives "\\" and group 1 (test bug77428.phpt)
            if d == b'\\' {
                out.push(b'\\');
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
}

/// The replace loop; `cb` builds each replacement.
fn replace_all(
    vm: &mut Vm,
    re: &Regex,
    t: &Text,
    cb: &mut dyn FnMut(&mut Vm, &Found, &mut Vec<u8>) -> R<()>,
) -> R<Vec<u8>> {
    let mut out = Vec::with_capacity(t.len());
    let mut last = 0; // original offset copied up to
    let mut start = 0; // UTF-8 search start
    loop {
        if start > t.utf8.len() {
            break;
        }
        let Some(f) = find(vm, re, t, start, false) else {
            break;
        };
        let (s, e) = f.groups[0].unwrap_or((0, 0));
        out.extend_from_slice(&t.orig[last..s]);
        cb(vm, &f, &mut out)?;
        last = e;
        let ue = t.utf8_off(e);
        if e == s {
            // an empty match: copy one character and go on after it
            let next = next_char(t, ue);
            if next > t.utf8.len() {
                break;
            }
            let oe = t.orig_off(next).max(e);
            if next <= t.utf8.len() {
                out.extend_from_slice(&t.orig[e..oe.min(t.len())]);
            }
            last = oe.min(t.len());
            start = next;
            if ue >= t.utf8.len() {
                break;
            }
        } else {
            start = ue;
        }
    }
    if last < t.len() {
        out.extend_from_slice(&t.orig[last..]);
    }
    Ok(out)
}

/// The UTF-8 offset after the character at `u` (past the end at the end).
fn next_char(t: &Text, u: usize) -> usize {
    if u >= t.utf8.len() {
        return u + 1;
    }
    let mut n = u + 1;
    while n < t.utf8.len() && (t.utf8[n] & 0xC0) == 0x80 {
        n += 1;
    }
    n
}

fn ereg_replace(vm: &mut Vm, a: &mut Args, icase: bool) -> R<Value> {
    let pat = a.str(0);
    // the search pattern is forgotten (test bug72399.phpt)
    if let Some(s) = vm.ext::<RxState>().search.as_mut() {
        s.re = None;
    }
    let mut o = opts_arg(vm, a, 3)?;
    o.icase |= icase;
    let Some(re) = compile(vm, a, &pat, o)? else {
        return Ok(Value::Bool(false));
    };
    let enc = st(vm).regex_enc;
    let Some(t) = Text::new(a.bytes(2), enc) else {
        return Ok(Value::Null);
    };
    let rep = a.str(1);
    let names = names_of(&re);
    let out = replace_all(vm, &re, &t, &mut |_, f, out| {
        expand(&rep, &t.orig, f, &names, out);
        Ok(())
    })?;
    Ok(Value::Str(PhpStr::from(out)))
}

pub(crate) fn mb_ereg_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ereg_replace(vm, a, false)
}

pub(crate) fn mb_eregi_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ereg_replace(vm, a, true)
}

pub(crate) fn mb_ereg_replace_callback(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let o = opts_arg(vm, a, 3)?;
    let Some(re) = compile(vm, a, &pat, o)? else {
        return Ok(Value::Bool(false));
    };
    let enc = st(vm).regex_enc;
    let Some(t) = Text::new(a.bytes(2), enc) else {
        return Ok(Value::Null);
    };
    let cb = a.val(1);
    let names = names_of(&re);
    let out = replace_all(vm, &re, &t, &mut |vm, f, out| {
        let arr = regs_array(&t.orig, &f.groups, &names);
        let r = vm.call_value(&cb, vec![Value::Array(arr)])?;
        let s = vm.to_php_str(&r)?;
        out.extend_from_slice(&s);
        Ok(())
    })?;
    Ok(Value::Str(PhpStr::from(out)))
}

pub(crate) fn mb_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    // 0 counts as 1 (test gh18901.phpt)
    let mut limit = match a.int_or(2, -1) {
        0 => 1,
        l => l,
    };
    let o = opts(vm);
    let enc = st(vm).regex_enc;
    let mut out = PhpArray::new();
    let Some(t) = Text::new(a.bytes(1), enc) else {
        return Ok(Value::Bool(false));
    };
    if pat.is_empty() {
        out.push(Value::Str(PhpStr::from(t.orig.clone()))).ok();
        return Ok(Value::Array(out));
    }
    let Some(re) = compile(vm, a, &pat, o)? else {
        return Ok(Value::Bool(false));
    };
    let mut piece = 0; // original offset of the current piece
    let mut start = 0; // UTF-8 search start
    while limit != 1 && start <= t.utf8.len() {
        let Some(f) = find(vm, &re, &t, start, false) else {
            break;
        };
        let (s, e) = f.groups[0].unwrap_or((0, 0));
        if e == s && s <= piece {
            // an empty match where the piece starts does not split
            let ue = t.utf8_off(e);
            start = next_char(&t, ue);
            continue;
        }
        out.push(Value::Str(PhpStr::from(t.orig[piece..s].to_vec())))
            .ok();
        piece = e;
        let ue = t.utf8_off(e);
        start = if e == s { next_char(&t, ue) } else { ue };
        if limit > 0 {
            limit -= 1;
        }
    }
    out.push(Value::Str(PhpStr::from(
        t.orig[piece.min(t.len())..].to_vec(),
    )))
    .ok();
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- search

pub(crate) fn mb_ereg_search_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enc = st(vm).regex_enc;
    let re = match a.opt_str(1) {
        Some(p) => {
            if p.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #2 ($pattern) must not be empty");
            }
            let o = opts_arg(vm, a, 2)?;
            match compile(vm, a, &p, o)? {
                Some(r) => Some(r),
                None => return Ok(Value::Bool(false)),
            }
        }
        None => vm.ext::<RxState>().search.take().and_then(|s| s.re),
    };
    // an invalid string is kept as nothing to search (test bug69151.phpt)
    let (text, valid) = match Text::new(a.bytes(0), enc) {
        Some(t) => (t, true),
        None => (
            Text {
                orig: Vec::new(),
                utf8: Vec::new(),
                map: Vec::new(),
            },
            false,
        ),
    };
    let names = re.as_ref().map(names_of).unwrap_or_default();
    vm.ext::<RxState>().search = Some(Search {
        text,
        valid,
        re,
        pos: 0,
        regs: None,
        names,
    });
    Ok(Value::Bool(true))
}

/// One search step; the match groups, or none.
fn search_step(vm: &mut Vm, a: &Args) -> R<Option<Vec<Option<(usize, usize)>>>> {
    if let Some(p) = a.opt_str(0) {
        let o = opts_arg(vm, a, 1)?;
        let Some(re) = compile(vm, a, &p, o)? else {
            // an invalid pattern discards the last match (test
            // mb_ereg_search_invalid_pattern.phpt)
            if let Some(s) = vm.ext::<RxState>().search.as_mut() {
                s.regs = None;
            }
            return Ok(None);
        };
        let names = names_of(&re);
        if let Some(s) = vm.ext::<RxState>().search.as_mut() {
            s.re = Some(re);
            s.names = names;
        }
    }
    // the pattern is checked first (test empty_pattern.phpt)
    if a.opt_str(0).is_none()
        && vm
            .ext::<RxState>()
            .search
            .as_ref()
            .is_none_or(|s| s.re.is_none())
    {
        return vm.throw("Error", "No pattern was provided");
    }
    let Some(mut s) = vm.ext::<RxState>().search.take() else {
        return vm.throw("Error", "No string was provided");
    };
    let mut r = None;
    if s.valid && s.pos <= s.text.len() {
        let start = s.text.utf8_off(s.pos);
        let re = s.re.take().unwrap();
        if let Some(f) = find(vm, &re, &s.text, start, false) {
            let (b, e) = f.groups[0].unwrap_or((0, 0));
            // a zero-width match leaves the position (test bug72693.phpt)
            let _ = b;
            s.pos = e;
            s.regs = Some(f.groups.clone());
            r = Some(f.groups);
        } else {
            s.regs = None;
        }
        s.re = Some(re);
    }
    vm.ext::<RxState>().search = Some(s);
    Ok(r)
}

pub(crate) fn mb_ereg_search(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(search_step(vm, a)?.is_some()))
}

pub(crate) fn mb_ereg_search_pos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match search_step(vm, a)? {
        Some(g) => {
            let (s, e) = g[0].unwrap_or((0, 0));
            Value::Array(PhpArray::from_values([
                Value::Int(s as i64),
                Value::Int((e - s) as i64),
            ]))
        }
        None => Value::Bool(false),
    })
}

fn current_regs(vm: &mut Vm) -> Value {
    let st = vm.ext::<RxState>();
    match &st.search {
        Some(s) => match &s.regs {
            Some(g) => Value::Array(regs_array(&s.text.orig, g, &s.names)),
            None => Value::Bool(false),
        },
        None => Value::Bool(false),
    }
}

pub(crate) fn mb_ereg_search_regs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match search_step(vm, a)? {
        Some(_) => current_regs(vm),
        None => Value::Bool(false),
    })
}

pub(crate) fn mb_ereg_search_getregs(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(current_regs(vm))
}

pub(crate) fn mb_ereg_search_getpos(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        vm.ext::<RxState>()
            .search
            .as_ref()
            .map_or(0, |s| s.pos as i64),
    ))
}

pub(crate) fn mb_ereg_search_setpos(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut off = a.int(0);
    let len = vm
        .ext::<RxState>()
        .search
        .as_ref()
        .map(|s| s.text.len() as i64);
    // without a string any position is accepted (test
    // mb_ereg_search_setpos.phpt)
    let Some(len) = len else {
        if off < 0 {
            return vm.fthrow(a, "ValueError", "Argument #1 ($offset) is out of range");
        }
        return Ok(Value::Bool(true));
    };
    if off < 0 {
        off += len;
    }
    if off < 0 || off > len {
        return vm.fthrow(a, "ValueError", "Argument #1 ($offset) is out of range");
    }
    if let Some(s) = vm.ext::<RxState>().search.as_mut() {
        s.pos = off as usize;
    }
    Ok(Value::Bool(true))
}
