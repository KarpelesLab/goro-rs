//! Line-oriented SQL shell used to test WAL mode across processes.
//!
//! usage: walshell DBFILE < script
//!
//! Each input line is executed as soon as it is read (output is flushed
//! after every line), so that a script can interleave work with other
//! processes using the same database.  Lines starting with a dot are
//! commands:
//!
//! * `.shell CMD`   run CMD with /bin/sh and wait for it
//! * `.sleep MS`    sleep for MS milliseconds
//! * `.ckpt MODE`   sqlite3_wal_checkpoint_v2() on "main" with MODE
//!   (0..3, or -1 for NOOP); prints `rc|nLog|nCkpt`
//! * `.crash`       exit immediately without closing the database (leaves
//!   a hot WAL / -shm behind, as a killed process would)
//! * `.echo TEXT`   print TEXT
//!
//! Rows are printed as `col|col|...`, errors as `Error: <code> <message>`.

use goro_sqlite::int::*;
use goro_sqlite::mainapi::*;
use goro_sqlite::prepare::sqlite3_prepare_v2;
use goro_sqlite::vdbeapi::*;
use std::io::{BufRead, Write};

fn run_sql(db: &mut Sqlite3, sql: &[u8], out: &mut impl Write) {
    let mut off = 0usize;
    while off < sql.len() {
        let mut stmt: Option<StmtId> = None;
        let mut tail = 0usize;
        let rest = sql[off..].to_vec();
        let rc = sqlite3_prepare_v2(db, Some(&rest), rest.len() as i32, &mut stmt, &mut tail);
        if rc != SQLITE_OK {
            let msg = sqlite3_errmsg(db).unwrap_or_default();
            let _ = writeln!(out, "Error: {} {}", rc, String::from_utf8_lossy(&msg));
        }
        if tail == 0 {
            break;
        }
        off += tail;
        let Some(id) = stmt else { continue };
        loop {
            let rc = sqlite3_step(db, id);
            if rc == SQLITE_ROW {
                let n = sqlite3_column_count(db, id);
                let mut line = Vec::new();
                for i in 0..n {
                    if i > 0 {
                        line.push(b'|');
                    }
                    if sqlite3_column_type(db, id, i) != SQLITE_NULL {
                        line.extend(sqlite3_column_text(db, id, i).unwrap_or_default());
                    }
                }
                line.push(b'\n');
                let _ = out.write_all(&line);
            } else {
                if rc != SQLITE_DONE {
                    let msg = sqlite3_errmsg(db).unwrap_or_default();
                    let _ = writeln!(out, "Error: {} {}", rc, String::from_utf8_lossy(&msg));
                }
                break;
            }
        }
        sqlite3_finalize(db, id);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| ":memory:".to_string());
    let mut pdb: Option<Box<Sqlite3>> = None;
    let rc = sqlite3_open_v2(
        Some(path.as_bytes()),
        &mut pdb,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI,
        None,
    );
    let mut db = pdb.expect("open");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if rc != SQLITE_OK {
        let _ = writeln!(out, "Error: open {}", rc);
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_default();
        let t = line.trim();
        if let Some(cmd) = t.strip_prefix(".shell ") {
            let _ = out.flush();
            let _ = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(cmd)
                .status();
        } else if let Some(ms) = t.strip_prefix(".sleep ") {
            std::thread::sleep(std::time::Duration::from_millis(
                ms.trim().parse().unwrap_or(0),
            ));
        } else if let Some(m) = t.strip_prefix(".ckpt ") {
            let (mut n_log, mut n_ckpt) = (0, 0);
            let rc = sqlite3_wal_checkpoint_v2(
                &mut db,
                Some(b"main"),
                m.trim().parse().unwrap_or(0),
                Some(&mut n_log),
                Some(&mut n_ckpt),
            );
            let _ = writeln!(out, "{}|{}|{}", rc, n_log, n_ckpt);
        } else if t == ".crash" {
            let _ = out.flush();
            std::process::exit(0);
        } else if let Some(s) = t.strip_prefix(".echo ") {
            let _ = writeln!(out, "{}", s);
        } else if !t.is_empty() {
            run_sql(&mut db, t.as_bytes(), &mut out);
        }
        let _ = out.flush();
    }
    sqlite3_close(&mut db);
}
