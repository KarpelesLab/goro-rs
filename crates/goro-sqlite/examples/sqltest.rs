//! Minimal SQL runner used for differential testing against the C library.
//!
//! usage: sqltest DBFILE < script.sql
//!
//! Every statement of the script is prepared and stepped in turn; rows are
//! printed as `col|col|...` (NULL as an empty field, blobs as X'..' hex),
//! errors as `Error: <code> <message>`.  A matching C program built from
//! the amalgamation produces the same format.

use goro_sqlite::int::*;
use goro_sqlite::mainapi::*;
use goro_sqlite::prepare::sqlite3_prepare_v2;
use goro_sqlite::vdbeapi::*;
use std::io::{Read, Write};

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
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut off = 0usize;
    while off < sql.len() {
        let mut stmt: Option<StmtId> = None;
        let mut tail = 0usize;
        let rest = sql[off..].to_vec();
        let rc = sqlite3_prepare_v2(
            &mut db,
            Some(&rest),
            rest.len() as i32,
            &mut stmt,
            &mut tail,
        );
        if rc != SQLITE_OK {
            let msg = sqlite3_errmsg(&mut db).unwrap_or_default();
            out.write_all(format!("Error: {} ", rc).as_bytes()).unwrap();
            out.write_all(&msg).unwrap();
            out.write_all(b"\n").unwrap();
            if tail == 0 {
                break;
            }
        }
        if tail == 0 {
            break;
        }
        off += tail;
        let Some(id) = stmt else { continue };
        loop {
            let rc = sqlite3_step(&mut db, id);
            if rc == SQLITE_ROW {
                let n = sqlite3_column_count(&mut db, id);
                let mut line = Vec::new();
                for i in 0..n {
                    if i > 0 {
                        line.push(b'|');
                    }
                    match sqlite3_column_type(&mut db, id, i) {
                        SQLITE_NULL => {}
                        SQLITE_BLOB => {
                            let b = sqlite3_column_blob(&mut db, id, i).unwrap_or_default();
                            line.extend_from_slice(b"X'");
                            for c in b {
                                line.extend_from_slice(format!("{:02X}", c).as_bytes());
                            }
                            line.push(b'\'');
                        }
                        _ => {
                            let t = sqlite3_column_text(&mut db, id, i).unwrap_or_default();
                            line.extend_from_slice(&t);
                        }
                    }
                }
                line.push(b'\n');
                out.write_all(&line).unwrap();
            } else {
                if rc != SQLITE_DONE {
                    let msg = sqlite3_errmsg(&mut db).unwrap_or_default();
                    out.write_all(format!("Error: {} ", rc).as_bytes()).unwrap();
                    out.write_all(&msg).unwrap();
                    out.write_all(b"\n").unwrap();
                }
                break;
            }
        }
        sqlite3_finalize(&mut db, id);
    }
    sqlite3_close(&mut db);
}
