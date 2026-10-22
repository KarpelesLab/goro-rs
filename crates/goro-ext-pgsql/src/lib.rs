//! php-src's ext/pgsql over `libpq`, a pure-Rust port of PostgreSQL's
//! client library.

pub mod libpq;

#[cfg(test)]
mod tests {
    use super::libpq::*;

    /// GORO_PGSQL_TEST_CONNSTR=... cargo test -p goro-ext-pgsql
    #[test]
    fn live_server() {
        let Ok(cs) = std::env::var("GORO_PGSQL_TEST_CONNSTR") else {
            return;
        };
        let mut c = PgConn::connectdb(cs.as_bytes());
        assert_eq!(
            c.status,
            ConnStatus::Ok,
            "{}",
            String::from_utf8_lossy(c.error_message())
        );
        let r = c.exec(b"select 1 as a, 'x'::text as b, null as c").unwrap();
        assert_eq!(r.status, ExecStatus::TuplesOk);
        assert_eq!(r.value(0, 0), b"1");
        assert!(r.is_null(0, 2));
        let r = c.exec(b"select * from nonexistent").unwrap();
        assert_eq!(r.status, ExecStatus::FatalError);
        eprintln!("{}", String::from_utf8_lossy(r.error_message()));
        let r = c
            .exec_params(b"select $1::int + 1", &[], &[Some(b"41".to_vec())], &[], 0)
            .unwrap();
        assert_eq!(r.value(0, 0), b"42");
        let r = c.prepare(b"s1", b"select $1::text || 'y'", &[]).unwrap();
        assert_eq!(r.status, ExecStatus::CommandOk);
        let r = c
            .exec_prepared(b"s1", &[Some(b"x".to_vec())], &[], 0)
            .unwrap();
        assert_eq!(r.value(0, 0), b"xy");
        c.exec(b"begin");
        let oid = c.lo_creat(lo::INV_READ | lo::INV_WRITE);
        assert!(oid > 0, "{}", String::from_utf8_lossy(c.error_message()));
        let fd = c.lo_open(oid, lo::INV_WRITE | lo::INV_READ);
        assert!(fd >= 0);
        assert_eq!(c.lo_write(fd, b"hello"), 5);
        assert_eq!(c.lo_lseek(fd, 0, 0), 0);
        assert_eq!(c.lo_read(fd, 100).unwrap(), b"hello");
        c.exec(b"commit");
        let r = c.exec(b"create temp table t (a int, b text)").unwrap();
        assert_eq!(r.status, ExecStatus::CommandOk);
        let r = c.exec(b"copy t from stdin").unwrap();
        assert_eq!(r.status, ExecStatus::CopyIn);
        c.put_copy_data(b"1\tfoo\n2\tbar\n");
        c.put_copy_end(None);
        let r = c.get_result().unwrap();
        assert_eq!(r.status, ExecStatus::CommandOk, "{:?}", r);
        assert!(c.get_result().is_none());
        let r = c.exec(b"copy t to stdout").unwrap();
        assert_eq!(r.status, ExecStatus::CopyOut);
        let mut rows = Vec::new();
        while let exec::CopyData::Row(d) = c.get_copy_data(false) {
            rows.push(d);
        }
        assert_eq!(rows.len(), 2);
        let r = c.get_result().unwrap();
        assert_eq!(r.cmd_tuples(), b"2");
        c.exec(b"listen foo");
        c.exec(b"notify foo, 'bar'");
        let n = c.notifies().unwrap();
        assert_eq!(n.relname, b"foo");
        assert_eq!(n.extra, b"bar");
    }
}
