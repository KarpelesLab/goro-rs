//! Multi-connection script runner used for differential testing of the
//! shared cache against the C library (a matching C program built from the
//! amalgamation produces the same output).
//!
//! usage: sqlmulti SCRIPT [DIR]
//!
//! `$DIR` in the script is replaced by DIR (default: the current
//! directory).
//!
//! Script lines:
//!
//! * `@open N FILENAME` / `@openshared N FILENAME`: sqlite3_open_v2() with
//!   READWRITE|CREATE|URI (plus SHAREDCACHE) as connection N
//! * `@close N`, `@enable B` (sqlite3_enable_shared_cache)
//! * `@prep N NAME SQL`, `@step NAME`, `@fin NAME`: a statement kept open
//! * `N: SQL`: run every statement of SQL on connection N
//!
//! Output: `N: a|b` for rows, `N: Error: rc extended-rc message`.

use goro_sqlite::int::*;
use goro_sqlite::mainapi::*;
use goro_sqlite::prepare::sqlite3_prepare_v2;
use goro_sqlite::vdbeapi::*;

fn row(out: &mut String, n: usize, db: &mut Sqlite3, id: StmtId) {
    let nc = sqlite3_column_count(db, id);
    let mut line = Vec::new();
    for i in 0..nc {
        if i > 0 {
            line.push(b'|');
        }
        if sqlite3_column_type(db, id, i) != SQLITE_NULL {
            line.extend_from_slice(&sqlite3_column_text(db, id, i).unwrap_or_default());
        }
    }
    out.push_str(&format!("{}: {}\n", n, String::from_utf8_lossy(&line)));
}

fn err(out: &mut String, n: usize, db: &mut Sqlite3, rc: i32) {
    let ext = sqlite3_extended_errcode(db);
    let msg = sqlite3_errmsg(db).unwrap_or_default();
    out.push_str(&format!(
        "{}: Error: {} {} {}\n",
        n,
        rc,
        ext,
        String::from_utf8_lossy(&msg)
    ));
}

/// Run a script (with `$DIR` replaced by dir), return its output.
pub fn run(script: &str, dir: &str) -> String {
    let script = script.replace("$DIR", dir);
    let mut out = String::new();
    let mut dbs: Vec<Option<Box<Sqlite3>>> = (0..10).map(|_| None).collect();
    let mut stmts: Vec<(String, usize, StmtId)> = Vec::new();
    for line in script.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let w: Vec<&str> = line.splitn(4, ' ').collect();
        match w[0] {
            "@open" | "@openshared" => {
                let n: usize = w[1].parse().unwrap();
                let mut fl = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI;
                if w[0] == "@openshared" {
                    fl |= SQLITE_OPEN_SHAREDCACHE;
                }
                let mut pdb: Option<Box<Sqlite3>> = None;
                let rc = sqlite3_open_v2(Some(w[2].as_bytes()), &mut pdb, fl, None);
                if rc != 0 {
                    out.push_str(&format!("{}: open error {}\n", n, rc));
                }
                dbs[n] = pdb;
            }
            "@close" => {
                let n: usize = w[1].parse().unwrap();
                let mut db = dbs[n].take().unwrap();
                let rc = sqlite3_close(&mut db);
                out.push_str(&format!("{}: close {}\n", n, rc));
            }
            "@enable" => {
                goro_sqlite::btree::sqlite3_enable_shared_cache(w[1].parse().unwrap());
            }
            "@prep" => {
                let n: usize = w[1].parse().unwrap();
                let db = dbs[n].as_mut().unwrap();
                let sql = w[3].as_bytes().to_vec();
                let mut stmt: Option<StmtId> = None;
                let mut tail = 0usize;
                let rc = sqlite3_prepare_v2(db, Some(&sql), sql.len() as i32, &mut stmt, &mut tail);
                if rc != 0 {
                    err(&mut out, n, db, rc);
                } else if let Some(id) = stmt {
                    stmts.push((w[2].to_string(), n, id));
                }
            }
            "@step" => {
                let Some(&(_, n, id)) = stmts.iter().find(|s| s.0 == w[1]) else {
                    out.push_str(&format!("no stmt {}\n", w[1]));
                    continue;
                };
                let db = dbs[n].as_mut().unwrap();
                let rc = sqlite3_step(db, id);
                if rc == SQLITE_ROW {
                    row(&mut out, n, db, id);
                } else if rc == SQLITE_DONE {
                    out.push_str(&format!("{}: done\n", n));
                } else {
                    err(&mut out, n, db, rc);
                }
            }
            "@fin" => {
                let Some(i) = stmts.iter().position(|s| s.0 == w[1]) else {
                    out.push_str(&format!("no stmt {}\n", w[1]));
                    continue;
                };
                let (_, n, id) = stmts.remove(i);
                sqlite3_finalize(dbs[n].as_mut().unwrap(), id);
            }
            _ => {
                let Some((a, sql)) = line.split_once(':') else {
                    out.push_str(&format!("bad line: {}\n", line));
                    continue;
                };
                let n: usize = a.parse().unwrap();
                let db = dbs[n].as_mut().unwrap();
                let sql = sql.as_bytes().to_vec();
                let mut off = 0usize;
                while off < sql.len() {
                    let rest = sql[off..].to_vec();
                    let mut stmt: Option<StmtId> = None;
                    let mut tail = 0usize;
                    let rc = sqlite3_prepare_v2(
                        db,
                        Some(&rest),
                        rest.len() as i32,
                        &mut stmt,
                        &mut tail,
                    );
                    if rc != 0 {
                        err(&mut out, n, db, rc);
                    }
                    if tail == 0 {
                        break;
                    }
                    off += tail;
                    let Some(id) = stmt else { continue };
                    loop {
                        let rc = sqlite3_step(db, id);
                        if rc == SQLITE_ROW {
                            row(&mut out, n, db, id);
                        } else {
                            if rc != SQLITE_DONE {
                                err(&mut out, n, db, rc);
                            }
                            break;
                        }
                    }
                    sqlite3_finalize(db, id);
                }
            }
        }
    }
    out
}

#[allow(dead_code)]
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: sqlmulti SCRIPT [DIR]");
    let dir = std::env::args().nth(2).unwrap_or_else(|| ".".to_string());
    let script = std::fs::read_to_string(path).unwrap();
    print!("{}", run(&script, &dir));
}
