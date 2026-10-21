//! Port of timelib parse_iso_intervals.re (timelib_strtointerval)

use super::parse_date::{Ptr, get_nr};
use super::regex::{Scanner as ReScanner, parse_definitions, parse_expr};
use super::*;
use std::cell::RefCell;

const DEFINITIONS: &str = r#"
any = [\000-\377];
number = [0-9]+;
hour24lz = [01][0-9] | "2"[0-4];
minutelz = [0-5][0-9];
monthlz = "0" [1-9] | "1" [0-2];
monthlzz = "0" [0-9] | "1" [0-2];
daylz   = "0" [1-9] | [1-2][0-9] | "3" [01];
daylzz  = "0" [0-9] | [1-2][0-9] | "3" [01];
secondlz = minutelz;
year4 = [0-9]{4};
weekofyear = "0"[1-9] | [1-4][0-9] | "5"[0-3];
space = [ \t]+;
datetimebasic  = year4 monthlz daylz "T" hour24lz minutelz secondlz "Z";
datetimeextended  = year4 "-" monthlz "-" daylz "T" hour24lz ':' minutelz ':' secondlz "Z";
period   = "P" (number "Y")? (number "M")? (number "W")? (number "D")? ("T" (number "H")? (number "M")? (number "S")?)?;
combinedrep = "P" year4 "-" monthlzz "-" daylzz "T" hour24lz ':' minutelz ':' secondlz;
recurrences = "R" number;
"#;

const RULES: &[&str] = &[
    "recurrences",
    "datetimebasic| datetimeextended",
    "period",
    "combinedrep",
    "[ .,\\t/]",
    "\"\\000\"|\"\\n\"",
    "any",
];

thread_local! {
    static SCANNER: RefCell<Option<ReScanner>> = const { RefCell::new(None) };
}

fn with_scanner<R>(f: impl FnOnce(&mut ReScanner) -> R) -> R {
    SCANNER.with(|s| {
        let mut b = s.borrow_mut();
        if b.is_none() {
            let defs = parse_definitions(DEFINITIONS);
            let rules: Vec<_> = RULES.iter().map(|e| parse_expr(e, &defs)).collect();
            *b = Some(ReScanner::new(&rules));
        }
        f(b.as_mut().unwrap())
    })
}

fn get_unsigned_nr(ptr: &mut Ptr, max_length: usize) -> i64 {
    let mut dir: i64 = 1;
    while !ptr.c().is_ascii_digit() && ptr.c() != b'+' && ptr.c() != b'-' {
        if ptr.c() == 0 {
            return UNSET;
        }
        ptr.adv();
    }
    while ptr.c() == b'+' || ptr.c() == b'-' {
        if ptr.c() == b'-' {
            dir = -dir;
        }
        ptr.adv();
    }
    dir.wrapping_mul(get_nr(ptr, max_length))
}

pub struct IsoInterval {
    pub begin: Option<Time>,
    pub end: Option<Time>,
    pub period: Option<RelTime>,
    pub recurrences: Option<i64>,
    pub errors: ErrorContainer,
}

fn new_time() -> Time {
    let mut t = Time::new();
    t.y = UNSET;
    t.d = UNSET;
    t.m = UNSET;
    t.h = UNSET;
    t.i = UNSET;
    t.s = UNSET;
    t.us = 0;
    t.z = 0;
    t.dst = 0;
    t.is_localtime = 0;
    t.zone_type = ZONETYPE_OFFSET;
    t
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

pub fn strtointerval(input: &[u8]) -> IsoInterval {
    let mut errors = ErrorContainer::default();
    let mut res = IsoInterval {
        begin: None,
        end: None,
        period: None,
        recurrences: None,
        errors: ErrorContainer::default(),
    };
    let len = input.len();
    let mut s0 = 0usize;
    let mut e: isize = len as isize - 1;
    if len > 0 {
        while is_c_space(input[s0]) && (s0 as isize) < e {
            s0 += 1;
        }
        while e >= 0 && is_c_space(input[e as usize]) && e > s0 as isize {
            e -= 1;
        }
    }
    let push_err = |errors: &mut ErrorContainer, pos: usize, ch: u8, msg: &str| {
        errors.error_messages.push(ErrorMessage {
            error_code: 0,
            position: pos as i32,
            character: ch,
            message: msg.to_string(),
        });
    };
    if e - (s0 as isize) < 0 {
        push_err(&mut errors, 0, 0, "Empty string");
        res.errors = errors;
        return res;
    }
    let slice = &input[s0..(e + 1) as usize];
    let mut buf = slice.to_vec();
    buf.extend(std::iter::repeat_n(0u8, 20));
    let input_len = slice.len();

    let mut begin = new_time();
    let mut end = new_time();
    let mut period = RelTime {
        days: UNSET,
        ..Default::default()
    };
    let mut recurrences: i64 = 1;
    let (
        mut have_period,
        mut have_recurrences,
        mut have_date,
        mut have_begin_date,
        mut have_end_date,
    ) = (false, false, false, false, false);

    let mut cur = 0usize;
    while cur <= input_len {
        let tok = cur;
        let (rule, endp) =
            with_scanner(|sc| sc.longest_match(&buf, cur)).unwrap_or((RULES.len() - 1, cur + 1));
        cur = endp;
        let token: Vec<u8> = buf[tok..cur].to_vec();
        let z = token.iter().position(|&c| c == 0).unwrap_or(token.len());
        let mut ptr = Ptr::new(&token[..z]);
        let tokch = buf[tok];
        match rule {
            0 => {
                ptr.adv();
                recurrences = get_unsigned_nr(&mut ptr, 9);
                have_recurrences = true;
            }
            1 => {
                let current = if have_date || have_period {
                    have_end_date = true;
                    &mut end
                } else {
                    have_begin_date = true;
                    &mut begin
                };
                current.y = get_nr(&mut ptr, 4);
                current.m = get_nr(&mut ptr, 2);
                current.d = get_nr(&mut ptr, 2);
                current.h = get_nr(&mut ptr, 2);
                current.i = get_nr(&mut ptr, 2);
                current.s = get_nr(&mut ptr, 2);
                have_date = true;
            }
            2 => {
                let mut in_time = false;
                ptr.adv();
                loop {
                    if ptr.c() == b'T' {
                        in_time = true;
                        ptr.adv();
                    }
                    if ptr.c() == 0 {
                        push_err(&mut errors, tok, tokch, "Missing expected time part");
                        break;
                    }
                    let nr = get_unsigned_nr(&mut ptr, 12);
                    match ptr.c() {
                        b'Y' => period.y = nr,
                        b'W' => period.d += nr * 7,
                        b'D' => period.d += nr,
                        b'H' => period.h = nr,
                        b'S' => period.s = nr,
                        b'M' => {
                            if in_time {
                                period.i = nr;
                            } else {
                                period.m = nr;
                            }
                        }
                        _ => push_err(&mut errors, tok, tokch, "Undefined period specifier"),
                    }
                    ptr.adv();
                    if errors.error_count() != 0 || ptr.c() == 0 {
                        break;
                    }
                }
                have_period = true;
            }
            3 => {
                period.y = get_unsigned_nr(&mut ptr, 4);
                ptr.adv();
                period.m = get_unsigned_nr(&mut ptr, 2);
                ptr.adv();
                period.d = get_unsigned_nr(&mut ptr, 2);
                ptr.adv();
                period.h = get_unsigned_nr(&mut ptr, 2);
                ptr.adv();
                period.i = get_unsigned_nr(&mut ptr, 2);
                ptr.adv();
                period.s = get_unsigned_nr(&mut ptr, 2);
                have_period = true;
            }
            4 | 5 => {}
            _ => push_err(&mut errors, tok, tokch, "Unexpected character"),
        }
    }
    res.errors = errors;
    if have_begin_date {
        res.begin = Some(begin);
    }
    if have_end_date {
        res.end = Some(end);
    }
    if have_period {
        res.period = Some(period);
    }
    if have_recurrences {
        res.recurrences = Some(recurrences);
    }
    res
}
