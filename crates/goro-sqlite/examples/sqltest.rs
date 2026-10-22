//! Minimal SQL runner used for differential testing against the C library.
//!
//! usage: sqltest DBFILE < script.sql
//!
//! Every statement of the script is prepared and stepped in turn; rows are
//! printed as `col|col|...` (NULL as an empty field, blobs as X'..' hex),
//! errors as `Error: <code> <message>`.  A matching C program built from
//! the amalgamation produces the same format.
//!
//! Optional environment variables (same in the C twin):
//! * `SQLTEST_SEED=n`: seed the PRNG (SQLITE_TESTCTRL_PRNG_SEED) so that
//!   random() & co are reproducible;
//! * `SQLTEST_NAMES=1`: before stepping a statement that returns columns,
//!   print `#name(decltype,db,table,origin)|...`;
//! * `SQLTEST_CHANGES=1`: after each statement print
//!   `#changes <changes> <total_changes> <autocommit>`.
//!
//! Meta commands (exercise the C API): a line starting with `.` and a
//! lowercase letter, where a statement would start, is a command:
//! * `.bind IDX TYPE [VALUE]`: queue a binding (IDX = number or parameter
//!   name; TYPE = int, double, text, blob (hex), null, zeroblob) for the
//!   next statement or `.rerun`;
//! * `.rerun` (reset the last statement, bind, step again), `.reset`,
//!   `.clear` (clear_bindings), `.info` (sql, expanded sql, flags,
//!   parameters), `.stmtstatus` (sqlite3_stmt_status() counters), `.err`
//!   (error codes and message), `.typed 0|1|2` (rows
//!   as `T<bytes>:value`; 2 also shows column_int64/column_double);
//! * `.func NAME NARG [FLAGS]` (FLAGS: d deterministic, D directonly,
//!   i innocuous, s subtype), `.agg NAME`, `.coll NAME`: register test
//!   functions and a reverse-binary collation;
//! * `.blob DB TABLE COL ROWID n | r OFF N | w OFF HEX`: incremental blob I/O;
//! * `.backup FILE` / `.restore FILE`: backup API to/from another file;
//! * `.meta DB TABLE COL`, `.limit ID VAL`, `.exec SQL`, `.complete SQL`,
//!   `.status`;
//! * `.conn2 FILE` (open a second connection), `.2 SQL` (run SQL on it,
//!   rows prefixed with `2: `), `.busy MS` (busy timeout), `.sleep MS`;
//! * `.auth on|off|deny CODE|ignore CODE`: authorizer printing its calls;
//! * `.hooks on|off`: update/commit/rollback hooks printing their calls;
//! * `.prep N SQL` (named statement N in 0..7, kept open), `.step N [K]`,
//!   `.resetn N`, `.final N`: interleaved stepping of several statements.

#![allow(non_snake_case)]

use goro_sqlite::int::*;
use goro_sqlite::mainapi::*;
use goro_sqlite::prepare::sqlite3_prepare_v2;
use goro_sqlite::vdbeapi::*;
use std::io::{Read, Write};
use std::rc::Rc;

struct Bind {
    idx: Vec<u8>,
    ty: String,
    val: Option<Vec<u8>>,
}

struct St {
    named: [Option<StmtId>; 8],
    db2: Option<Box<Sqlite3>>,
    out: std::io::StdoutLock<'static>,
    binds: Vec<Bind>,
    typed: i32,
    changes: bool,
}

fn w(st: &mut St, b: &[u8]) {
    st.out.write_all(b).unwrap();
}

fn errmsg(db: &mut Sqlite3) -> Vec<u8> {
    sqlite3_errmsg(db).unwrap_or_default()
}

fn print_row(db: &mut Sqlite3, id: StmtId, st: &mut St) {
    let n = sqlite3_column_count(db, id);
    let mut line = Vec::new();
    for i in 0..n {
        if i > 0 {
            line.push(b'|');
        }
        let mut t = sqlite3_column_type(db, id, i);
        if st.typed >= 2 {
            let iv = sqlite3_column_int64(db, id, i);
            let dv = sqlite3_column_double(db, id, i);
            let zd = goro_sqlite::printf::sqlite3_mprintf(
                b"%!.17g",
                &[goro_sqlite::printf::PfArg::Dbl(dv)],
            );
            let c = b"?irtbn"[t as usize] as char;
            line.extend_from_slice(format!("{}<{},", c, iv).as_bytes());
            line.extend_from_slice(&zd);
            line.push(b'>');
            t = sqlite3_column_type(db, id, i);
        }
        if st.typed > 0 {
            let c = b"?irtbn"[t as usize] as char;
            let nb = sqlite3_column_bytes(db, id, i);
            line.extend_from_slice(format!("{}{}:", c, nb).as_bytes());
        }
        match t {
            SQLITE_NULL => {}
            SQLITE_BLOB => {
                let b = sqlite3_column_blob(db, id, i).unwrap_or_default();
                line.extend_from_slice(b"X'");
                for c in b {
                    line.extend_from_slice(format!("{:02X}", c).as_bytes());
                }
                line.push(b'\'');
            }
            _ => {
                let t = sqlite3_column_text(db, id, i).unwrap_or_default();
                line.extend_from_slice(&t);
            }
        }
    }
    line.push(b'\n');
    w(st, &line);
}

fn run_stmt(db: &mut Sqlite3, id: StmtId, st: &mut St) {
    loop {
        let rc = sqlite3_step(db, id);
        if rc == SQLITE_ROW {
            print_row(db, id, st);
        } else {
            if rc != SQLITE_DONE {
                let msg = errmsg(db);
                w(st, format!("Error: {} ", rc).as_bytes());
                w(st, &msg);
                w(st, b"\n");
            }
            break;
        }
    }
    if st.changes {
        let c = sqlite3_changes(db);
        let t = sqlite3_total_changes(db);
        let a = sqlite3_get_autocommit(db);
        w(st, format!("#changes {} {} {}\n", c, t, a).as_bytes());
    }
}

fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn unhex(z: &[u8]) -> Vec<u8> {
    z.chunks_exact(2)
        .map(|p| hexval(p[0]) * 16 + hexval(p[1]))
        .collect()
}

/// C atoi()/strtoll() on a word.
fn atoi(z: &[u8]) -> i64 {
    let s = String::from_utf8_lossy(z);
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + 1;
        } else {
            break;
        }
    }
    s[..end].parse().unwrap_or(0)
}

/// C strtod() on a word (the runner only needs plain numbers).
fn atof(z: &[u8]) -> f64 {
    String::from_utf8_lossy(z).trim().parse().unwrap_or(0.0)
}

fn apply_binds(db: &mut Sqlite3, id: StmtId, st: &mut St) {
    let binds = std::mem::take(&mut st.binds);
    for b in binds {
        let i = if b.idx.first().is_some_and(|c| c.is_ascii_digit()) {
            atoi(&b.idx) as i32
        } else {
            sqlite3_bind_parameter_index(db, id, Some(&b.idx))
        };
        let v = b.val.clone().unwrap_or_default();
        let rc = match b.ty.as_str() {
            "int" => sqlite3_bind_int64(db, id, i, atoi(&v)),
            "double" => sqlite3_bind_double(db, id, i, atof(&v)),
            "text" => sqlite3_bind_text(db, id, i, Some(&v), -1),
            "blob" => {
                let x = unhex(&v);
                sqlite3_bind_blob(db, id, i, &x, x.len() as i32)
            }
            "null" => sqlite3_bind_null(db, id, i),
            "zeroblob" => sqlite3_bind_zeroblob(db, id, i, atoi(&v) as i32),
            _ => 0,
        };
        if rc != 0 {
            let s = format!("bind {} rc={}\n", String::from_utf8_lossy(&b.idx), rc);
            w(st, s.as_bytes());
        }
    }
}

/// Text of a value as C "%s" prints it (up to the first NUL).
fn cstr(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

fn append_value(s: &mut Vec<u8>, v: &mut Mem) {
    const TYPES: [&str; 6] = ["?", "int", "real", "text", "blob", "null"];
    let t = sqlite3_value_type(v);
    s.extend_from_slice(TYPES[t as usize].as_bytes());
    s.push(b':');
    if t == SQLITE_BLOB {
        let b = sqlite3_value_blob(v)
            .map(|b| b.to_vec())
            .unwrap_or_default();
        for c in b {
            s.extend_from_slice(format!("{:02x}", c).as_bytes());
        }
    } else if t != SQLITE_NULL {
        let z = sqlite3_value_text(v)
            .map(|z| z.to_vec())
            .unwrap_or_default();
        s.extend_from_slice(cstr(&z));
    }
}

#[derive(Default)]
struct AggCtx {
    s: Option<Vec<u8>>,
    n: i32,
}

/// Split into at most n words; the last one takes the rest of the line.
fn words(z: &[u8], n: usize) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < z.len() && v.len() < n {
        while i < z.len() && (z[i] == b' ' || z[i] == b'\t') {
            i += 1;
        }
        if i >= z.len() {
            break;
        }
        if v.len() == n - 1 {
            v.push(z[i..].to_vec());
            break;
        }
        let s = i;
        while i < z.len() && z[i] != b' ' && z[i] != b'\t' {
            i += 1;
        }
        v.push(z[s..i].to_vec());
    }
    v
}

fn s(z: &[u8]) -> String {
    String::from_utf8_lossy(z).into_owned()
}

fn backup_file(db: &mut Sqlite3, zFile: &[u8], toFile: bool, st: &mut St) {
    use goro_sqlite::backup::*;
    let mut pdb: Option<Box<Sqlite3>> = None;
    let rc = sqlite3_open(Some(zFile), &mut pdb);
    let Some(mut pFile) = pdb else { return };
    if rc == SQLITE_OK {
        let (pTo, pFrom): (&mut Sqlite3, &mut Sqlite3) = if toFile {
            (&mut pFile, db)
        } else {
            (db, &mut pFile)
        };
        let p = sqlite3_backup_init(pTo, Some(b"main"), Some(&mut *pFrom), Some(b"main"));
        match p {
            Some(p) => {
                let mut rc = sqlite3_backup_step(pTo, pFrom, &p, 5);
                let m = format!(
                    "backup step rc={} remaining={} pagecount={}\n",
                    rc,
                    sqlite3_backup_remaining(&p),
                    sqlite3_backup_pagecount(&p)
                );
                w(st, m.as_bytes());
                while rc == SQLITE_OK {
                    rc = sqlite3_backup_step(pTo, pFrom, &p, 5);
                }
                let m = format!(
                    "backup step rc={} remaining={} pagecount={}\n",
                    rc,
                    sqlite3_backup_remaining(&p),
                    sqlite3_backup_pagecount(&p)
                );
                w(st, m.as_bytes());
                let rc = sqlite3_backup_finish(pTo, pFrom, p);
                w(st, format!("backup finish rc={}\n", rc).as_bytes());
            }
            None => {
                let c = sqlite3_errcode(pTo);
                let m = errmsg(pTo);
                w(st, format!("backup init failed: {} ", c).as_bytes());
                w(st, &m);
                w(st, b"\n");
            }
        }
    }
    sqlite3_close(&mut pFile);
}

fn run_on2(st: &mut St, z: &[u8]) {
    let Some(mut db2) = st.db2.take() else { return };
    let mut off = 0usize;
    while off < z.len() {
        let mut stmt: Option<StmtId> = None;
        let mut tail = 0usize;
        let rc = sqlite3_prepare_v2(&mut db2, Some(&z[off..]), -1, &mut stmt, &mut tail);
        if rc != SQLITE_OK {
            let m = errmsg(&mut db2);
            w(st, format!("2: Error: {} ", rc).as_bytes());
            w(st, &m);
            w(st, b"\n");
        }
        if tail == 0 {
            break;
        }
        off += tail;
        let Some(id) = stmt else { continue };
        loop {
            let rc = sqlite3_step(&mut db2, id);
            if rc == SQLITE_ROW {
                w(st, b"2: ");
                print_row(&mut db2, id, st);
            } else {
                if rc != SQLITE_DONE {
                    let m = errmsg(&mut db2);
                    w(st, format!("2: Error: {} ", rc).as_bytes());
                    w(st, &m);
                    w(st, b"\n");
                }
                break;
            }
        }
        sqlite3_finalize(&mut db2, id);
    }
    st.db2 = Some(db2);
}

/// Output from a callback (the main loop holds the reentrant stdout lock).
fn out_cb(b: &[u8]) {
    std::io::stdout().write_all(b).unwrap();
}

fn meta_command(db: &mut Sqlite3, last: Option<StmtId>, zLine: &[u8], st: &mut St) {
    let az = words(zLine, 8);
    if az.is_empty() {
        return;
    }
    let n = az.len();
    let cmd = az[0].as_slice();
    // the words of the line after the first k ones, as the C code rebuilds them
    let rest = |k: usize| -> Vec<u8> { words(zLine, k + 1).get(k).cloned().unwrap_or_default() };
    match cmd {
        b".bind" if n >= 3 => {
            let val = if n >= 4 { Some(rest(3)) } else { None };
            let mut idx = az[1].clone();
            idx.truncate(63);
            let mut ty = s(&az[2]);
            ty.truncate(15);
            st.binds.push(Bind { idx, ty, val });
        }
        b".rerun" => {
            if let Some(id) = last {
                sqlite3_reset(db, id);
                apply_binds(db, id, st);
                run_stmt(db, id, st);
            }
        }
        b".reset" => {
            if let Some(id) = last {
                let rc = sqlite3_reset(db, id);
                w(st, format!("reset rc={}\n", rc).as_bytes());
            }
        }
        b".clear" => {
            if let Some(id) = last {
                let rc = sqlite3_clear_bindings(db, id);
                w(st, format!("clear rc={}\n", rc).as_bytes());
            }
        }
        b".info" => {
            if let Some(id) = last {
                let np = sqlite3_bind_parameter_count(db, id);
                let zx = sqlite3_expanded_sql(db, id);
                let zs = sqlite3_sql(db, id).unwrap_or_default();
                w(st, b"sql=[");
                w(st, &zs);
                w(st, b"]\nexpanded=[");
                w(st, zx.as_deref().unwrap_or(b"<null>"));
                w(st, b"]\n");
                let m = format!(
                    "readonly={} isexplain={} busy={} columns={} data={} params={}\n",
                    sqlite3_stmt_readonly(db, id),
                    sqlite3_stmt_isexplain(db, id),
                    sqlite3_stmt_busy(db, id),
                    sqlite3_column_count(db, id),
                    sqlite3_data_count(db, id),
                    np
                );
                w(st, m.as_bytes());
                for i in 1..=np {
                    let zn = sqlite3_bind_parameter_name(db, id, i);
                    w(st, format!("param {}=", i).as_bytes());
                    w(st, zn.as_deref().unwrap_or(b"<null>"));
                    w(st, b"\n");
                }
            }
        }
        b".stmtstatus" => {
            if let Some(id) = last {
                let ops = [
                    SQLITE_STMTSTATUS_FULLSCAN_STEP,
                    SQLITE_STMTSTATUS_SORT,
                    SQLITE_STMTSTATUS_AUTOINDEX,
                    SQLITE_STMTSTATUS_VM_STEP,
                    SQLITE_STMTSTATUS_REPREPARE,
                    SQLITE_STMTSTATUS_RUN,
                    SQLITE_STMTSTATUS_FILTER_MISS,
                    SQLITE_STMTSTATUS_FILTER_HIT,
                ];
                let mut m = String::from("stmtstatus");
                for op in ops {
                    m.push_str(&format!(" {}", sqlite3_stmt_status(db, id, op as i32, 0)));
                }
                m.push('\n');
                w(st, m.as_bytes());
            }
        }
        b".err" => {
            let m = format!(
                "errcode={} extended={} offset={} msg=",
                sqlite3_errcode(db),
                sqlite3_extended_errcode(db),
                sqlite3_error_offset(db)
            );
            w(st, m.as_bytes());
            let e = errmsg(db);
            w(st, &e);
            w(st, b"\n");
        }
        b".typed" if n >= 2 => st.typed = atoi(&az[1]) as i32,
        b".func" if n >= 3 => {
            let mut fl = SQLITE_UTF8 as i32 | SQLITE_DETERMINISTIC as i32;
            if n >= 4 {
                fl = SQLITE_UTF8 as i32;
                for c in az[3].iter() {
                    match c {
                        b'd' => fl |= SQLITE_DETERMINISTIC as i32,
                        b'D' => fl |= SQLITE_DIRECTONLY as i32,
                        b'i' => fl |= SQLITE_INNOCUOUS as i32,
                        b's' => fl |= SQLITE_SUBTYPE as i32,
                        _ => {}
                    }
                }
            }
            let name = az[1].clone();
            let f: XSFunc = Rc::new(move |ctx: &mut Context, argv: &mut [Mem]| {
                let mut r = name.clone();
                r.push(b'(');
                for (i, a) in argv.iter_mut().enumerate() {
                    if i > 0 {
                        r.push(b',');
                    }
                    append_value(&mut r, a);
                }
                r.push(b')');
                sqlite3_result_text(ctx, Some(&r), r.len() as i32);
            });
            let rc = sqlite3_create_function_v2(
                db,
                Some(&az[1]),
                atoi(&az[2]) as i32,
                fl,
                FuncUserData::None,
                Some(f),
                None,
                None,
                None,
            );
            w(st, format!("func rc={}\n", rc).as_bytes());
        }
        b".agg" if n >= 2 => {
            let step: XSFunc = Rc::new(|ctx: &mut Context, argv: &mut [Mem]| {
                let mut add = Vec::new();
                for (i, a) in argv.iter_mut().enumerate() {
                    if i > 0 {
                        add.push(b',');
                    }
                    append_value(&mut add, a);
                }
                if let Some(p) = sqlite3_aggregate_context::<AggCtx>(ctx, 16) {
                    let s = p.s.get_or_insert_with(Vec::new);
                    if p.n != 0 {
                        s.push(b'+');
                    }
                    s.extend_from_slice(&add);
                    p.n += 1;
                }
            });
            let fin: XFinal = Rc::new(|ctx: &mut Context| {
                let r = match sqlite3_aggregate_context::<AggCtx>(ctx, 0) {
                    Some(p) if p.s.is_some() => {
                        let mut r = p.s.take().unwrap();
                        r.extend_from_slice(format!(" n={}", p.n).as_bytes());
                        r
                    }
                    _ => b"empty".to_vec(),
                };
                sqlite3_result_text(ctx, Some(&r), r.len() as i32);
            });
            let rc = sqlite3_create_function(
                db,
                Some(&az[1]),
                -1,
                SQLITE_UTF8 as i32,
                FuncUserData::None,
                None,
                Some(step),
                Some(fin),
            );
            w(st, format!("agg rc={}\n", rc).as_bytes());
        }
        b".coll" if n >= 2 => {
            let f: XCmp = Rc::new(|a: &[u8], b: &[u8]| {
                let k = a.len().min(b.len());
                let mut rc = 0i32;
                for i in 0..k {
                    if a[i] != b[i] {
                        rc = a[i] as i32 - b[i] as i32;
                        break;
                    }
                }
                if rc == 0 {
                    rc = a.len() as i32 - b.len() as i32;
                }
                -rc
            });
            let rc = sqlite3_create_collation(db, Some(&az[1]), SQLITE_UTF8 as i32, Some(f));
            w(st, format!("coll rc={}\n", rc).as_bytes());
        }
        b".blob" if n >= 6 => {
            use goro_sqlite::vdbeblob::*;
            let wr = az[5].first() == Some(&b'w');
            let mut pBlob = None;
            let rc = sqlite3_blob_open(
                db,
                Some(&az[1]),
                Some(&az[2]),
                Some(&az[3]),
                atoi(&az[4]),
                wr as i32,
                &mut pBlob,
            );
            if rc != 0 {
                let m = errmsg(db);
                w(st, format!("blob open rc={} ", rc).as_bytes());
                w(st, &m);
                w(st, b"\n");
                return;
            }
            let mut b = pBlob.unwrap();
            match az[5].first() {
                Some(b'n') => {
                    let m = format!("blob bytes={}\n", sqlite3_blob_bytes(&mut b));
                    w(st, m.as_bytes());
                }
                Some(b'r') if n >= 8 => {
                    let nb = atoi(&az[7]) as i32;
                    let mut buf = vec![0u8; nb.max(1) as usize];
                    let rc = sqlite3_blob_read(db, &mut b, &mut buf, nb, atoi(&az[6]) as i32);
                    let mut m = format!("blob read rc={} ", rc);
                    if rc == 0 {
                        for c in &buf[..nb.max(0) as usize] {
                            m.push_str(&format!("{:02X}", c));
                        }
                    }
                    m.push('\n');
                    w(st, m.as_bytes());
                }
                Some(b'w') if n >= 8 => {
                    let x = unhex(&az[7]);
                    let rc =
                        sqlite3_blob_write(db, &mut b, &x, x.len() as i32, atoi(&az[6]) as i32);
                    w(st, format!("blob write rc={}\n", rc).as_bytes());
                }
                _ => {}
            }
            let rc = sqlite3_blob_close(db, &mut b);
            w(st, format!("blob close rc={}\n", rc).as_bytes());
        }
        b".backup" if n >= 2 => backup_file(db, &az[1], true, st),
        b".restore" if n >= 2 => backup_file(db, &az[1], false, st),
        b".meta" if n >= 4 => {
            let (mut zType, mut zColl) = (None, None);
            let (mut nn, mut pk, mut ai) = (0, 0, 0);
            let dbn = if az[1] == b"-" {
                None
            } else {
                Some(&az[1][..])
            };
            let col = if az[3] == b"-" {
                None
            } else {
                Some(&az[3][..])
            };
            let rc = sqlite3_table_column_metadata(
                db,
                dbn,
                Some(&az[2]),
                col,
                &mut zType,
                &mut zColl,
                &mut nn,
                &mut pk,
                &mut ai,
            );
            if rc != 0 {
                let m = errmsg(db);
                w(st, format!("meta rc={} ", rc).as_bytes());
                w(st, &m);
                w(st, b"\n");
            } else {
                w(st, b"meta type=");
                w(st, zType.as_deref().unwrap_or(b"<null>"));
                w(st, b" coll=");
                w(st, zColl.as_deref().unwrap_or(b"<null>"));
                w(
                    st,
                    format!(" notnull={} pk={} autoinc={}\n", nn, pk, ai).as_bytes(),
                );
            }
        }
        b".limit" if n >= 3 => {
            let old = sqlite3_limit(db, atoi(&az[1]) as i32, atoi(&az[2]) as i32);
            w(st, format!("limit old={}\n", old).as_bytes());
        }
        b".exec" if n >= 2 => {
            let sql = rest(1);
            let mut zErr = None;
            let mut lines: Vec<u8> = Vec::new();
            let mut cb = |vals: &[Option<&[u8]>], cols: &[&[u8]]| -> i32 {
                if vals.is_empty() {
                    // the empty-result callback (C argv==NULL)
                    lines.extend_from_slice(b"empty-result");
                    for c in cols {
                        lines.push(b' ');
                        lines.extend_from_slice(c);
                    }
                    lines.push(b'\n');
                    return 0;
                }
                for (i, c) in cols.iter().enumerate() {
                    if i > 0 {
                        lines.push(b' ');
                    }
                    lines.extend_from_slice(c);
                    lines.push(b'=');
                    lines.extend_from_slice(vals.get(i).copied().flatten().unwrap_or(b"NULL"));
                }
                lines.push(b'\n');
                0
            };
            let rc = goro_sqlite::legacy::sqlite3_exec(db, Some(&sql), Some(&mut cb), &mut zErr);
            w(st, &lines);
            w(st, format!("exec rc={} err=", rc).as_bytes());
            w(st, zErr.as_deref().unwrap_or(b"<null>"));
            w(st, b"\n");
        }
        b".complete" => {
            let sql = if n >= 2 { rest(1) } else { Vec::new() };
            let c = goro_sqlite::complete::sqlite3_complete(Some(&sql));
            w(st, format!("complete={}\n", c).as_bytes());
        }
        b".status" => {
            let m = format!(
                "changes={} total={} lastrowid={} autocommit={} txn={} readonly={}\n",
                sqlite3_changes64(db),
                sqlite3_total_changes64(db),
                sqlite3_last_insert_rowid(db),
                sqlite3_get_autocommit(db),
                sqlite3_txn_state(db, Some(b"main")),
                sqlite3_db_readonly(db, Some(b"main"))
            );
            w(st, m.as_bytes());
        }
        b".conn2" if n >= 2 => {
            if let Some(mut d) = st.db2.take() {
                sqlite3_close(&mut d);
            }
            let mut pdb: Option<Box<Sqlite3>> = None;
            let rc = sqlite3_open_v2(
                Some(&az[1]),
                &mut pdb,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI,
                None,
            );
            st.db2 = pdb;
            w(st, format!("conn2 rc={}\n", rc).as_bytes());
        }
        b".2" if n >= 2 && st.db2.is_some() => {
            let sql = rest(1);
            run_on2(st, &sql);
        }
        b".busy" if n >= 2 => {
            let rc = sqlite3_busy_timeout(db, atoi(&az[1]) as i32);
            w(st, format!("busy rc={}\n", rc).as_bytes());
        }
        b".prep" if n >= 3 => {
            let id = (atoi(&az[1]) & 7) as usize;
            let sql = rest(2);
            if let Some(s0) = st.named[id].take() {
                sqlite3_finalize(db, s0);
            }
            let mut stmt = None;
            let mut tail = 0usize;
            let rc = sqlite3_prepare_v2(db, Some(&sql), -1, &mut stmt, &mut tail);
            st.named[id] = stmt;
            w(st, format!("prep {} rc={}", id, rc).as_bytes());
            if rc != 0 {
                let m = errmsg(db);
                w(st, b" ");
                w(st, &m);
            }
            w(st, b"\n");
        }
        b".step" if n >= 2 => {
            let id = (atoi(&az[1]) & 7) as usize;
            let mut cnt = if n >= 3 { atoi(&az[2]) } else { 1 };
            while let Some(sid) = st.named[id] {
                if cnt <= 0 {
                    break;
                }
                cnt -= 1;
                let rc = sqlite3_step(db, sid);
                if rc == SQLITE_ROW {
                    w(st, format!("{}: ", id).as_bytes());
                    print_row(db, sid, st);
                } else {
                    w(st, format!("{}: rc={}", id, rc).as_bytes());
                    if rc != SQLITE_DONE {
                        let m = errmsg(db);
                        w(st, b" ");
                        w(st, &m);
                    }
                    w(st, b"\n");
                    break;
                }
            }
        }
        b".resetn" if n >= 2 => {
            let id = (atoi(&az[1]) & 7) as usize;
            if let Some(sid) = st.named[id] {
                let rc = sqlite3_reset(db, sid);
                w(st, format!("resetn {} rc={}\n", id, rc).as_bytes());
            }
        }
        b".final" if n >= 2 => {
            let id = (atoi(&az[1]) & 7) as usize;
            if let Some(sid) = st.named[id].take() {
                let rc = sqlite3_finalize(db, sid);
                w(st, format!("final {} rc={}\n", id, rc).as_bytes());
            }
        }
        b".auth" if n >= 2 => {
            if az[1] == b"off" {
                goro_sqlite::auth::sqlite3_set_authorizer(db, None);
                return;
            }
            let (code, mode) = if n >= 3 {
                let m = match &az[1][..] {
                    b"deny" => SQLITE_DENY,
                    b"ignore" => SQLITE_IGNORE,
                    _ => 99,
                };
                (atoi(&az[2]) as i32, m)
            } else {
                (-1, 0)
            };
            let f: XAuth = Rc::new(
                move |op: i32,
                      a: Option<&[u8]>,
                      b: Option<&[u8]>,
                      c: Option<&[u8]>,
                      d: Option<&[u8]>| {
                    let mut l = format!("auth {}", op).into_bytes();
                    for z in [a, b, c, d] {
                        l.push(b' ');
                        l.extend_from_slice(cstr(z.unwrap_or(b"-")));
                    }
                    l.push(b'\n');
                    out_cb(&l);
                    if op == code { mode } else { SQLITE_OK }
                },
            );
            goro_sqlite::auth::sqlite3_set_authorizer(db, Some(f));
        }
        b".hooks" if n >= 2 => {
            let on = az[1] == b"on";
            if on {
                sqlite3_update_hook(
                    db,
                    Some(Rc::new(|op: i32, zDb: &[u8], zTab: &[u8], r: i64| {
                        let mut l = format!("update {} ", op).into_bytes();
                        l.extend_from_slice(cstr(zDb));
                        l.push(b' ');
                        l.extend_from_slice(cstr(zTab));
                        l.extend_from_slice(format!(" {}\n", r).as_bytes());
                        out_cb(&l);
                    })),
                );
                sqlite3_commit_hook(
                    db,
                    Some(Rc::new(|| {
                        out_cb(b"commit\n");
                        0
                    })),
                );
                sqlite3_rollback_hook(db, Some(Rc::new(|| out_cb(b"rollback\n"))));
            } else {
                sqlite3_update_hook(db, None);
                sqlite3_commit_hook(db, None);
                sqlite3_rollback_hook(db, None);
            }
        }
        b".sleep" if n >= 2 => {
            st.out.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(atoi(&az[1]).max(0) as u64));
        }
        _ => {
            w(st, b"unknown command ");
            w(st, cmd);
            w(st, b"\n");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| ":memory:".to_string());
    let mut sql = Vec::new();
    std::io::stdin().read_to_end(&mut sql).unwrap();
    let mut pdb: Option<Box<Sqlite3>> = None;
    let rc = sqlite3_open_v2(
        Some(path.as_bytes()),
        &mut pdb,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI,
        None,
    );
    let mut db = pdb.expect("open");
    if rc != SQLITE_OK {
        println!("Error: open {}", rc);
        return;
    }
    if let Ok(z) = std::env::var("SQLTEST_SEED") {
        let x: i64 = z.trim().parse().unwrap_or(0);
        goro_sqlite::mainapi::sqlite3_test_control(
            &mut db,
            SQLITE_TESTCTRL_PRNG_SEED,
            &[goro_sqlite::printf::PfArg::Int(x)],
        );
    }
    let b_names = std::env::var_os("SQLTEST_NAMES").is_some();
    let mut st = St {
        out: std::io::stdout().lock(),
        binds: Vec::new(),
        db2: None,
        named: [None; 8],
        typed: 0,
        changes: std::env::var_os("SQLTEST_CHANGES").is_some(),
    };
    let mut last: Option<StmtId> = None;
    let mut off = 0usize;
    while off < sql.len() {
        // meta command?
        {
            let mut p = off;
            while p < sql.len() && matches!(sql[p], b' ' | b'\t' | b'\n' | b'\r') {
                p += 1;
            }
            if p + 1 < sql.len()
                && sql[p] == b'.'
                && (sql[p + 1].is_ascii_lowercase() || sql[p + 1] == b'2')
                && (p == 0 || sql[p - 1] == b'\n')
            {
                let e = sql[p..].iter().position(|&c| c == b'\n').map(|k| p + k);
                let mut line = sql[p..e.unwrap_or(sql.len())].to_vec();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                st.out.flush().unwrap();
                meta_command(&mut db, last, &line, &mut st);
                off = e.map(|k| k + 1).unwrap_or(sql.len());
                continue;
            }
        }
        let mut stmt: Option<StmtId> = None;
        let mut tail = 0usize;
        let rc = sqlite3_prepare_v2(
            &mut db,
            Some(&sql[off..]),
            -1, // like the C twin (NUL-terminated text, no length check)
            &mut stmt,
            &mut tail,
        );
        if rc != SQLITE_OK {
            let msg = errmsg(&mut db);
            w(&mut st, format!("Error: {} ", rc).as_bytes());
            w(&mut st, &msg);
            w(&mut st, b"\n");
        }
        if tail == 0 {
            break;
        }
        off += tail;
        let Some(id) = stmt else { continue };
        if let Some(l) = last.take() {
            sqlite3_finalize(&mut db, l);
        }
        let nc = sqlite3_column_count(&mut db, id);
        if b_names && nc > 0 {
            let mut line = b"#".to_vec();
            for i in 0..nc {
                let v = [
                    sqlite3_column_name(&mut db, id, i),
                    sqlite3_column_decltype(&mut db, id, i),
                    sqlite3_column_database_name(&mut db, id, i),
                    sqlite3_column_table_name(&mut db, id, i),
                    sqlite3_column_origin_name(&mut db, id, i),
                ];
                if i > 0 {
                    line.push(b'|');
                }
                for (k, z) in v.iter().enumerate() {
                    line.extend_from_slice(match k {
                        0 => b"",
                        1 => b"(",
                        _ => b",",
                    });
                    line.extend_from_slice(z.as_deref().unwrap_or(b"<null>"));
                }
                line.push(b')');
            }
            line.push(b'\n');
            w(&mut st, &line);
        }
        if !st.binds.is_empty() {
            apply_binds(&mut db, id, &mut st);
        }
        run_stmt(&mut db, id, &mut st);
        last = Some(id);
    }
    if let Some(l) = last.take() {
        sqlite3_finalize(&mut db, l);
    }
    for k in 0..8 {
        if let Some(sid) = st.named[k].take() {
            sqlite3_finalize(&mut db, sid);
        }
    }
    if let Some(mut d) = st.db2.take() {
        sqlite3_close(&mut d);
    }
    sqlite3_close(&mut db);
}
