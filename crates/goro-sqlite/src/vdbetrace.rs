//! Port of vdbetrace.c: the routine that renders the SQL of a statement
//! with its host parameters expanded (sqlite3_expanded_sql(), tracing).

#![allow(unused_variables)]

use crate::int::*;
use crate::printf::{
    PfArg, sqlite3_str_append, sqlite3_str_appendf, sqlite3_str_reset, sqlite3StrAccumFinish,
    sqlite3StrAccumInit,
};

/// zSql is a string of UTF-8 SQL text.  Return the number of bytes in this
/// text up to but excluding the first character in a host parameter.  If
/// the text contains no host parameters, return the total number of bytes
/// in the text (findNextHostParameter()).
fn findNextHostParameter(zSql: &[u8], pnToken: &mut i64) -> i64 {
    let mut tokenType = 0;
    let mut nTotal: i64 = 0;
    let mut z = zSql;

    *pnToken = 0;
    while !z.is_empty() && z[0] != 0 {
        let n = crate::tokenize::sqlite3GetToken(z, &mut tokenType);
        debug_assert!(n > 0 && tokenType != TK_ILLEGAL as i32);
        if tokenType == TK_VARIABLE as i32 {
            *pnToken = n;
            break;
        }
        nTotal += n;
        z = &z[(n as usize).min(z.len())..];
    }
    nTotal
}

/// C: `char * sqlite3VdbeExpandSql( Vdbe *p, const char *zRawSql )`
///
/// Returns a string holding a copy of zRawSql but with host parameters
/// expanded to their current bindings when sqlite3.nVdbeExec is 1.  Or, if
/// sqlite3.nVdbeExec is greater than 1, then the returned string holds a
/// copy of zRawSql with "-- " prepended to each line of text.
///
/// ALGORITHM:  Scan the input string looking for host parameters in any of
/// these forms:  ?, ?N, $A, @A, :A.  Take care to avoid text within string
/// literals, quoted identifier names, and comments.  For text forms, the
/// host parameter index is found by scanning the prepared statement for
/// the corresponding OP_Variable opcode.  Once the host parameter index is
/// known, locate the value in p->aVar[].  Then render the value as a
/// literal in place of the host parameter name.
///
/// Rust: zRawSql ends at the end of the slice (or at a NUL byte).
pub fn sqlite3VdbeExpandSql(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    zRawSql: Option<&[u8]>,
) -> Option<ZStr> {
    let mut idx: i32 = 0; /* Index of a host parameter */
    let mut nextIndex: i32 = 1; /* Index of next ? host parameter */
    let mut nToken: i64 = 0; /* Length of the parameter token */
    let mut out = StrAccum::default(); /* Accumulate the output here */

    let raw = zRawSql.unwrap_or(b"");
    let raw = &raw[..raw.iter().position(|&c| c == 0).unwrap_or(raw.len())];
    let mut z: &[u8] = raw;

    sqlite3StrAccumInit(&mut out, 0, db.aLimit[SQLITE_LIMIT_LENGTH as usize]);
    if db.nVdbeExec > 1 {
        while !z.is_empty() {
            let zStart = z;
            /* while( *(zRawSql++)!='\n' && *zRawSql ); */
            let mut k = 0usize;
            loop {
                let c = z[k];
                k += 1;
                if !(c != b'\n' && k < z.len()) {
                    break;
                }
            }
            sqlite3_str_append(&mut out, b"-- ", 3);
            debug_assert!(k > 0);
            sqlite3_str_append(&mut out, &zStart[..k], k as i32);
            z = &z[k..];
        }
    } else if p.nVar == 0 {
        sqlite3_str_append(&mut out, z, z.len() as i32);
    } else {
        while !z.is_empty() {
            let n = findNextHostParameter(z, &mut nToken);
            debug_assert!(n > 0);
            let n = (n as usize).min(z.len());
            sqlite3_str_append(&mut out, &z[..n], n as i32);
            z = &z[n..];
            debug_assert!(!z.is_empty() || nToken == 0);
            if nToken == 0 {
                break;
            }
            if z[0] == b'?' {
                if nToken > 1 {
                    debug_assert!(crate::global::sqlite3Isdigit(z[1]));
                    crate::util::sqlite3GetInt32(Some(&z[1..]), &mut idx);
                } else {
                    idx = nextIndex;
                }
            } else {
                debug_assert!(z[0] == b':' || z[0] == b'$' || z[0] == b'@' || z[0] == b'#');
                let nt = (nToken as usize).min(z.len());
                idx = crate::vdbeapi::sqlite3VdbeParameterIndex(p, Some(&z[..nt]), nt as i32);
                debug_assert!(idx > 0);
            }
            z = &z[(nToken as usize).min(z.len())..];
            nextIndex = (idx + 1).max(nextIndex);
            debug_assert!(idx > 0 && idx <= p.nVar as i32);
            let Some(pVar) = p.aVar.get((idx - 1).max(0) as usize) else {
                continue;
            };
            if pVar.flags & MEM_Null != 0 {
                sqlite3_str_append(&mut out, b"NULL", 4);
            } else if pVar.flags & (MEM_Int | MEM_IntReal) != 0 {
                sqlite3_str_appendf(&mut out, b"%lld", &[PfArg::Int(pVar.u.i)]);
            } else if pVar.flags & MEM_Real != 0 {
                sqlite3_str_appendf(&mut out, b"%!.15g", &[PfArg::Dbl(pVar.u.r)]);
            } else if pVar.flags & MEM_Str != 0 {
                let enc = db.ENC();
                let text: Vec<u8> = if enc != SQLITE_UTF8 {
                    let mut utf8 = Mem::default();
                    let zv = pVar.bytes().to_vec();
                    crate::vdbemem::sqlite3VdbeMemSetStr(
                        db,
                        &mut utf8,
                        Some(&zv),
                        zv.len() as i64,
                        enc,
                    );
                    if SQLITE_NOMEM
                        == crate::vdbemem::sqlite3VdbeChangeEncoding(
                            db,
                            &mut utf8,
                            SQLITE_UTF8 as i32,
                        )
                    {
                        out.accError = SQLITE_NOMEM as u8;
                        out.nAlloc = 0;
                    }
                    utf8.bytes().to_vec()
                } else {
                    pVar.bytes().to_vec()
                };
                let nOut = text.len() as i64;
                sqlite3_str_appendf(
                    &mut out,
                    b"'%.*q'",
                    &[PfArg::Int(nOut), PfArg::Str(Some(&text))],
                );
            } else if pVar.flags & MEM_Zero != 0 {
                sqlite3_str_appendf(
                    &mut out,
                    b"zeroblob(%d)",
                    &[PfArg::Int(pVar.u.nZero as i64)],
                );
            } else {
                debug_assert!(pVar.flags & MEM_Blob != 0);
                sqlite3_str_append(&mut out, b"x'", 2);
                let b = pVar.bytes().to_vec();
                for c in b {
                    sqlite3_str_appendf(&mut out, b"%02x", &[PfArg::Int((c & 0xff) as i64)]);
                }
                sqlite3_str_append(&mut out, b"'", 1);
            }
        }
    }
    if out.accError != 0 {
        sqlite3_str_reset(&mut out);
    }
    sqlite3StrAccumFinish(&mut out)
}
