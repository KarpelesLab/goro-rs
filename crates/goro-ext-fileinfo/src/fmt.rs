//! The vspprintf() (main/spprintf.c) subset file_printf() relies on.

#[derive(Clone, Debug)]
pub enum Arg<'a> {
    /// an integer argument (sign- or zero-extended by its C type)
    I(i64),
    F(f64),
    S(&'a [u8]),
}

fn arg_int(a: Option<&Arg>) -> i64 {
    match a {
        Some(Arg::I(n)) => *n,
        Some(Arg::F(f)) => *f as i64,
        _ => 0,
    }
}

fn arg_f(a: Option<&Arg>) -> f64 {
    match a {
        Some(Arg::F(f)) => *f,
        Some(Arg::I(n)) => *n as f64,
        _ => 0.0,
    }
}

/// zend_dtoa(value, 2, ndigit): significant digits (trailing zeros
/// removed) and the decimal point position.
fn dtoa_digits(v: f64, ndigit: usize) -> (Vec<u8>, i32) {
    if v == 0.0 {
        return (b"0".to_vec(), 1);
    }
    let s = format!("{:.*e}", ndigit.max(1) - 1, v.abs());
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let mut digits: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, exp + 1)
}

/// php_gcvt()
pub fn php_gcvt(value: f64, ndigit: usize, exp_char: u8) -> Vec<u8> {
    let (digits, decpt) = dtoa_digits(value, ndigit);
    let mut dst = Vec::new();
    if value < 0.0 {
        dst.push(b'-');
    }
    if if decpt < 0 {
        decpt < -3
    } else {
        decpt > ndigit as i32
    } {
        let mut decpt = decpt - 1;
        let sign = if decpt < 0 {
            decpt = -decpt;
            true
        } else {
            false
        };
        dst.push(digits[0]);
        dst.push(b'.');
        if digits.len() == 1 {
            dst.push(b'0');
        } else {
            dst.extend_from_slice(&digits[1..]);
        }
        dst.push(exp_char);
        dst.push(if sign { b'-' } else { b'+' });
        dst.extend_from_slice(decpt.to_string().as_bytes());
    } else if decpt < 0 {
        dst.extend_from_slice(b"0.");
        let mut d = decpt;
        loop {
            dst.push(b'0');
            d += 1;
            if d >= 0 {
                break;
            }
        }
        dst.extend_from_slice(&digits);
    } else {
        let mut i = 0usize;
        while (i as i32) < decpt {
            dst.push(*digits.get(i).unwrap_or(&b'0'));
            i += 1;
        }
        if (decpt as usize) < digits.len() {
            if decpt == 0 {
                dst.push(b'0');
            }
            dst.push(b'.');
            dst.extend_from_slice(&digits[decpt as usize..]);
        }
    }
    dst
}

/// vspprintf()
pub fn format(fmt: &[u8], args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ai = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let mut left = false;
        let mut plus = false;
        let mut space = false;
        let mut alt = false;
        let mut zero = false;
        while i < fmt.len() {
            match fmt[i] {
                b'-' => left = true,
                b'+' => plus = true,
                b' ' => space = true,
                b'#' => alt = true,
                b'0' => zero = true,
                b'\'' => {}
                _ => break,
            }
            i += 1;
        }
        let mut width: Option<usize> = None;
        if i < fmt.len() && fmt[i] == b'*' {
            let w = arg_int(args.get(ai));
            ai += 1;
            if w < 0 {
                left = true;
            }
            width = Some(w.unsigned_abs() as usize);
            i += 1;
        } else {
            let mut w = 0usize;
            let mut any = false;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                w = w * 10 + (fmt[i] - b'0') as usize;
                any = true;
                i += 1;
            }
            if any {
                width = Some(w);
            }
        }
        let mut prec: Option<usize> = None;
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                let p = arg_int(args.get(ai));
                ai += 1;
                prec = Some(p.max(0) as usize);
                i += 1;
            } else {
                let mut p = 0usize;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    p = p * 10 + (fmt[i] - b'0') as usize;
                    i += 1;
                }
                prec = Some(p);
            }
        }
        // length modifiers: 'h' is ignored (LM_STD)
        let mut long = false;
        while i < fmt.len() {
            match fmt[i] {
                b'l' | b'q' | b'j' | b'z' | b't' | b'L' => long = true,
                b'h' => {}
                b'I' => {
                    long = true;
                    if fmt[i..].starts_with(b"I64") || fmt[i..].starts_with(b"I32") {
                        long = fmt[i + 1] == b'6';
                        i += 2;
                    }
                }
                _ => break,
            }
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            break;
        };
        i += 1;
        let mut prefix: Vec<u8> = Vec::new();
        let body: Vec<u8>;
        let mut numeric = false;
        match conv {
            b'%' => {
                out.push(b'%');
                continue;
            }
            b'd' | b'i' => {
                numeric = true;
                let n = arg_int(args.get(ai));
                ai += 1;
                let n = if long { n } else { n as i32 as i64 };
                if n < 0 {
                    prefix.push(b'-');
                } else if plus {
                    prefix.push(b'+');
                } else if space {
                    prefix.push(b' ');
                }
                let mut d = n.unsigned_abs().to_string().into_bytes();
                if let Some(p) = prec {
                    while d.len() < p {
                        d.insert(0, b'0');
                    }
                    if p == 0 && n == 0 {
                        d.clear();
                    }
                }
                body = d;
            }
            b'u' | b'o' | b'x' | b'X' => {
                numeric = true;
                let n = arg_int(args.get(ai));
                ai += 1;
                let n = if long { n as u64 } else { n as u32 as u64 };
                let mut d = match conv {
                    b'u' => n.to_string(),
                    b'o' => format!("{:o}", n),
                    b'x' => format!("{:x}", n),
                    _ => format!("{:X}", n),
                }
                .into_bytes();
                if let Some(p) = prec {
                    while d.len() < p {
                        d.insert(0, b'0');
                    }
                    if p == 0 && n == 0 {
                        d.clear();
                    }
                }
                if alt && n != 0 {
                    match conv {
                        b'o' => {
                            if d.first() != Some(&b'0') {
                                d.insert(0, b'0');
                            }
                        }
                        b'x' => prefix.extend_from_slice(b"0x"),
                        b'X' => prefix.extend_from_slice(b"0X"),
                        _ => {}
                    }
                }
                body = d;
            }
            b'c' => {
                let n = arg_int(args.get(ai));
                ai += 1;
                body = vec![n as u8];
            }
            b's' => {
                let s: &[u8] = match args.get(ai) {
                    Some(Arg::S(s)) => s,
                    _ => b"(null)",
                };
                ai += 1;
                let s = super::magic::cstr(s);
                body = match prec {
                    Some(p) if p < s.len() => s[..p].to_vec(),
                    _ => s.to_vec(),
                };
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'H' | b'k' => {
                numeric = true;
                let v = arg_f(args.get(ai));
                ai += 1;
                if v.is_nan() {
                    body = b"NAN".to_vec();
                } else if v.is_infinite() {
                    body = if v < 0.0 {
                        b"-INF".to_vec()
                    } else {
                        b"INF".to_vec()
                    };
                } else {
                    if v >= 0.0 {
                        if plus {
                            prefix.push(b'+');
                        } else if space {
                            prefix.push(b' ');
                        }
                    }
                    body = match conv {
                        b'g' | b'G' | b'H' | b'k' => {
                            let mut p = prec.unwrap_or(0);
                            if p == 0 {
                                p = 6;
                            }
                            if p > 40 {
                                p = 40;
                            }
                            let mut s = php_gcvt(
                                v,
                                p,
                                if conv == b'G' || conv == b'H' {
                                    b'E'
                                } else {
                                    b'e'
                                },
                            );
                            if alt && !s.contains(&b'.') {
                                s.push(b'.');
                            }
                            s
                        }
                        b'e' | b'E' => {
                            let p = prec.unwrap_or(6);
                            let s = format!("{:.*e}", p, v);
                            let (m, e) = s.split_once('e').unwrap();
                            let e: i32 = e.parse().unwrap();
                            let mut r = m.to_string();
                            r.push(if conv == b'E' { 'E' } else { 'e' });
                            r.push(if e < 0 { '-' } else { '+' });
                            r.push_str(&e.unsigned_abs().to_string());
                            r.into_bytes()
                        }
                        _ => format!("{:.*}", prec.unwrap_or(6), v).into_bytes(),
                    };
                }
            }
            _ => {
                // unknown conversion: printed as is
                out.push(b'%');
                out.push(conv);
                continue;
            }
        }
        let len = prefix.len() + body.len();
        let w = width.unwrap_or(0);
        if len >= w {
            out.extend_from_slice(&prefix);
            out.extend_from_slice(&body);
        } else if left {
            out.extend_from_slice(&prefix);
            out.extend_from_slice(&body);
            out.extend(std::iter::repeat_n(b' ', w - len));
        } else if zero && numeric && !(prec.is_some() && conv != b'f' && conv != b'e') {
            out.extend_from_slice(&prefix);
            out.extend(std::iter::repeat_n(b'0', w - len));
            out.extend_from_slice(&body);
        } else {
            out.extend(std::iter::repeat_n(b' ', w - len));
            out.extend_from_slice(&prefix);
            out.extend_from_slice(&body);
        }
    }
    out
}
