//! Character encodings: lexbor's encoding label table
//! (lxb_encoding_data_by_name), the HTML prescan for a meta charset
//! (html/encoding.c) and the bridge to encoding_rs, which implements the
//! same WHATWG Encoding Standard decoders.

/// (label, encoding name) sorted by label; generated from lexbor's encoding/res.c
static LABELS: [(&str, &str); 219] = [
    ("866", "IBM866"),
    ("ansi_x3.4-1968", "windows-1252"),
    ("arabic", "ISO-8859-6"),
    ("ascii", "windows-1252"),
    ("asmo-708", "ISO-8859-6"),
    ("big5", "Big5"),
    ("big5-hkscs", "Big5"),
    ("chinese", "GBK"),
    ("cn-big5", "Big5"),
    ("cp1250", "windows-1250"),
    ("cp1251", "windows-1251"),
    ("cp1252", "windows-1252"),
    ("cp1253", "windows-1253"),
    ("cp1254", "windows-1254"),
    ("cp1255", "windows-1255"),
    ("cp1256", "windows-1256"),
    ("cp1257", "windows-1257"),
    ("cp1258", "windows-1258"),
    ("cp819", "windows-1252"),
    ("cp866", "IBM866"),
    ("csbig5", "Big5"),
    ("cseuckr", "EUC-KR"),
    ("cseucpkdfmtjapanese", "EUC-JP"),
    ("csgb2312", "GBK"),
    ("csibm866", "IBM866"),
    ("csiso2022jp", "ISO-2022-JP"),
    ("csiso2022kr", "replacement"),
    ("csiso58gb231280", "GBK"),
    ("csiso88596e", "ISO-8859-6"),
    ("csiso88596i", "ISO-8859-6"),
    ("csiso88598e", "ISO-8859-8"),
    ("csiso88598i", "ISO-8859-8-I"),
    ("csisolatin1", "windows-1252"),
    ("csisolatin2", "ISO-8859-2"),
    ("csisolatin3", "ISO-8859-3"),
    ("csisolatin4", "ISO-8859-4"),
    ("csisolatin5", "windows-1254"),
    ("csisolatin6", "ISO-8859-10"),
    ("csisolatin9", "ISO-8859-15"),
    ("csisolatinarabic", "ISO-8859-6"),
    ("csisolatincyrillic", "ISO-8859-5"),
    ("csisolatingreek", "ISO-8859-7"),
    ("csisolatinhebrew", "ISO-8859-8"),
    ("cskoi8r", "KOI8-R"),
    ("csksc56011987", "EUC-KR"),
    ("csmacintosh", "macintosh"),
    ("csshiftjis", "Shift_JIS"),
    ("cyrillic", "ISO-8859-5"),
    ("dos-874", "windows-874"),
    ("ecma-114", "ISO-8859-6"),
    ("ecma-118", "ISO-8859-7"),
    ("elot_928", "ISO-8859-7"),
    ("euc-jp", "EUC-JP"),
    ("euc-kr", "EUC-KR"),
    ("gb18030", "gb18030"),
    ("gb2312", "GBK"),
    ("gb_2312", "GBK"),
    ("gb_2312-80", "GBK"),
    ("gbk", "GBK"),
    ("greek", "ISO-8859-7"),
    ("greek8", "ISO-8859-7"),
    ("hebrew", "ISO-8859-8"),
    ("hz-gb-2312", "replacement"),
    ("ibm819", "windows-1252"),
    ("ibm866", "IBM866"),
    ("iso-2022-cn", "replacement"),
    ("iso-2022-cn-ext", "replacement"),
    ("iso-2022-jp", "ISO-2022-JP"),
    ("iso-2022-kr", "replacement"),
    ("iso-8859-1", "windows-1252"),
    ("iso-8859-10", "ISO-8859-10"),
    ("iso-8859-11", "windows-874"),
    ("iso-8859-13", "ISO-8859-13"),
    ("iso-8859-14", "ISO-8859-14"),
    ("iso-8859-15", "ISO-8859-15"),
    ("iso-8859-16", "ISO-8859-16"),
    ("iso-8859-2", "ISO-8859-2"),
    ("iso-8859-3", "ISO-8859-3"),
    ("iso-8859-4", "ISO-8859-4"),
    ("iso-8859-5", "ISO-8859-5"),
    ("iso-8859-6", "ISO-8859-6"),
    ("iso-8859-6-e", "ISO-8859-6"),
    ("iso-8859-6-i", "ISO-8859-6"),
    ("iso-8859-7", "ISO-8859-7"),
    ("iso-8859-8", "ISO-8859-8"),
    ("iso-8859-8-e", "ISO-8859-8"),
    ("iso-8859-8-i", "ISO-8859-8-I"),
    ("iso-8859-9", "windows-1254"),
    ("iso-ir-100", "windows-1252"),
    ("iso-ir-101", "ISO-8859-2"),
    ("iso-ir-109", "ISO-8859-3"),
    ("iso-ir-110", "ISO-8859-4"),
    ("iso-ir-126", "ISO-8859-7"),
    ("iso-ir-127", "ISO-8859-6"),
    ("iso-ir-138", "ISO-8859-8"),
    ("iso-ir-144", "ISO-8859-5"),
    ("iso-ir-148", "windows-1254"),
    ("iso-ir-149", "EUC-KR"),
    ("iso-ir-157", "ISO-8859-10"),
    ("iso-ir-58", "GBK"),
    ("iso8859-1", "windows-1252"),
    ("iso8859-10", "ISO-8859-10"),
    ("iso8859-11", "windows-874"),
    ("iso8859-13", "ISO-8859-13"),
    ("iso8859-14", "ISO-8859-14"),
    ("iso8859-15", "ISO-8859-15"),
    ("iso8859-2", "ISO-8859-2"),
    ("iso8859-3", "ISO-8859-3"),
    ("iso8859-4", "ISO-8859-4"),
    ("iso8859-5", "ISO-8859-5"),
    ("iso8859-6", "ISO-8859-6"),
    ("iso8859-7", "ISO-8859-7"),
    ("iso8859-8", "ISO-8859-8"),
    ("iso8859-9", "windows-1254"),
    ("iso88591", "windows-1252"),
    ("iso885910", "ISO-8859-10"),
    ("iso885911", "windows-874"),
    ("iso885913", "ISO-8859-13"),
    ("iso885914", "ISO-8859-14"),
    ("iso885915", "ISO-8859-15"),
    ("iso88592", "ISO-8859-2"),
    ("iso88593", "ISO-8859-3"),
    ("iso88594", "ISO-8859-4"),
    ("iso88595", "ISO-8859-5"),
    ("iso88596", "ISO-8859-6"),
    ("iso88597", "ISO-8859-7"),
    ("iso88598", "ISO-8859-8"),
    ("iso88599", "windows-1254"),
    ("iso_8859-1", "windows-1252"),
    ("iso_8859-15", "ISO-8859-15"),
    ("iso_8859-1:1987", "windows-1252"),
    ("iso_8859-2", "ISO-8859-2"),
    ("iso_8859-2:1987", "ISO-8859-2"),
    ("iso_8859-3", "ISO-8859-3"),
    ("iso_8859-3:1988", "ISO-8859-3"),
    ("iso_8859-4", "ISO-8859-4"),
    ("iso_8859-4:1988", "ISO-8859-4"),
    ("iso_8859-5", "ISO-8859-5"),
    ("iso_8859-5:1988", "ISO-8859-5"),
    ("iso_8859-6", "ISO-8859-6"),
    ("iso_8859-6:1987", "ISO-8859-6"),
    ("iso_8859-7", "ISO-8859-7"),
    ("iso_8859-7:1987", "ISO-8859-7"),
    ("iso_8859-8", "ISO-8859-8"),
    ("iso_8859-8:1988", "ISO-8859-8"),
    ("iso_8859-9", "windows-1254"),
    ("iso_8859-9:1989", "windows-1254"),
    ("koi", "KOI8-R"),
    ("koi8", "KOI8-R"),
    ("koi8-r", "KOI8-R"),
    ("koi8-ru", "KOI8-U"),
    ("koi8-u", "KOI8-U"),
    ("koi8_r", "KOI8-R"),
    ("korean", "EUC-KR"),
    ("ks_c_5601-1987", "EUC-KR"),
    ("ks_c_5601-1989", "EUC-KR"),
    ("ksc5601", "EUC-KR"),
    ("ksc_5601", "EUC-KR"),
    ("l1", "windows-1252"),
    ("l2", "ISO-8859-2"),
    ("l3", "ISO-8859-3"),
    ("l4", "ISO-8859-4"),
    ("l5", "windows-1254"),
    ("l6", "ISO-8859-10"),
    ("l9", "ISO-8859-15"),
    ("latin1", "windows-1252"),
    ("latin2", "ISO-8859-2"),
    ("latin3", "ISO-8859-3"),
    ("latin4", "ISO-8859-4"),
    ("latin5", "windows-1254"),
    ("latin6", "ISO-8859-10"),
    ("logical", "ISO-8859-8-I"),
    ("mac", "macintosh"),
    ("macintosh", "macintosh"),
    ("ms932", "Shift_JIS"),
    ("ms_kanji", "Shift_JIS"),
    ("replacement", "replacement"),
    ("shift-jis", "Shift_JIS"),
    ("shift_jis", "Shift_JIS"),
    ("sjis", "Shift_JIS"),
    ("sun_eu_greek", "ISO-8859-7"),
    ("tis-620", "windows-874"),
    ("unicode-1-1-utf-8", "UTF-8"),
    ("us-ascii", "windows-1252"),
    ("utf-16", "UTF-16LE"),
    ("utf-16be", "UTF-16BE"),
    ("utf-16le", "UTF-16LE"),
    ("utf-8", "UTF-8"),
    ("utf8", "UTF-8"),
    ("visual", "ISO-8859-8"),
    ("windows-1250", "windows-1250"),
    ("windows-1251", "windows-1251"),
    ("windows-1252", "windows-1252"),
    ("windows-1253", "windows-1253"),
    ("windows-1254", "windows-1254"),
    ("windows-1255", "windows-1255"),
    ("windows-1256", "windows-1256"),
    ("windows-1257", "windows-1257"),
    ("windows-1258", "windows-1258"),
    ("windows-31j", "Shift_JIS"),
    ("windows-874", "windows-874"),
    ("windows-949", "EUC-KR"),
    ("x-cp1250", "windows-1250"),
    ("x-cp1251", "windows-1251"),
    ("x-cp1252", "windows-1252"),
    ("x-cp1253", "windows-1253"),
    ("x-cp1254", "windows-1254"),
    ("x-cp1255", "windows-1255"),
    ("x-cp1256", "windows-1256"),
    ("x-cp1257", "windows-1257"),
    ("x-cp1258", "windows-1258"),
    ("x-euc-jp", "EUC-JP"),
    ("x-gbk", "GBK"),
    ("x-mac-cyrillic", "x-mac-cyrillic"),
    ("x-mac-roman", "macintosh"),
    ("x-mac-ukrainian", "x-mac-cyrillic"),
    ("x-sjis", "Shift_JIS"),
    ("x-user-defined", "x-user-defined"),
    ("x-x-big5", "Big5"),
];

/// lxb_encoding_data_by_name: exact (ASCII case-insensitive) label lookup.
pub fn by_name(label: &[u8]) -> Option<&'static str> {
    if label.is_empty() {
        return None;
    }
    let l: Vec<u8> = label.iter().map(|c| c.to_ascii_lowercase()).collect();
    LABELS
        .binary_search_by(|(k, _)| k.as_bytes().cmp(&l[..]))
        .ok()
        .map(|i| LABELS[i].1)
}

fn is_sp(c: u8) -> bool {
    matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// lxb_encoding_data_by_pre_name: trims ASCII whitespace first.
pub fn by_pre_name(label: &[u8]) -> Option<&'static str> {
    let mut s = 0;
    let mut e = label.len();
    while s < e && is_sp(label[s]) {
        s += 1;
    }
    while e > s && is_sp(label[e - 1]) {
        e -= 1;
    }
    if s == e {
        return None;
    }
    by_name(&label[s..e])
}

/// The encoding_rs implementation of a lexbor encoding name.
pub fn to_encoding_rs(name: &str) -> &'static encoding_rs::Encoding {
    if name == "replacement" {
        return encoding_rs::REPLACEMENT;
    }
    encoding_rs::Encoding::for_label(name.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}

/// dom_common_save's encoder: UTF-8 output as is, other encodings with
/// "?" for unencodable code points (lexbor's replacement for non-UTF-8).
/// `label` is the document's encoding (looked up like
/// lxb_encoding_data_by_name).
pub fn encode_output(s: &str, label: Option<&str>) -> Vec<u8> {
    let mut e = OutputEncoder::new(label);
    let mut out = e.encode(s, false);
    out.extend(e.encode("", true));
    out
}

/// A streaming dom_common_save encoder (one per save, pieces in order).
pub struct OutputEncoder {
    name: &'static str,
    encoder: Option<encoding_rs::Encoder>,
}

impl OutputEncoder {
    pub fn new(label: Option<&str>) -> OutputEncoder {
        let name = label.and_then(|l| by_name(l.as_bytes())).unwrap_or("UTF-8");
        let encoder = match name {
            "UTF-8" | "UTF-16LE" | "UTF-16BE" | "replacement" => None,
            _ => Some(to_encoding_rs(name).new_encoder()),
        };
        OutputEncoder { name, encoder }
    }

    pub fn encode(&mut self, s: &str, last: bool) -> Vec<u8> {
        match self.name {
            "UTF-8" => s.as_bytes().to_vec(),
            "UTF-16LE" | "UTF-16BE" => {
                let mut out = Vec::with_capacity(s.len() * 2);
                for u in s.encode_utf16() {
                    if self.name == "UTF-16LE" {
                        out.extend_from_slice(&u.to_le_bytes());
                    } else {
                        out.extend_from_slice(&u.to_be_bytes());
                    }
                }
                out
            }
            "replacement" => Vec::new(),
            _ => {
                let encoder = self.encoder.as_mut().unwrap();
                let mut out = Vec::with_capacity(s.len() + 16);
                let mut src = s;
                loop {
                    let need = encoder
                        .max_buffer_length_from_utf8_without_replacement(src.len())
                        .unwrap_or(src.len() * 4 + 16);
                    out.reserve(need + 16);
                    let (res, read) =
                        encoder.encode_from_utf8_to_vec_without_replacement(src, &mut out, last);
                    src = &src[read..];
                    match res {
                        encoding_rs::EncoderResult::InputEmpty => break,
                        encoding_rs::EncoderResult::OutputFull => continue,
                        encoding_rs::EncoderResult::Unmappable(_) => out.push(b'?'),
                    }
                }
                out
            }
        }
    }
}

fn skip_spaces(d: &[u8], mut i: usize) -> usize {
    while i < d.len() && is_sp(d[i]) {
        i += 1;
    }
    i
}

fn skip_name(d: &[u8], mut i: usize) -> usize {
    while i < d.len() {
        if is_sp(d[i]) || d[i] == b'>' {
            return i;
        }
        i += 1;
    }
    d.len()
}

fn tag_end(d: &[u8], i: usize) -> usize {
    match d[i.min(d.len())..].iter().position(|&c| c == b'>') {
        Some(p) => i + p + 1,
        None => d.len(),
    }
}

type Span = Option<(usize, usize)>;

/// lxb_html_get_attribute: returns (new position, name, value)
fn get_attribute(d: &[u8], mut i: usize) -> (usize, Span, Span) {
    let end = d.len();
    loop {
        if i >= end {
            return (i, None, None);
        }
        match d[i] {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F => i += 1,
            0x3E => return (i + 1, None, None),
            _ => break,
        }
    }
    let name = i;
    let name_end;
    let mut goto_value = false;
    loop {
        if i >= end {
            name_end = i;
            break;
        }
        match d[i] {
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                name_end = i;
                i += 1;
                break;
            }
            b'/' | b'>' => return (i, Some((name, i)), None),
            b'=' => {
                name_end = i;
                i += 1;
                goto_value = true;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let nm = Some((name, name_end));
    if !goto_value {
        i = skip_spaces(d, i);
        if i == end {
            return (i, nm, None);
        }
        if d[i] != b'=' {
            return (i, nm, None);
        }
        i += 1;
    }
    i = skip_spaces(d, i);
    if i == end {
        return (i, nm, None);
    }
    match d[i] {
        b'"' | b'\'' => {
            let ch = d[i];
            i += 1;
            if i == end {
                return (i, nm, None);
            }
            let v = i;
            while i < end {
                if d[i] == ch {
                    return (i + 1, nm, Some((v, i)));
                }
                i += 1;
            }
            return (i, nm, None);
        }
        b'>' => return (i, nm, None),
        _ => {}
    }
    let v = i;
    i += 1;
    while i < end {
        if matches!(d[i], 0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x3E) {
            return (i, nm, Some((v, i)));
        }
        i += 1;
    }
    (i, nm, None)
}

/// lxb_html_encoding_content: the charset in a `content` attribute value
fn encoding_content(d: &[u8]) -> Span {
    let end = d.len();
    let mut i = 0;
    loop {
        let mut found = false;
        while i + 7 < end {
            if d[i..i + 7].eq_ignore_ascii_case(b"charset") {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return None;
        }
        i = skip_spaces(d, i + 7);
        if i >= end {
            return None;
        }
        if d[i] != b'=' {
            continue;
        }
        i = skip_spaces(d, i + 1);
        if i >= end {
            return None;
        }
        break;
    }
    if d[i] == b'\'' || d[i] == b'"' {
        let q = d[i];
        i += 1;
        let name = i;
        while i < end && d[i] != q {
            i += 1;
        }
        return Some((name, i));
    }
    let name = i;
    while i < end {
        match d[i] {
            b';' | 0x09 | 0x0A | 0x0C | 0x0D | 0x20 => return Some((name, i)),
            b'"' | b'\'' => return None,
            _ => {}
        }
        i += 1;
    }
    if i == name {
        return None;
    }
    Some((name, i))
}

/// lxb_html_encoding_meta: process the attributes of a <meta> tag.
fn encoding_meta(d: &[u8], mut i: usize, result: &mut Vec<(usize, usize)>) -> usize {
    let mut got_pragma = false;
    let mut have_content = false;
    let mut need_pragma = 0u8;
    let cur = result.len();
    let mut cache: Vec<(usize, usize)> = Vec::new();
    while i < d.len() {
        let (ni, name, value) = get_attribute(d, i);
        i = ni;
        let (ns, ne) = match name {
            None => break,
            Some(n) => n,
        };
        let len = ne - ns;
        if len < 7 {
            continue;
        }
        if cache
            .iter()
            .any(|&(cs, ce)| ce - cs == len && d[cs..ce].eq_ignore_ascii_case(&d[ns..ne]))
        {
            continue;
        }
        cache.push((ns, ne));
        let (vs, ve) = match value {
            None => continue,
            Some(v) => v,
        };
        if len == 10 {
            if !d[ns..ne].eq_ignore_ascii_case(b"http-equiv") {
                continue;
            }
            if ve - vs == 12 && d[vs..ve].eq_ignore_ascii_case(b"content-type") {
                got_pragma = true;
            }
            continue;
        }
        if d[ns..ns + 7].eq_ignore_ascii_case(b"content") {
            if !have_content {
                if let Some((cs, ce)) = encoding_content(&d[vs..ve]) {
                    result.push((vs + cs, vs + ce));
                    need_pragma = 2;
                    have_content = true;
                }
            }
            continue;
        }
        if d[ns..ns + 7].eq_ignore_ascii_case(b"charset") {
            result.push((vs, ve));
            need_pragma = 1;
        }
    }
    if (need_pragma == 0 || (need_pragma == 2 && !got_pragma)) && cur != result.len() {
        result.pop();
    }
    i
}

/// lxb_html_encoding_determine: all meta charset candidates in `d`.
fn determine(d: &[u8]) -> Vec<(usize, usize)> {
    let end = d.len();
    let mut result = Vec::new();
    let mut i = 0;
    while i < end {
        i = match d[i..].iter().position(|&c| c == b'<') {
            None => return result,
            Some(p) => i + p,
        };
        i += 1;
        if i == end {
            return result;
        }
        let mut skip_attrs = false;
        match d[i] {
            b'!' => {
                if i + 5 > end {
                    return result;
                }
                if d[i + 1] != b'-' || d[i + 2] != b'-' {
                    i = tag_end(d, i);
                    continue;
                }
                while i < end {
                    i = tag_end(d, i);
                    if i >= 3 && d[i - 3] == b'-' && d[i - 2] == b'-' {
                        break;
                    }
                }
            }
            b'?' => i = tag_end(d, i),
            b'/' => {
                i += 1;
                if i + 3 > end {
                    return result;
                }
                if d[i].is_ascii_alphabetic() {
                    skip_attrs = true;
                } else {
                    i = tag_end(d, i);
                }
            }
            c => {
                if !c.is_ascii_alphabetic() {
                    continue;
                }
                if i + 6 > end {
                    return result;
                }
                if !d[i..i + 4].eq_ignore_ascii_case(b"meta") {
                    skip_attrs = true;
                } else {
                    i += 4;
                    let c = d[i];
                    i += 1;
                    match c {
                        0x09 | 0x0A | 0x0C | 0x0D | 0x20 | 0x2F => {
                            i = encoding_meta(d, i, &mut result);
                        }
                        _ => skip_attrs = true,
                    }
                }
            }
        }
        if skip_attrs {
            i = skip_name(d, i);
            if i >= end {
                return result;
            }
            if d[i] == b'>' {
                i += 1;
                continue;
            }
            while i < end {
                let (ni, name, _) = get_attribute(d, i);
                i = ni;
                if name.is_none() {
                    break;
                }
            }
        }
    }
    result
}

/// dom_determine_encoding: BOM sniffing then the meta prescan of at most
/// 1024 bytes. Returns (encoding name, BOM length).
pub fn determine_encoding(source: &[u8]) -> (&'static str, usize) {
    if source.starts_with(b"\xEF\xBB\xBF") {
        return ("UTF-8", 3);
    }
    if source.starts_with(b"\xFE\xFF") {
        return ("UTF-16BE", 2);
    }
    if source.starts_with(b"\xFF\xFE") {
        return ("UTF-16LE", 2);
    }
    let d = &source[..source.len().min(1024)];
    let res = determine(d);
    if let Some(&(s, e)) = res.first() {
        if let Some(n) = by_pre_name(&d[s..e]) {
            return (n, 0);
        }
    }
    ("UTF-8", 0)
}
