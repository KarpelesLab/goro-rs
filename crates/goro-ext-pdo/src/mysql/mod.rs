//! ext/pdo_mysql: the MySQL PDO driver (pdo_mysql.c) and Pdo\Mysql.

pub mod driver;
pub mod statement;

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Func, parse};

use crate::core::*;
pub use statement::S;

// enum of php_pdo_mysql_int.h (a mysqlnd build)
pub const PDO_MYSQL_ATTR_USE_BUFFERED_QUERY: i64 = PDO_ATTR_DRIVER_SPECIFIC;
pub const PDO_MYSQL_ATTR_LOCAL_INFILE: i64 = PDO_ATTR_DRIVER_SPECIFIC + 1;
pub const PDO_MYSQL_ATTR_INIT_COMMAND: i64 = PDO_ATTR_DRIVER_SPECIFIC + 2;
pub const PDO_MYSQL_ATTR_COMPRESS: i64 = PDO_ATTR_DRIVER_SPECIFIC + 3;
pub const PDO_MYSQL_ATTR_FOUND_ROWS: i64 = PDO_ATTR_DRIVER_SPECIFIC + 4;
pub const PDO_MYSQL_ATTR_IGNORE_SPACE: i64 = PDO_ATTR_DRIVER_SPECIFIC + 5;
pub const PDO_MYSQL_ATTR_SSL_KEY: i64 = PDO_ATTR_DRIVER_SPECIFIC + 6;
pub const PDO_MYSQL_ATTR_SSL_CERT: i64 = PDO_ATTR_DRIVER_SPECIFIC + 7;
pub const PDO_MYSQL_ATTR_SSL_CA: i64 = PDO_ATTR_DRIVER_SPECIFIC + 8;
pub const PDO_MYSQL_ATTR_SSL_CAPATH: i64 = PDO_ATTR_DRIVER_SPECIFIC + 9;
pub const PDO_MYSQL_ATTR_SSL_CIPHER: i64 = PDO_ATTR_DRIVER_SPECIFIC + 10;
pub const PDO_MYSQL_ATTR_SERVER_PUBLIC_KEY: i64 = PDO_ATTR_DRIVER_SPECIFIC + 11;
pub const PDO_MYSQL_ATTR_MULTI_STATEMENTS: i64 = PDO_ATTR_DRIVER_SPECIFIC + 12;
pub const PDO_MYSQL_ATTR_SSL_VERIFY_SERVER_CERT: i64 = PDO_ATTR_DRIVER_SPECIFIC + 13;
pub const PDO_MYSQL_ATTR_LOCAL_INFILE_DIRECTORY: i64 = PDO_ATTR_DRIVER_SPECIFIC + 14;

/// The driver's class constants in declaration order (Pdo\Mysql::NAME,
/// and the deprecated PDO::MYSQL_NAME aliases)
pub const CONSTANTS: &[(&str, i64)] = &[
    ("ATTR_USE_BUFFERED_QUERY", PDO_MYSQL_ATTR_USE_BUFFERED_QUERY),
    ("ATTR_LOCAL_INFILE", PDO_MYSQL_ATTR_LOCAL_INFILE),
    ("ATTR_INIT_COMMAND", PDO_MYSQL_ATTR_INIT_COMMAND),
    ("ATTR_COMPRESS", PDO_MYSQL_ATTR_COMPRESS),
    ("ATTR_DIRECT_QUERY", PDO_ATTR_EMULATE_PREPARES),
    ("ATTR_FOUND_ROWS", PDO_MYSQL_ATTR_FOUND_ROWS),
    ("ATTR_IGNORE_SPACE", PDO_MYSQL_ATTR_IGNORE_SPACE),
    ("ATTR_SSL_KEY", PDO_MYSQL_ATTR_SSL_KEY),
    ("ATTR_SSL_CERT", PDO_MYSQL_ATTR_SSL_CERT),
    ("ATTR_SSL_CA", PDO_MYSQL_ATTR_SSL_CA),
    ("ATTR_SSL_CAPATH", PDO_MYSQL_ATTR_SSL_CAPATH),
    ("ATTR_SSL_CIPHER", PDO_MYSQL_ATTR_SSL_CIPHER),
    ("ATTR_SERVER_PUBLIC_KEY", PDO_MYSQL_ATTR_SERVER_PUBLIC_KEY),
    ("ATTR_MULTI_STATEMENTS", PDO_MYSQL_ATTR_MULTI_STATEMENTS),
    (
        "ATTR_SSL_VERIFY_SERVER_CERT",
        PDO_MYSQL_ATTR_SSL_VERIFY_SERVER_CERT,
    ),
    (
        "ATTR_LOCAL_INFILE_DIRECTORY",
        PDO_MYSQL_ATTR_LOCAL_INFILE_DIRECTORY,
    ),
];

/// Pdo\Mysql::getWarningCount
pub fn get_warning_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let o = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    let dbh = match crate::objects::dbh_of(&o) {
        Some(d) => d,
        None => return Value::Null,
    };
    if dbh.borrow().driver.is_none() {
        let m = format!(
            "{} object is uninitialized",
            goro_core::value::display_class_name(&o.borrow().class_name)
        );
        throw(vm, b"Error", &m);
        return Value::Null;
    }
    match driver::server(&dbh) {
        Some(c) => Value::Long(c.borrow().upsert.warning_count as i64),
        None => Value::Long(0),
    }
}

/// PHP_MINFO(pdo_mysql)
pub fn minfo(vm: &mut Vm) -> String {
    let sock = match vm.constants.get(b"pdo_mysql.default_socket".as_slice()) {
        Some(Value::String(s)) if !s.is_empty() => s.to_string_lossy(),
        _ => "no value".to_string(),
    };
    format!(
        "\npdo_mysql\n\nPDO Driver for MySQL => enabled\nClient API version => {}\n\nDirective => Local Value => Master Value\npdo_mysql.default_socket => {} => {}\n",
        goro_ext_mysqli::link::client_info(vm),
        sock,
        sock
    )
}

/// PHP_MINIT(pdo_mysql)
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"pdo_mysql");
    vm.constants
        .entry(b"pdo_mysql.default_socket".to_vec())
        .or_insert_with(|| {
            Value::String(goro_core::string::PhpString::from_bytes(b"/tmp/mysql.sock"))
        });
    register_driver(&driver::MYSQL_DRIVER);
    register_driver_specific_ce(&driver::MYSQL_DRIVER, b"pdo\\mysql");
    vm.phpinfo_hooks.push(minfo);
}
