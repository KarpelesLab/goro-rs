//! Shared-cache mode with several connections.
//!
//! Each `tests/shared_cache/NAME.txt` script (format: see
//! examples/sqlmulti.rs) is run through the same runner as the sqlmulti
//! example; `NAME.out` is the output of C SQLite 3.54.1 (default build,
//! shared cache compiled in, SQLITE_THREADSAFE=0) running the script with
//! `tests/shared_cache/multi.c`:
//!
//! ```text
//! gcc -DSQLITE_THREADSAFE=0 -DSQLITE_ENABLE_COLUMN_METADATA \
//!     -DSQLITE_ENABLE_MATH_FUNCTIONS -I$AMALG multi.c $AMALG/sqlite3.c -lm
//! sed "s#\$DIR#$PWD#g" NAME.txt > x.txt && ./a.out x.txt > NAME.out
//! ```

#[path = "../examples/sqlmulti.rs"]
#[allow(dead_code)]
mod sqlmulti;

fn check(name: &str) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/shared_cache");
    let script = std::fs::read_to_string(base.join(format!("{name}.txt"))).unwrap();
    let expected = std::fs::read_to_string(base.join(format!("{name}.out"))).unwrap();
    let dir = std::env::temp_dir().join(format!("goro-sqlite-shc-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let got = sqlmulti::run(&script, dir.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(got, expected, "script {name}");
}

/// The scripts run one after the other: enable_private.txt flips the
/// process-wide sqlite3_enable_shared_cache() setting.
#[test]
fn shared_cache_scripts() {
    for name in [
        "uri_memory",
        "file_shared",
        "enable_private",
        "schema_changes",
    ] {
        check(name);
    }
}
