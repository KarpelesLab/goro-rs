//! mb_encode_mimeheader / mb_decode_mimeheader (C: mb_mime_header_encode,
//! mb_mime_header_decode in mbstring.c)

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, ConvBuf, EncNo, Encoding, MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR};
use crate::mbstring::*;
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_encode_mimeheader", mb_encode_mimeheader);
    // $charset / $transfer_encoding are Z_PARAM_STR under a ?string arginfo:
    // checked by mb_encode_mimeheader() itself (see mime_str_param)
    {
        use goro_core::opcode::ParamType;
        let s = || Some(ParamType::Simple(b"string".to_vec()));
        vm.register_builtin_param_types(
            b"mb_encode_mimeheader",
            vec![s(), None, None, s(), Some(ParamType::Simple(b"int".to_vec()))],
        );
    }
    vm.register_function(b"mb_decode_mimeheader", mb_decode_mimeheader);
}

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const WBUF: usize = 90; // MBSTRING_HEADER_ENC_WCHAR_BUFSIZE
const MIN_WBUF: usize = 5; // MBSTRING_MIN_WCHAR_BUFSIZE

fn needs_qencode(c: u8) -> bool {
    use crate::mbfl::tables::unicode_prop::mime_char_needs_qencode;
    c > 0x7F || c == b'=' || mime_char_needs_qencode[c as usize] != 0
}

fn transfer_encoded_size(tmp: &[u8], base64: bool) -> usize {
    if base64 {
        tmp.len().div_ceil(3) * 4
    } else {
        tmp.iter()
            .map(|&c| if needs_qencode(c) { 3 } else { 1 })
            .sum()
    }
}

#[allow(clippy::chunks_exact_to_as_chunks)] // keep chunks_exact + remainder like the C loop
fn transfer_encode_mime_bytes(tmp: &mut ConvBuf, out: &mut Vec<u8>, base64: bool) {
    let p = &tmp.out;
    if base64 {
        let mut chunks = p.chunks_exact(3);
        for c in &mut chunks {
            let bits = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32;
            out.push(BASE64_TABLE[((bits >> 18) & 0x3F) as usize]);
            out.push(BASE64_TABLE[((bits >> 12) & 0x3F) as usize]);
            out.push(BASE64_TABLE[((bits >> 6) & 0x3F) as usize]);
            out.push(BASE64_TABLE[(bits & 0x3F) as usize]);
        }
        let r = chunks.remainder();
        if r.len() == 1 {
            let bits = r[0] as u32;
            out.push(BASE64_TABLE[((bits >> 2) & 0x3F) as usize]);
            out.push(BASE64_TABLE[((bits & 0x3) << 4) as usize]);
            out.push(b'=');
            out.push(b'=');
        } else if r.len() == 2 {
            let bits = ((r[0] as u32) << 8) | r[1] as u32;
            out.push(BASE64_TABLE[((bits >> 10) & 0x3F) as usize]);
            out.push(BASE64_TABLE[((bits >> 4) & 0x3F) as usize]);
            out.push(BASE64_TABLE[((bits & 0xF) << 2) as usize]);
            out.push(b'=');
        }
    } else {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for &c in p {
            if needs_qencode(c) {
                out.push(b'=');
                out.push(HEX[((c >> 4) & 0xF) as usize]);
                out.push(HEX[(c & 0xF) as usize]);
            } else {
                out.push(c);
            }
        }
    }
    tmp.out.clear();
}

/// Emulates repeated `to_wchar` calls into a bounded buffer, over a
/// pre-decoded codepoint array.
struct Src {
    all: Vec<u32>,
    pos: usize,
}

impl Src {
    fn remaining(&self) -> bool {
        self.pos < self.all.len()
    }
    fn fill(&mut self, dst: &mut [u32]) -> usize {
        let n = dst.len().min(self.all.len() - self.pos);
        dst[..n].copy_from_slice(&self.all[self.pos..self.pos + n]);
        self.pos += n;
        n
    }
}

/// C: `mb_mime_header_encode`
pub fn mime_header_encode(
    input: &[u8],
    incode: &Encoding,
    outcode: &Encoding,
    base64: bool,
    linefeed: &[u8],
    indent: i64,
) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut indent: usize = if !(0..74).contains(&indent) {
        0
    } else {
        indent as usize
    };
    let mut linefeed = &linefeed[..linefeed.len().min(8)];
    if let Some(z) = linefeed.iter().position(|&c| c == 0) {
        linefeed = &linefeed[..z];
    }
    let mime_name = outcode.mime_name.unwrap_or("");
    let mime_len = mime_name.len();
    let from_wchar = outcode.from_wchar.unwrap();

    let all = incode.decode(input);

    // If the entire input string is ASCII with no spaces (except possibly
    // leading spaces), just pass it through unchanged
    {
        let mut checking_leading_spaces = true;
        let mut passthrough = true;
        for &w in &all {
            if checking_leading_spaces {
                if w == b' ' as u32 {
                    continue;
                } else {
                    checking_leading_spaces = false;
                }
            }
            if !(0x21..=0x7E).contains(&w)
                || w == b'=' as u32
                || w == b'?' as u32
                || w == b'_' as u32
            {
                passthrough = false;
                break;
            }
        }
        if passthrough {
            return input.to_vec();
        }
    }

    let mut src = Src { all, pos: 0 };
    let mut buf: Vec<u8> = Vec::with_capacity(input.len());
    let mut wbuf = [0u32; WBUF];
    let mut offset = 0usize;
    let mut line_start = 0usize;
    let mut p: usize;
    let mut e: usize;

    // ASCII phase
    loop {
        if !src.remaining() {
            return buf;
        }
        let out_len = src.fill(&mut wbuf[offset..WBUF]);
        p = 0;
        e = offset + out_len;
        let mut word_start = 0usize;
        while p < e && wbuf[p] == b' ' as u32 && (p - word_start) <= 74 {
            p += 1;
        }
        let mut feed = false;
        while p < e {
            let w = wbuf[p];
            p += 1;
            if !(0x20..=0x7E).contains(&w)
                || w == b'?' as u32
                || w == b'=' as u32
                || w == b'_' as u32
                || (w == b' ' as u32 && (p - word_start) > 74)
            {
                feed = true;
                break;
            } else if w == b' ' as u32 {
                if buf.len() - line_start + (p - word_start) + indent > 75 {
                    buf.extend_from_slice(linefeed);
                    buf.push(b' ');
                    indent = 0;
                    line_start = buf.len();
                } else if !buf.is_empty() {
                    buf.push(b' ');
                }
                while word_start < p - 1 {
                    buf.push((wbuf[word_start] & 0xFF) as u8);
                    word_start += 1;
                }
                word_start += 1;
                while p < e && wbuf[p] == b' ' as u32 {
                    p += 1;
                }
            }
        }
        if !feed {
            if src.remaining() {
                if word_start < MIN_WBUF {
                    feed = true;
                } else {
                    offset = e - word_start;
                    wbuf.copy_within(word_start..e, 0);
                }
            } else {
                if word_start < e && !buf.is_empty() {
                    if buf.len() - line_start + (p - word_start) + indent > 74 {
                        buf.extend_from_slice(linefeed);
                        buf.push(b' ');
                    } else {
                        buf.push(b' ');
                    }
                }
                while word_start < e {
                    buf.push((wbuf[word_start] & 0xFF) as u8);
                    word_start += 1;
                }
                return buf;
            }
        }
        if feed {
            // feed_and_mime_encode:
            if buf.len() - line_start + indent + mime_len > 55 {
                buf.extend_from_slice(linefeed);
                buf.push(b' ');
                indent = 0;
                line_start = buf.len();
            } else if !buf.is_empty() {
                buf.push(b' ');
            }
            p = word_start;
            break;
        }
    }

    // mime_encoding_needed:
    let mut tmpbuf = ConvBuf::new(
        input.len(),
        b'?' as u32,
        MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
    );
    offset = e - p;
    let mut need_refill;
    if WBUF - offset < MIN_WBUF {
        need_refill = false;
    } else {
        wbuf.copy_within(p..e, 0);
        need_refill = true;
    }

    loop {
        if need_refill {
            let out_len = src.fill(&mut wbuf[offset..WBUF]);
            p = 0;
            e = offset + out_len;
        }
        // start_new_line:
        buf.extend_from_slice(b"=?");
        buf.extend_from_slice(mime_name.as_bytes());
        buf.push(b'?');
        buf.push(if base64 { b'B' } else { b'Q' });
        buf.push(b'?');

        let mut n: usize = 12;
        let space_available = (73usize)
            .wrapping_sub(indent)
            .wrapping_sub(buf.len() - line_start);

        loop {
            if p >= e {
                // Should not happen; avoid looping forever
                buf.extend_from_slice(b"?=");
                return buf;
            }
            let tmppos = tmpbuf.out.len();
            let tmpstate = tmpbuf.state;
            n = n.min(e - p);
            from_wchar(&wbuf[p..p + n], &mut tmpbuf, false);
            let tmppos2 = tmpbuf.out.len();
            let tmpstate2 = tmpbuf.state;
            from_wchar(&[], &mut tmpbuf, true);

            if transfer_encoded_size(&tmpbuf.out, base64) <= space_available
                || (n == 1 && tmppos == 0)
            {
                p += n;
                if p == e {
                    transfer_encode_mime_bytes(&mut tmpbuf, &mut buf, base64);
                    buf.extend_from_slice(b"?=");
                    return buf;
                } else {
                    tmpbuf.out.truncate(tmppos2);
                    tmpbuf.state = tmpstate2;
                }
            } else {
                tmpbuf.out.truncate(tmppos);
                tmpbuf.state = tmpstate;
                if n == 1 {
                    from_wchar(&[], &mut tmpbuf, true);
                    transfer_encode_mime_bytes(&mut tmpbuf, &mut buf, base64);
                    tmpbuf.state = 0;
                    buf.extend_from_slice(b"?=");
                    indent = 0;
                    if src.remaining() || p < e {
                        buf.extend_from_slice(linefeed);
                        buf.push(b' ');
                        line_start = buf.len();
                        offset = e - p;
                        if src.remaining() && (WBUF - offset >= MIN_WBUF) {
                            wbuf.copy_within(p..e, 0);
                            need_refill = true;
                        } else {
                            need_refill = false;
                        }
                        break;
                    } else {
                        return buf;
                    }
                } else {
                    n = (n >> 1).max(1);
                }
            }
        }
    }
}

/// Z_PARAM_STR of mb_encode_mimeheader()'s ?string-declared parameters: in
/// strict mode only a string (null included is a TypeError of type
/// "string"); a NAN is coerced with its warning.
#[inline(never)]
fn mime_str_param(
    vm: &mut Vm,
    f: &str,
    args: &[Value],
    i: usize,
    pname: &str,
) -> Result<(), VmError> {
    let Some(v) = args.get(i).map(|v| v.deref()) else {
        return Ok(());
    };
    if matches!(v, Value::Undef) {
        return Ok(());
    }
    if vm.caller_strict_types && !matches!(v, Value::String(_)) {
        let msg = format!(
            "{}(): Argument #{} (${}) must be of type string, {} given",
            f,
            i + 1,
            pname,
            Vm::value_type_name(&v)
        );
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    if matches!(v, Value::Double(d) if d.is_nan()) {
        let line = vm.current_line;
        vm.warn_nan_coercion(&v, "string", line);
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: vm.uncaught_exception_message(),
                line,
            });
        }
    }
    Ok(())
}

fn mb_encode_mimeheader(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_encode_mimeheader";
    let s = str_arg(vm, f, args, 0, "string")?;
    state::sync(vm);
    let mut charset: &'static Encoding = &mbfl::basic::ENC_PASS;
    let mut base64 = true;
    // Z_PARAM_STR (not _OR_NULL) despite the ?string arginfo: an explicit
    // null is deprecated and parsed as "".
    let null_given = |vm: &mut Vm, i: usize, pname: &str| {
        let given = matches!(args.get(i).map(|v| v.deref()), Some(Value::Null));
        if given {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} (${}) of type ?string is deprecated",
                f,
                i + 1,
                pname
            ));
        }
        given
    };
    // (all the arguments are parsed before the charset is looked up)
    mime_str_param(vm, f, args, 1, "charset")?;
    let charset_null = null_given(vm, 1, "charset");
    let charset_name = if present(args, 1) || charset_null {
        Some(str_arg(vm, f, args, 1, "charset")?)
    } else {
        None
    };
    mime_str_param(vm, f, args, 2, "transfer_encoding")?;
    let transenc_null = null_given(vm, 2, "transfer_encoding");
    let transenc = if present(args, 2) || transenc_null {
        Some(str_arg(vm, f, args, 2, "transfer_encoding")?)
    } else {
        None
    };
    let linefeed = if present(args, 3) {
        str_arg(vm, f, args, 3, "newline")?
    } else {
        b"\r\n".to_vec()
    };
    let indent = if present(args, 4) {
        long_arg(vm, f, args, 4, "indent")?
    } else {
        0
    };
    if let Some(name) = charset_name {
        charset = get_encoding(vm, f, Some(&name), 2, "charset")?;
        if charset.mime_name.map(|m| m.is_empty()).unwrap_or(true) || charset.no == EncNo::Qprint {
            return Err(arg_value_error(
                vm,
                f,
                2,
                "charset",
                &format!(
                    "\"{}\" cannot be used for MIME header encoding",
                    goro_core::string::msg_str(&name)
                ),
            ));
        }
    } else {
        let lang = state::no2language(state::with(|s| s.language));
        if let Some(c) = mbfl::no2encoding(lang.mail_charset) {
            charset = c;
        }
        if let Some(t) = mbfl::no2encoding(lang.mail_header_encoding) {
            let c = t.name.as_bytes()[0];
            if c == b'Q' || c == b'q' {
                base64 = false;
            }
        }
    }
    if let Some(t) = transenc {
        if !t.is_empty() && (t[0] == b'Q' || t[0] == b'q') {
            base64 = false;
        }
    }
    if charset.from_wchar.is_none() || charset.mime_name.is_none() {
        // pass (no language charset found): output unchanged
        return Ok(pstring(s));
    }
    let incode = state::current_internal_encoding();
    Ok(pstring(mime_header_encode(
        &s, incode, charset, base64, &linefeed, indent,
    )))
}

fn decode_base64(c: u8) -> i8 {
    match c {
        b'A'..=b'Z' => (c - b'A') as i8,
        b'a'..=b'z' => (c - b'a' + 26) as i8,
        b'0'..=b'9' => (c - b'0' + 52) as i8,
        b'+' => 62,
        b'/' => 63,
        _ => -1,
    }
}

fn qprint_map(c: u8) -> i8 {
    match c {
        b'0'..=b'9' => (c - b'0') as i8,
        b'A'..=b'F' => (c - b'A' + 10) as i8,
        b'a'..=b'f' => (c - b'a' + 10) as i8,
        _ => -1,
    }
}

/// Decode a MIME encoded word starting at s[p] ("=?"). Returns the new
/// position after the word, or None if it is not a valid encoded word.
fn decode_encoded_word(
    s: &[u8],
    mut p: usize,
    mut e: usize,
    outcode: &Encoding,
    outbuf: &mut ConvBuf,
    state: &mut u32,
) -> Option<usize> {
    if e - p < 6 {
        return None;
    }
    p += 2;
    let charset = p;
    let charset_end = charset + s[charset..e].iter().position(|&c| c == b'?')?;
    let encoding = charset_end + 1;
    p = encoding + 1;
    if p >= e || s[p] != b'?' {
        return None;
    }
    p += 1;
    let incode = mbfl::name2encoding(&s[charset..charset_end])?;
    if let Some(m) = crate::strfuncs::memnstr(s, p, b"?=", e) {
        e = m;
    } else if p < e && s[e - 1] == b'?' {
        e -= 1;
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(e.saturating_sub(p));
    let enc_c = s[encoding];
    if enc_c == b'Q' || enc_c == b'q' {
        while p < e {
            let c = s[p];
            p += 1;
            if c == b'_' {
                bytes.push(b' ');
                continue;
            } else if c == b'=' && e - p >= 2 {
                let c2 = s[p];
                let c3 = s[p + 1];
                p += 2;
                if qprint_map(c2) >= 0 && qprint_map(c3) >= 0 {
                    bytes.push(((qprint_map(c2) as u8) << 4) | (qprint_map(c3) as u8 & 0xF));
                    continue;
                } else if c2 == b'\r' {
                    if c3 != b'\n' {
                        p -= 1;
                    }
                    continue;
                } else if c2 == b'\n' {
                    p -= 1;
                    continue;
                }
            }
            bytes.push(c);
        }
    } else if enc_c == b'B' || enc_c == b'b' {
        let mut bits = 0u32;
        let mut cache = 0u32;
        while p < e {
            let c = s[p];
            p += 1;
            if c == b'\r' || c == b'\n' || c == b' ' || c == b'\t' || c == b'=' {
                continue;
            }
            let d = decode_base64(c);
            if d == -1 {
                bytes.push(b'?');
                continue;
            }
            bits += 6;
            cache = (cache << 6) | (d as u32 & 0x3F);
            if bits == 24 {
                bytes.push(((cache >> 16) & 0xFF) as u8);
                bytes.push(((cache >> 8) & 0xFF) as u8);
                bytes.push((cache & 0xFF) as u8);
                bits = 0;
                cache = 0;
            }
        }
        if bits == 18 {
            bytes.push(((cache >> 10) & 0xFF) as u8);
            bytes.push(((cache >> 2) & 0xFF) as u8);
        } else if bits == 12 {
            bytes.push(((cache >> 4) & 0xFF) as u8);
        }
    } else {
        return None;
    }
    if !bytes.is_empty() {
        let mut w = Vec::new();
        if let Some(tw) = incode.to_wchar {
            tw(&bytes, &mut w, state);
        }
        (outcode.from_wchar.unwrap())(&w, outbuf, false);
    }
    Some(e + 2)
}

/// C: `mb_mime_header_decode`
pub fn mime_header_decode(s: &[u8], outcode: &Encoding) -> Vec<u8> {
    let e = s.len();
    let mut p = 0usize;
    let mut state = 0u32;
    let mut space_pending = false;
    let from_wchar = match outcode.from_wchar {
        Some(f) => f,
        None => return s.to_vec(),
    };
    let mut buf = ConvBuf::new(s.len(), b'?' as u32, MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR);
    let is_ws = |c: u8| c == b'\n' || c == b'\r' || c == b'\t' || c == b' ';
    while p < e {
        let c = s[p];
        if c == b'=' && p + 1 < e && s[p + 1] == b'?' && e - p >= 6 {
            if let Some(ie) = s[p + 2..e]
                .iter()
                .position(|&c| c == b'?')
                .map(|x| x + p + 2)
            {
                if e - ie >= 3 {
                    if let Some(np) = decode_encoded_word(s, p, e, outcode, &mut buf, &mut state) {
                        p = np.min(e);
                        if p < e && is_ws(s[p]) {
                            while p < e && is_ws(s[p]) {
                                p += 1;
                            }
                            space_pending = true;
                        } else {
                            space_pending = false;
                        }
                        continue;
                    }
                }
            }
        }
        if space_pending {
            from_wchar(&[b' ' as u32], &mut buf, false);
            space_pending = false;
        }
        if c != b'\n' && c != b'\r' {
            let mut end = p + 1;
            while end < e && s[end] != b'=' && s[end] != b'\n' && s[end] != b'\r' {
                end += 1;
            }
            let w: Vec<u32> = s[p..end]
                .iter()
                .map(|&c| {
                    if c < 0x80 {
                        c as u32
                    } else {
                        mbfl::MBFL_BAD_INPUT
                    }
                })
                .collect();
            from_wchar(&w, &mut buf, false);
            p = end;
        }
        if p < e && (s[p] == b'\n' || s[p] == b'\r') {
            p += 1;
            while p < e && is_ws(s[p]) {
                p += 1;
            }
            if p < e {
                from_wchar(&[b' ' as u32], &mut buf, false);
            }
        }
    }
    from_wchar(&[], &mut buf, true);
    buf.out
}

fn mb_decode_mimeheader(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_decode_mimeheader", args, 0, "string")?;
    state::sync(vm);
    let out = state::current_internal_encoding();
    Ok(pstring(mime_header_decode(&s, out)))
}
