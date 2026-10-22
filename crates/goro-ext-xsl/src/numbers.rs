//! xsl:number and format-number() (numbers.c)

use crate::style::{DecimalFormat, PreComp};
use crate::transform::Transform;
use crate::util::*;
use crate::xpath::{self, XObj};
use goro_ext_xml::tree::{NType, NodeId};

const MAX_TOKENS: usize = 1024;

#[derive(Clone, Debug)]
struct FormatToken {
    separator: Option<String>,
    token: u32,
    width: i32,
}

#[derive(Debug, Default)]
struct Format {
    start: Option<String>,
    tokens: Vec<FormatToken>,
    end: Option<String>,
}

fn is_letter_digit(c: u32) -> bool {
    goro_ext_xml::xmlunicode::is_letter(c) || goro_ext_xml::xmlunicode::is_digit(c)
}

fn is_digit_zero(c: u32) -> bool {
    matches!(
        c,
        0x0030
            | 0x0660
            | 0x06F0
            | 0x0966
            | 0x09E6
            | 0x0A66
            | 0x0AE6
            | 0x0B66
            | 0x0C66
            | 0x0CE6
            | 0x0D66
            | 0x0E50
            | 0x0ED0
            | 0x0F20
    )
}

fn is_digit_one(c: u32) -> bool {
    c > 0 && is_digit_zero(c - 1)
}

/// xsltNumberFormatDecimal
pub fn format_decimal(
    buffer: &mut String,
    mut number: f64,
    digit_zero: u32,
    width: i32,
    digits_per_group: i32,
    grouping_character: u32,
) {
    let mut out: Vec<char> = Vec::new();
    let mut i = 0;
    let mut chars = 0usize;
    loop {
        if chars >= 490 {
            php_error_noctx("xsltNumberFormatDecimal: Internal buffer size exceeded\n");
            break;
        }
        if i >= width && number.abs() < 1.0 {
            break;
        }
        if i > 0 && grouping_character != 0 && digits_per_group > 0 && i % digits_per_group == 0 {
            if let Some(c) = char::from_u32(grouping_character) {
                out.push(c);
                chars += c.len_utf8();
            }
        }
        let v = digit_zero as i64 + (number % 10.0) as i64;
        if let Some(c) = char::from_u32(v as u32) {
            out.push(c);
            chars += c.len_utf8();
        }
        number /= 10.0;
        i += 1;
    }
    for c in out.into_iter().rev() {
        buffer.push(c);
    }
}

fn php_error_noctx(_m: &str) {}

/// xsltNumberFormatAlpha
fn format_alpha(
    data: &crate::style::NumberData,
    buffer: &mut String,
    mut number: f64,
    upper: bool,
) {
    if number < 1.0 {
        format_decimal(
            buffer,
            number,
            '0' as u32,
            1,
            data.digits_per_group,
            data.grouping_character,
        );
        return;
    }
    let list = if upper {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    } else {
        b"abcdefghijklmnopqrstuvwxyz"
    };
    let mut out = Vec::new();
    for _ in 1..(std::mem::size_of::<f64>() * 8 + 1) {
        number -= 1.0;
        let idx = (number % 26.0) as usize;
        out.push(list[idx.min(25)]);
        number /= 26.0;
        if number < 1.0 {
            break;
        }
    }
    out.reverse();
    buffer.push_str(&String::from_utf8_lossy(&out));
}

/// xsltNumberFormatRoman
fn format_roman(
    data: &crate::style::NumberData,
    buffer: &mut String,
    mut number: f64,
    upper: bool,
) {
    if !(1.0..=5000.0).contains(&number) {
        format_decimal(
            buffer,
            number,
            '0' as u32,
            1,
            data.digits_per_group,
            data.grouping_character,
        );
        return;
    }
    let mut add = |s: &str| {
        if upper {
            buffer.push_str(&s.to_uppercase());
        } else {
            buffer.push_str(s);
        }
    };
    while number >= 1000.0 {
        add("m");
        number -= 1000.0;
    }
    if number >= 900.0 {
        add("cm");
        number -= 900.0;
    }
    while number >= 500.0 {
        add("d");
        number -= 500.0;
    }
    if number >= 400.0 {
        add("cd");
        number -= 400.0;
    }
    while number >= 100.0 {
        add("c");
        number -= 100.0;
    }
    if number >= 90.0 {
        add("xc");
        number -= 90.0;
    }
    while number >= 50.0 {
        add("l");
        number -= 50.0;
    }
    if number >= 40.0 {
        add("xl");
        number -= 40.0;
    }
    while number >= 10.0 {
        add("x");
        number -= 10.0;
    }
    if number >= 9.0 {
        add("ix");
        number -= 9.0;
    }
    while number >= 5.0 {
        add("v");
        number -= 5.0;
    }
    if number >= 4.0 {
        add("iv");
        number -= 4.0;
    }
    while number >= 1.0 {
        add("i");
        number -= 1.0;
    }
}

/// xsltNumberFormatTokenize
fn tokenize(format: &str) -> Format {
    let chars: Vec<char> = format.chars().collect();
    let mut f = Format::default();
    let mut ix = 0;
    while ix < chars.len() && !is_letter_digit(chars[ix] as u32) {
        ix += 1;
    }
    if ix > 0 {
        f.start = Some(chars[..ix].iter().collect());
    }
    while f.tokens.len() < MAX_TOKENS {
        if ix >= chars.len() {
            break;
        }
        let mut tok = FormatToken {
            separator: None,
            token: '0' as u32,
            width: 1,
        };
        if !f.tokens.is_empty() {
            tok.separator = f.end.take();
        }
        let mut val = chars[ix] as u32;
        if is_digit_one(val) || is_digit_zero(val) {
            tok.width = 1;
            while ix < chars.len() && is_digit_zero(chars[ix] as u32) {
                tok.width += 1;
                ix += 1;
            }
            val = chars.get(ix).map(|c| *c as u32).unwrap_or(0);
            if is_digit_one(val) {
                tok.token = val - 1;
                ix += 1;
            } else {
                tok.token = '0' as u32;
                tok.width = 1;
            }
        } else if matches!(chars[ix], 'A' | 'a' | 'I' | 'i') {
            tok.token = val;
            ix += 1;
        } else {
            tok.token = '0' as u32;
            tok.width = 1;
        }
        while ix < chars.len() && is_letter_digit(chars[ix] as u32) {
            ix += 1;
        }
        let j = ix;
        while ix < chars.len() && !is_letter_digit(chars[ix] as u32) {
            ix += 1;
        }
        if ix > j {
            f.end = Some(chars[j..ix].iter().collect());
        }
        f.tokens.push(tok);
    }
    f
}

/// xsltNumberFormatInsertNumbers
fn insert_numbers(
    t: &mut Transform,
    data: &crate::style::NumberData,
    numbers: &[f64],
    tokens: &Format,
    buffer: &mut String,
) {
    if let Some(s) = &tokens.start {
        buffer.push_str(s);
    }
    let default_token = FormatToken {
        separator: Some(".".into()),
        token: '0' as u32,
        width: 1,
    };
    let n = numbers.len();
    for i in 0..n {
        let mut number = numbers[(n - 1) - i];
        number = (number + 0.5).floor();
        if number < 0.0 {
            transform_error(t.vm, false, false, None, "xsl-number : negative value\n");
            number = 0.0;
        }
        let token = if i < tokens.tokens.len() {
            &tokens.tokens[i]
        } else if !tokens.tokens.is_empty() {
            &tokens.tokens[tokens.tokens.len() - 1]
        } else {
            &default_token
        };
        if i > 0 {
            match &token.separator {
                Some(s) => buffer.push_str(s),
                None => buffer.push('.'),
            }
        }
        if number == f64::NEG_INFINITY {
            buffer.push_str("-Infinity");
        } else if number == f64::INFINITY {
            buffer.push_str("Infinity");
        } else if number.is_nan() {
            buffer.push_str("NaN");
        } else {
            match char::from_u32(token.token) {
                Some('A') => format_alpha(data, buffer, number, true),
                Some('a') => format_alpha(data, buffer, number, false),
                Some('I') => format_roman(data, buffer, number, true),
                Some('i') => format_roman(data, buffer, number, false),
                _ => {
                    if is_digit_zero(token.token) {
                        format_decimal(
                            buffer,
                            number,
                            token.token,
                            token.width,
                            data.digits_per_group,
                            data.grouping_character,
                        );
                    }
                }
            }
        }
    }
    if let Some(e) = &tokens.end {
        buffer.push_str(e);
    }
}

/// xsltTestCompMatchCount
fn test_comp_match_count(
    t: &mut Transform,
    node: NodeId,
    count_pat: Option<&Vec<usize>>,
    cur: NodeId,
) -> bool {
    match count_pat {
        Some(p) => t.test_comp_match_list(node, p),
        None => tw(|tr| {
            let a = &tr.nodes[node];
            let b = &tr.nodes[cur];
            if a.t != b.t {
                return false;
            }
            if a.t == NType::NamespaceDecl {
                return true;
            }
            if a.name != b.name {
                return false;
            }
            if a.ns == b.ns {
                return true;
            }
            if a.ns.is_none() || b.ns.is_none() {
                return false;
            }
            tr.ns_href(a.ns) == tr.ns_href(b.ns)
        }),
    }
}

/// xsltNumberFormatGetAnyLevel
fn get_any_level(
    t: &mut Transform,
    node: NodeId,
    count_pat: Option<&Vec<usize>>,
    from_pat: Option<&Vec<usize>>,
) -> f64 {
    let mut cnt = 0;
    let mut cur = Some(node);
    while let Some(c) = cur {
        if test_comp_match_count(t, c, count_pat, node) {
            cnt += 1;
        }
        if let Some(fp) = from_pat {
            if t.test_comp_match_list(c, fp) {
                break;
            }
        }
        cur = tw(|tr| {
            let n = &tr.nodes[c];
            match n.t {
                NType::Document | NType::HtmlDocument => None,
                NType::NamespaceDecl | NType::Attribute => n.parent,
                _ => {
                    let mut x = c;
                    while let Some(pv) = tr.nodes[x].prev {
                        if tr.nodes[pv].t == NType::Dtd {
                            x = pv;
                        } else {
                            break;
                        }
                    }
                    match tr.nodes[x].prev {
                        Some(pv) => {
                            let mut y = pv;
                            while let Some(l) = tr.nodes[y].last {
                                y = l;
                            }
                            Some(y)
                        }
                        None => tr.nodes[x].parent,
                    }
                }
            }
        });
    }
    cnt as f64
}

/// xsltNumberFormatGetMultipleLevel
fn get_multiple_level(
    t: &mut Transform,
    node: NodeId,
    count_pat: Option<&Vec<usize>>,
    from_pat: Option<&Vec<usize>>,
    max: usize,
) -> Vec<f64> {
    let mut out = Vec::new();
    let mut ancestor = Some(node);
    while let Some(a) = ancestor {
        if tw(|tr| tr.nodes[a].t == NType::Document) {
            break;
        }
        if let Some(fp) = from_pat {
            if t.test_comp_match_list(a, fp) {
                break;
            }
        }
        if test_comp_match_count(t, a, count_pat, node) {
            let mut cnt = 1;
            let mut preceding = tw(|tr| {
                if tr.nodes[a].t != NType::NamespaceDecl {
                    tr.nodes[a].prev
                } else {
                    None
                }
            });
            while let Some(pv) = preceding {
                if test_comp_match_count(t, pv, count_pat, node) {
                    cnt += 1;
                }
                preceding = tw(|tr| tr.nodes[pv].prev);
            }
            out.push(cnt as f64);
            if out.len() >= max {
                break;
            }
        }
        ancestor = tw(|tr| {
            let n = &tr.nodes[a];
            if n.t == NType::NamespaceDecl {
                n.parent.filter(|&p| tr.nodes[p].t != NType::NamespaceDecl)
            } else {
                n.parent
            }
        });
    }
    out
}

/// xsltNumberFormat
pub fn number_format(t: &mut Transform, comp: &PreComp, inst: NodeId, node: NodeId) {
    let data = &comp.numdata;
    let tokens = match &data.format {
        Some(f) => tokenize(f),
        None => {
            if !data.has_format {
                return;
            }
            let f = match t.eval_attr_value_template(inst, "format", Some(XSLT_NAMESPACE)) {
                Some(f) => f,
                None => return,
            };
            tokenize(&f)
        }
    };
    let mut output = String::new();
    if let Some(value) = &data.value {
        let expr = format!("number({})", value);
        let old_node = t.xp.node;
        t.xp.node = Some(node);
        let r = match xpath::compile(&expr) {
            Ok(c) => xpath::eval(t, &c),
            Err(msgs) => {
                for m in msgs {
                    php_error(t.vm, &format!("{}\n", m));
                }
                None
            }
        };
        t.xp.node = old_node;
        if let Some(r) = r {
            let n = match r {
                XObj::Num(n) => n,
                _ => 0.0,
            };
            insert_numbers(t, data, &[n], &tokens, &mut output);
        }
    } else if let Some(level) = &data.level {
        match level.as_str() {
            "single" => {
                let v =
                    get_multiple_level(t, node, data.count_pat.as_ref(), data.from_pat.as_ref(), 1);
                if v.len() == 1 {
                    insert_numbers(t, data, &v, &tokens, &mut output);
                }
            }
            "multiple" => {
                let v = get_multiple_level(
                    t,
                    node,
                    data.count_pat.as_ref(),
                    data.from_pat.as_ref(),
                    1024,
                );
                if !v.is_empty() {
                    insert_numbers(t, data, &v, &tokens, &mut output);
                }
            }
            "any" => {
                let n = get_any_level(t, node, data.count_pat.as_ref(), data.from_pat.as_ref());
                insert_numbers(t, data, &[n], &tokens, &mut output);
            }
            _ => {}
        }
    }
    let ins = t.insert;
    t.copy_text_string(ins, &output, false);
}

/// First UTF-8 character of `s` compared with the start of `rest`
/// (xsltUTF8Charcmp(rest, s) == 0)
fn starts_with_char(rest: &str, s: &str) -> bool {
    match s.chars().next() {
        None => false,
        Some(c) => rest.starts_with(c),
    }
}

struct FmtInfo {
    integer_hash: i32,
    integer_digits: i32,
    frac_digits: i32,
    frac_hash: i32,
    group: i32,
    multiplier: i32,
    add_decimal: bool,
    is_multiplier_set: bool,
    is_negative_pattern: bool,
}

fn is_special(df: &DecimalFormat, rest: &str) -> bool {
    starts_with_char(rest, &df.zero_digit)
        || starts_with_char(rest, &df.digit)
        || starts_with_char(rest, &df.decimal_point)
        || starts_with_char(rest, &df.grouping)
        || starts_with_char(rest, &df.pattern_separator)
}

/// xsltFormatNumberPreSuffix: returns the byte count (or -1) and advances
fn pre_suffix(df: &DecimalFormat, fmt: &str, pos: &mut usize, info: &mut FmtInfo) -> i32 {
    let mut count = 0i32;
    loop {
        let rest = &fmt[*pos..];
        let c = match rest.chars().next() {
            None => return count,
            Some(c) => c,
        };
        if c == '\'' {
            *pos += 1;
            if *pos >= fmt.len() {
                return -1;
            }
        } else if is_special(df, rest) {
            return count;
        } else if starts_with_char(rest, &df.percent) {
            if info.is_multiplier_set {
                return -1;
            }
            info.multiplier = 100;
            info.is_multiplier_set = true;
        } else if starts_with_char(rest, &df.permille) {
            if info.is_multiplier_set {
                return -1;
            }
            info.multiplier = 1000;
            info.is_multiplier_set = true;
        }
        let rest = &fmt[*pos..];
        let len = match rest.chars().next() {
            None => return -1,
            Some(c) => c.len_utf8(),
        };
        count += len as i32;
        *pos += len;
    }
}

/// xsltFormatNumberConversion
pub fn format_number_conversion(
    t: &mut Transform,
    df: &DecimalFormat,
    format: &str,
    number: f64,
) -> String {
    if format.is_empty() {
        transform_error(
            t.vm,
            false,
            false,
            None,
            "xsltFormatNumberConversion : Invalid format (0-length)\n",
        );
    }
    if number.is_nan() {
        return df.no_number.clone();
    }
    let mut info = FmtInfo {
        integer_hash: 0,
        integer_digits: 0,
        frac_digits: 0,
        frac_hash: 0,
        group: -1,
        multiplier: 1,
        add_decimal: false,
        is_multiplier_set: false,
        is_negative_pattern: false,
    };
    let mut pos = 0usize;
    let mut found_error = false;
    let mut default_sign = false;
    let mut prefix_start = 0usize;
    let mut prefix_length: i32;
    let mut suffix_start = 0usize;
    let mut suffix_length: i32 = 0;
    let mut delayed_multiplier = 0;
    'parse: {
        prefix_length = pre_suffix(df, format, &mut pos, &mut info);
        if prefix_length < 0 {
            found_error = true;
            break 'parse;
        }
        let grouping_len = df.grouping.len();
        let mut len = 0usize;
        while pos < format.len() {
            let rest = &format[pos..];
            if starts_with_char(rest, &df.decimal_point)
                || starts_with_char(rest, &df.pattern_separator)
            {
                break;
            }
            if delayed_multiplier != 0 {
                info.multiplier = delayed_multiplier;
                info.is_multiplier_set = true;
                delayed_multiplier = 0;
            }
            if starts_with_char(rest, &df.digit) {
                if info.integer_digits > 0 {
                    found_error = true;
                    break 'parse;
                }
                info.integer_hash += 1;
                if info.group >= 0 {
                    info.group += 1;
                }
            } else if starts_with_char(rest, &df.zero_digit) {
                info.integer_digits += 1;
                if info.group >= 0 {
                    info.group += 1;
                }
            } else if grouping_len > 0 && rest.starts_with(df.grouping.as_str()) {
                info.group = 0;
                pos += grouping_len;
                continue;
            } else if starts_with_char(rest, &df.percent) {
                if info.is_multiplier_set {
                    found_error = true;
                    break 'parse;
                }
                delayed_multiplier = 100;
            } else if starts_with_char(rest, &df.permille) {
                if info.is_multiplier_set {
                    found_error = true;
                    break 'parse;
                }
                delayed_multiplier = 1000;
            } else {
                break;
            }
            len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            pos += len;
        }
        if pos < format.len() && starts_with_char(&format[pos..], &df.decimal_point) {
            info.add_decimal = true;
            pos += format[pos..].chars().next().unwrap().len_utf8();
        }
        while pos < format.len() {
            let rest = &format[pos..];
            if starts_with_char(rest, &df.zero_digit) {
                if info.frac_hash != 0 {
                    found_error = true;
                    break 'parse;
                }
                info.frac_digits += 1;
            } else if starts_with_char(rest, &df.digit) {
                info.frac_hash += 1;
            } else if starts_with_char(rest, &df.percent) {
                if info.is_multiplier_set {
                    found_error = true;
                    break 'parse;
                }
                delayed_multiplier = 100;
                len = rest.chars().next().unwrap().len_utf8();
                pos += len;
                continue;
            } else if starts_with_char(rest, &df.permille) {
                if info.is_multiplier_set {
                    found_error = true;
                    break 'parse;
                }
                delayed_multiplier = 1000;
                len = rest.chars().next().unwrap().len_utf8();
                pos += len;
                continue;
            } else if !starts_with_char(rest, &df.grouping) {
                break;
            }
            len = rest.chars().next().unwrap().len_utf8();
            pos += len;
            if delayed_multiplier != 0 {
                info.multiplier = delayed_multiplier;
                delayed_multiplier = 0;
                info.is_multiplier_set = true;
            }
        }
        if delayed_multiplier != 0 {
            pos -= len;
            delayed_multiplier = 0;
        }
        suffix_start = pos;
        suffix_length = pre_suffix(df, format, &mut pos, &mut info);
        if suffix_length < 0
            || (pos < format.len() && !starts_with_char(&format[pos..], &df.pattern_separator))
        {
            found_error = true;
            break 'parse;
        }
        if number < 0.0 {
            let sep = df.pattern_separator.chars().next();
            let j = sep.and_then(|s| format.find(s));
            match j {
                None => default_sign = true,
                Some(jb) => {
                    pos = jb + sep.unwrap().len_utf8();
                    info.is_negative_pattern = true;
                    info.is_multiplier_set = false;
                    let nprefix_start = pos;
                    let nprefix_length = pre_suffix(df, format, &mut pos, &mut info);
                    if nprefix_length < 0 {
                        found_error = true;
                        break 'parse;
                    }
                    let mut len = 0;
                    while pos < format.len() {
                        let rest = &format[pos..];
                        if starts_with_char(rest, &df.percent)
                            || starts_with_char(rest, &df.permille)
                        {
                            if info.is_multiplier_set {
                                found_error = true;
                                break 'parse;
                            }
                            info.is_multiplier_set = true;
                            delayed_multiplier = 1;
                        } else if is_special(df, rest) {
                            delayed_multiplier = 0;
                        } else {
                            break;
                        }
                        len = rest.chars().next().unwrap().len_utf8();
                        pos += len;
                    }
                    if delayed_multiplier != 0 {
                        info.is_multiplier_set = false;
                        pos -= len;
                    }
                    let (nsuffix_start, nsuffix_length) = if pos < format.len() {
                        let st = pos;
                        let l = pre_suffix(df, format, &mut pos, &mut info);
                        if l < 0 {
                            found_error = true;
                            break 'parse;
                        }
                        (st, l)
                    } else {
                        (pos, 0)
                    };
                    if pos < format.len() {
                        found_error = true;
                        break 'parse;
                    }
                    let differ = nprefix_length != prefix_length
                        || nsuffix_length != suffix_length
                        || (nprefix_length > 0
                            && format.as_bytes()
                                [nprefix_start..nprefix_start + prefix_length.max(0) as usize]
                                != format.as_bytes()
                                    [prefix_start..prefix_start + prefix_length.max(0) as usize])
                        || (nsuffix_length > 0
                            && format
                                .as_bytes()
                                .get(nsuffix_start..nsuffix_start + suffix_length.max(0) as usize)
                                != format.as_bytes().get(
                                    suffix_start..suffix_start + suffix_length.max(0) as usize,
                                ));
                    if differ {
                        prefix_start = nprefix_start;
                        prefix_length = nprefix_length;
                        suffix_start = nsuffix_start;
                        suffix_length = nsuffix_length;
                    }
                }
            }
        }
    }
    if found_error {
        transform_error(
            t.vm,
            false,
            false,
            None,
            &format!(
                "xsltFormatNumberConversion : error in format string '{}', using default\n",
                format
            ),
        );
        default_sign = number < 0.0;
        prefix_length = 0;
        suffix_length = 0;
        info.integer_hash = 0;
        info.integer_digits = 1;
        info.frac_digits = 1;
        info.frac_hash = 4;
        info.group = -1;
        info.multiplier = 1;
        info.add_decimal = true;
    }
    let mut number = number * info.multiplier as f64;
    if number == f64::NEG_INFINITY {
        return format!("{}{}", df.minus_sign, df.infinity);
    }
    if number == f64::INFINITY {
        return df.infinity.clone();
    }
    let mut buffer = String::new();
    if default_sign {
        if let Some(c) = df.minus_sign.chars().next() {
            buffer.push(c);
        }
    }
    let copy_affix = |buffer: &mut String, start: usize, length: i32| {
        let mut j = 0;
        let mut p = start;
        while j < length {
            if format[p..].starts_with('\'') {
                p += 1;
            }
            let c = match format[p..].chars().next() {
                Some(c) => c,
                None => break,
            };
            buffer.push(c);
            p += c.len_utf8();
            j += c.len_utf8() as i32;
        }
    };
    copy_affix(&mut buffer, prefix_start, prefix_length);
    number = number.abs();
    let mut exp10 = info.frac_digits + info.frac_hash;
    if exp10 > 308 {
        if info.frac_digits > 308 {
            info.frac_digits = 308;
            info.frac_hash = 0;
        } else {
            info.frac_hash = 308 - info.frac_digits;
        }
        exp10 = 308;
    }
    let scale = 10f64.powf(exp10 as f64);
    number += 0.5 / scale;
    number -= number % (1.0 / scale);
    let zero = df
        .zero_digit
        .chars()
        .next()
        .map(|c| c as u32)
        .unwrap_or('0' as u32);
    let zero_byte = df.zero_digit.as_bytes().first().copied().unwrap_or(b'0') as u32;
    if !df.grouping.is_empty() {
        let gchar = df.grouping.chars().next().unwrap() as u32;
        format_decimal(
            &mut buffer,
            number.floor(),
            zero_byte,
            info.integer_digits,
            info.group,
            gchar,
        );
    } else {
        format_decimal(
            &mut buffer,
            number.floor(),
            zero_byte,
            info.integer_digits,
            info.group,
            ',' as u32,
        );
    }
    let _ = zero;
    if info.integer_digits + info.integer_hash + info.frac_digits == 0 && info.frac_hash > 0 {
        info.frac_digits += 1;
        info.frac_hash -= 1;
    }
    if number.floor() == 0.0 && info.integer_digits + info.frac_digits == 0 {
        if let Some(c) = df.zero_digit.chars().next() {
            buffer.push(c);
        }
    }
    if info.frac_digits + info.frac_hash == 0 {
        if info.add_decimal {
            if let Some(c) = df.decimal_point.chars().next() {
                buffer.push(c);
            }
        }
    } else {
        number -= number.floor();
        if number != 0.0 || info.frac_digits != 0 {
            if let Some(c) = df.decimal_point.chars().next() {
                buffer.push(c);
            }
            number = (scale * number + 0.5).floor();
            let mut j = info.frac_hash;
            while j > 0 {
                if number % 10.0 >= 1.0 {
                    break;
                }
                number /= 10.0;
                j -= 1;
            }
            format_decimal(
                &mut buffer,
                number.floor(),
                zero_byte,
                info.frac_digits + j,
                0,
                0,
            );
        }
    }
    copy_affix(&mut buffer, suffix_start, suffix_length);
    buffer
}
