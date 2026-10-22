//! Port of complete.c.
//!
//! An SQL tokenizer that is capable of detecting the end of an SQL
//! statement (sqlite3_complete() / sqlite3_incomplete()).

use crate::global::{sqlite3CtypeMap, sqlite3UpperToLower};
use crate::int::*;

/// IdChar(C), as in tokenize.c.
#[inline]
fn IdChar(c: u8) -> bool {
    (sqlite3CtypeMap[c as usize] & 0x46) != 0
}

/// `z[i]`, or 0 past the end of `z` (the C code reads the NUL terminator).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// sqlite3StrNICmp(z, kw, n)==0 for an ASCII keyword `kw` of length n
/// (same folding as sqlite3StrNICmp: sqlite3UpperToLower).
fn kw_eq(z: &[u8], kw: &[u8]) -> bool {
    kw.iter()
        .enumerate()
        .all(|(i, &k)| sqlite3UpperToLower[at(z, i) as usize] == sqlite3UpperToLower[k as usize])
}

/*
** Token types used by the sqlite3_complete() routine.  See the header
** comments on that procedure for additional information.
*/
const tkSEMI: u8 = 0;
const tkWS: u8 = 1;
const tkOTHER: u8 = 2;
const tkEXPLAIN: u8 = 3;
const tkCREATE: u8 = 4;
const tkTEMP: u8 = 5;
const tkTRIGGER: u8 = 6;
const tkEND: u8 = 7;

/* A complex statement machine used to detect the end of a CREATE TRIGGER
** statement.  This is the normal case.
*/
#[rustfmt::skip]
static trans: [[u8; 8]; 8] = [
                 /* Token:                                                */
 /* State:       **  SEMI  WS  OTHER  EXPLAIN  CREATE  TEMP  TRIGGER  END */
 /* 0 NOTHING: */ [    1,  0,     2,       3,      4,    2,       2,   2, ],
 /* 1   START: */ [    1,  1,     2,       3,      4,    2,       2,   2, ],
 /* 2  NORMAL: */ [    1,  2,     2,       2,      2,    2,       2,   2, ],
 /* 3 EXPLAIN: */ [    1,  3,     3,       2,      4,    2,       2,   2, ],
 /* 4  CREATE: */ [    1,  4,     2,       2,      2,    4,       5,   2, ],
 /* 5 TRIGGER: */ [    6,  5,     5,       5,      5,    5,       5,   5, ],
 /* 6    SEMI: */ [    6,  6,     5,       5,      5,    5,       5,   7, ],
 /* 7     END: */ [    1,  7,     5,       5,      5,    5,       5,   5, ],
];

/* Mapping state number to yy value for the return */
#[rustfmt::skip]
static statemap: [u8; 8] = [
 /* 0 NOTHING */ 0,
 /* 1 START   */ 0,
 /* 2 NORMAL  */ 1,
 /* 3 EXPLAIN */ 1,
 /* 4 CREATE  */ 1,
 /* 5 TRIGGER */ 3,
 /* 6 SEMI    */ 2,
 /* 7 END     */ 1,
];

/// Return zero if the given SQL string is complete - if all comments,
/// string and blob literals, and quoted identifiers have been closed and
/// if the entire string ends with ";" and possibly with ";END;" if the
/// string is a CREATE TRIGGER statement.  A non-zero return indicates
/// that the string is empty or incomplete.  Bits of the return value
/// indicate what is missing and is needed to close out the statement
/// (see complete.c for the layout: R = 0xwwwwwwww00xxyyzz).
///
/// C: `sqlite3_int64 sqlite3_incomplete(const char *zSql)`
pub fn sqlite3_incomplete(zSql: Option<&[u8]>) -> i64 {
    let mut state: u8 = 0; /* Current state, using numbers defined in header comment */
    let mut token: u8; /* Value of the next token */
    let mut pending: u8 = 0; /* unmatched structure character */
    let mut nParen: i32 = 0; /* Nested parentheses */

    let z = match zSql {
        Some(z) => z,
        None => return SQLITE_MISUSE as i64,
    };
    let mut p = 0usize;

    'outer: while at(z, p) != 0 {
        match at(z, p) {
            b';' => {
                /* A semicolon */
                token = tkSEMI;
            }
            b' ' | b'\r' | b'\t' | b'\n' | 0x0c => {
                /* White space is ignored */
                token = tkWS;
            }
            b'/' => {
                /* C-style comments */
                if at(z, p + 1) != b'*' {
                    token = tkOTHER;
                } else {
                    p += 2;
                    while at(z, p) != 0 && (at(z, p) != b'*' || at(z, p + 1) != b'/') {
                        p += 1;
                    }
                    if at(z, p) == 0 {
                        pending = b'/';
                        break 'outer;
                    }
                    p += 1;
                    token = tkWS;
                }
            }
            b'-' => {
                /* SQL-style comments from "--" to end of line */
                if at(z, p + 1) != b'-' {
                    token = tkOTHER;
                } else {
                    while at(z, p) != 0 && at(z, p) != b'\n' {
                        p += 1;
                    }
                    if at(z, p) == 0 {
                        pending = b'-';
                        break 'outer;
                    }
                    token = tkWS;
                }
            }
            b'[' => {
                /* Microsoft-style identifiers in [...] */
                p += 1;
                while at(z, p) != 0 && at(z, p) != b']' {
                    p += 1;
                }
                if at(z, p) == 0 {
                    pending = b']';
                    break 'outer;
                }
                token = tkOTHER;
            }
            b'`' | b'"' | b'\'' => {
                /* Grave-accent quoted symbols used by MySQL,
                 ** single- and double-quoted strings */
                let c = at(z, p);
                p += 1;
                while at(z, p) != 0 && at(z, p) != c {
                    p += 1;
                }
                if at(z, p) == 0 {
                    pending = c;
                    break 'outer;
                }
                token = tkOTHER;
            }
            b'(' => {
                nParen += 1;
                token = tkOTHER;
            }
            b')' => {
                nParen -= 1;
                token = tkOTHER;
            }
            c0 => {
                if IdChar(c0) {
                    /* Keywords and unquoted identifiers */
                    let mut nId = 1usize;
                    while IdChar(at(z, p + nId)) {
                        nId += 1;
                    }
                    let w = &z[p..];
                    token = match c0 {
                        b'c' | b'C' => {
                            if nId == 6 && kw_eq(w, b"create") {
                                tkCREATE
                            } else {
                                tkOTHER
                            }
                        }
                        b't' | b'T' => {
                            if nId == 7 && kw_eq(w, b"trigger") {
                                tkTRIGGER
                            } else if nId == 4 && kw_eq(w, b"temp") {
                                tkTEMP
                            } else if nId == 9 && kw_eq(w, b"temporary") {
                                tkTEMP
                            } else {
                                tkOTHER
                            }
                        }
                        b'e' | b'E' => {
                            if nId == 3 && kw_eq(w, b"end") {
                                tkEND
                            } else if nId == 7 && kw_eq(w, b"explain") {
                                tkEXPLAIN
                            } else {
                                tkOTHER
                            }
                        }
                        _ => tkOTHER,
                    };
                    p += nId - 1;
                } else {
                    /* Operators and special symbols */
                    token = tkOTHER;
                }
            }
        }
        state = trans[state as usize][token as usize];
        p += 1;
    }
    /* incomplete_finish: */
    if state == 0 && pending == 0 {
        return SQLITE_EMPTY as i64;
    }
    if state == 1 {
        nParen = 0;
    }
    ((((nParen as i64) as u64) << 32)
        | ((pending as u64) << 16)
        | ((statemap[state as usize] as u64) << 8)
        | ((state > 1 || pending != 0) as u64)) as i64
}

/// C: `int sqlite3_complete(const char *zSql)`
pub fn sqlite3_complete(zSql: Option<&[u8]>) -> i32 {
    (sqlite3_incomplete(zSql) == 0) as i32
}

/// This routine is the same as the sqlite3_complete() routine described
/// above, except that the parameter is required to be UTF-16 encoded
/// (native byte order, NUL terminated or ending at the end of the slice),
/// not UTF-8.
///
/// C: `int sqlite3_complete16(const void *zSql)`
pub fn sqlite3_complete16(zSql: &[u8]) -> i32 {
    let units = zSql
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0);
    let s: String = std::char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    let rc = (sqlite3_incomplete(Some(s.as_bytes())) == 0) as i32;
    rc & 0xff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> i32 {
        sqlite3_complete(Some(s.as_bytes()))
    }

    #[test]
    fn complete() {
        assert_eq!(c("select 1;"), 1);
        assert_eq!(c("select 1"), 0);
        assert_eq!(c(""), 0);
        assert_eq!(c("   "), 0);
        assert_eq!(c(";"), 1);
        assert_eq!(c("select 'a;"), 0);
        assert_eq!(c("select 'a;';"), 1);
        assert_eq!(c("select 1; -- x"), 0);
        assert_eq!(c("select 1; -- x\n"), 1);
        assert_eq!(c("select 1 /* ; */;"), 1);
        assert_eq!(c("CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1;"), 0);
        assert_eq!(
            c("CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1; END;"),
            1
        );
        assert_eq!(
            c("create temp trigger t after insert on x begin select 1; end ;"),
            1
        );
        assert_eq!(
            c("explain create trigger t after insert on x begin select 1; end;"),
            1
        );
        assert_eq!(c("create table trigger(x);"), 1);
        assert_eq!(sqlite3_incomplete(Some(b"")), SQLITE_EMPTY as i64);
        assert_eq!(sqlite3_incomplete(Some(b"select (1")), (1i64 << 32) | 0x101);
        assert_eq!(
            sqlite3_incomplete(Some(b"select 1)")),
            ((-1i64 as u64) << 32 | 0x101) as i64
        );
        assert_eq!(
            sqlite3_incomplete(Some(b"select 'x")),
            ((b'\'' as i64) << 16) | 0x101
        );
        assert_eq!(sqlite3_incomplete(None), SQLITE_MISUSE as i64);
        let u: Vec<u8> = "select 1;"
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .collect();
        assert_eq!(sqlite3_complete16(&u), 1);
    }
}
