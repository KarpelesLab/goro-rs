//! ext/pdo (PHP manual: PHP Data Objects): `PDO`, `PDOStatement`,
//! `PDOException`, `PDORow`, `pdo_drivers()`, and pdo_sqlite's
//! `Pdo\Sqlite` (RFC "PDO driver specific subclasses", PHP 8.4).
//!
//! Class shapes and constants from Reflection on PHP 8.5 (PDO core) and
//! the manual (Pdo\Sqlite); behaviour from the manual and the ext/pdo
//! and ext/pdo_sqlite tests.

mod core;
pub mod driver;
mod fetch;
pub mod parser;
mod sqlite;
mod sqlite_ext;
mod stmt;

use crate::registry::Module;

const PDO_EXCEPTION: &str = r#"<?php
class PDOException extends RuntimeException {
    protected $code = 0;
    public ?array $errorInfo = null;
}"#;

const PDO_CLASS: &str = r#"<?php
#[NotSerializable]
#[NotCloneable]
class PDO {
    public const int PARAM_NULL = 0;
    public const int PARAM_BOOL = 5;
    public const int PARAM_INT = 1;
    public const int PARAM_STR = 2;
    public const int PARAM_LOB = 3;
    public const int PARAM_STMT = 4;
    public const int PARAM_INPUT_OUTPUT = 2147483648;
    public const int PARAM_STR_NATL = 1073741824;
    public const int PARAM_STR_CHAR = 536870912;
    public const int PARAM_EVT_ALLOC = 0;
    public const int PARAM_EVT_FREE = 1;
    public const int PARAM_EVT_EXEC_PRE = 2;
    public const int PARAM_EVT_EXEC_POST = 3;
    public const int PARAM_EVT_FETCH_PRE = 4;
    public const int PARAM_EVT_FETCH_POST = 5;
    public const int PARAM_EVT_NORMALIZE = 6;
    public const int FETCH_DEFAULT = 0;
    public const int FETCH_LAZY = 1;
    public const int FETCH_ASSOC = 2;
    public const int FETCH_NUM = 3;
    public const int FETCH_BOTH = 4;
    public const int FETCH_OBJ = 5;
    public const int FETCH_BOUND = 6;
    public const int FETCH_COLUMN = 7;
    public const int FETCH_CLASS = 8;
    public const int FETCH_INTO = 9;
    public const int FETCH_FUNC = 10;
    public const int FETCH_GROUP = 32;
    public const int FETCH_UNIQUE = 64;
    public const int FETCH_KEY_PAIR = 12;
    public const int FETCH_CLASSTYPE = 128;
    public const int FETCH_SERIALIZE = 512;
    public const int FETCH_PROPS_LATE = 256;
    public const int FETCH_NAMED = 11;
    public const int ATTR_AUTOCOMMIT = 0;
    public const int ATTR_PREFETCH = 1;
    public const int ATTR_TIMEOUT = 2;
    public const int ATTR_ERRMODE = 3;
    public const int ATTR_SERVER_VERSION = 4;
    public const int ATTR_CLIENT_VERSION = 5;
    public const int ATTR_SERVER_INFO = 6;
    public const int ATTR_CONNECTION_STATUS = 7;
    public const int ATTR_CASE = 8;
    public const int ATTR_CURSOR_NAME = 9;
    public const int ATTR_CURSOR = 10;
    public const int ATTR_ORACLE_NULLS = 11;
    public const int ATTR_PERSISTENT = 12;
    public const int ATTR_STATEMENT_CLASS = 13;
    public const int ATTR_FETCH_TABLE_NAMES = 14;
    public const int ATTR_FETCH_CATALOG_NAMES = 15;
    public const int ATTR_DRIVER_NAME = 16;
    public const int ATTR_STRINGIFY_FETCHES = 17;
    public const int ATTR_MAX_COLUMN_LEN = 18;
    public const int ATTR_EMULATE_PREPARES = 20;
    public const int ATTR_DEFAULT_FETCH_MODE = 19;
    public const int ATTR_DEFAULT_STR_PARAM = 21;
    public const int ERRMODE_SILENT = 0;
    public const int ERRMODE_WARNING = 1;
    public const int ERRMODE_EXCEPTION = 2;
    public const int CASE_NATURAL = 0;
    public const int CASE_LOWER = 2;
    public const int CASE_UPPER = 1;
    public const int NULL_NATURAL = 0;
    public const int NULL_EMPTY_STRING = 1;
    public const int NULL_TO_STRING = 2;
    public const string ERR_NONE = '00000';
    public const int FETCH_ORI_NEXT = 0;
    public const int FETCH_ORI_PRIOR = 1;
    public const int FETCH_ORI_FIRST = 2;
    public const int FETCH_ORI_LAST = 3;
    public const int FETCH_ORI_ABS = 4;
    public const int FETCH_ORI_REL = 5;
    public const int CURSOR_FWDONLY = 0;
    public const int CURSOR_SCROLL = 1;
    #[\Deprecated(message: "use Pdo\\Sqlite::ATTR_OPEN_FLAGS instead", since: "8.5")]
    public const int SQLITE_ATTR_OPEN_FLAGS = 1000;
    #[\Deprecated(message: "use Pdo\\Sqlite::OPEN_READONLY instead", since: "8.5")]
    public const int SQLITE_OPEN_READONLY = 1;
    #[\Deprecated(message: "use Pdo\\Sqlite::OPEN_READWRITE instead", since: "8.5")]
    public const int SQLITE_OPEN_READWRITE = 2;
    #[\Deprecated(message: "use Pdo\\Sqlite::OPEN_CREATE instead", since: "8.5")]
    public const int SQLITE_OPEN_CREATE = 4;
    #[\Deprecated(message: "use Pdo\\Sqlite::ATTR_READONLY_STATEMENT instead", since: "8.5")]
    public const int SQLITE_ATTR_READONLY_STATEMENT = 1001;
    #[\Deprecated(message: "use Pdo\\Sqlite::ATTR_EXTENDED_RESULT_CODES instead", since: "8.5")]
    public const int SQLITE_ATTR_EXTENDED_RESULT_CODES = 1002;
    #[\Deprecated(message: "use Pdo\\Sqlite::DETERMINISTIC instead", since: "8.5")]
    public const int SQLITE_DETERMINISTIC = 2048;
    public function __construct(string $dsn, ?string $username = null, #[\SensitiveParameter] ?string $password = null, ?array $options = null) {}
    public static function connect(string $dsn, ?string $username = null, #[\SensitiveParameter] ?string $password = null, ?array $options = null): static {}
    public function beginTransaction(): bool {}
    public function commit(): bool {}
    public function errorCode(): ?string {}
    public function errorInfo(): array {}
    public function exec(string $statement): int|false {}
    public function getAttribute(int $attribute): mixed {}
    public static function getAvailableDrivers(): array {}
    public function inTransaction(): bool {}
    public function lastInsertId(?string $name = null): string|false {}
    public function prepare(string $query, array $options = []): PDOStatement|false {}
    public function query(string $query, ?int $fetchMode = null, mixed ...$fetchModeArgs): PDOStatement|false {}
    public function quote(string $string, int $type = PDO::PARAM_STR): string|false {}
    public function rollBack(): bool {}
    public function setAttribute(int $attribute, mixed $value): bool {}
    #[\Deprecated(message: "use Pdo\\Sqlite::createFunction() instead", since: "8.5")]
    public function sqliteCreateFunction(string $function_name, mixed $callback, int $num_args = -1, int $flags = 0): bool {}
    #[\Deprecated(message: "use Pdo\\Sqlite::createAggregate() instead", since: "8.5")]
    public function sqliteCreateAggregate(string $name, mixed $step, mixed $finalize, int $numArgs = -1): bool {}
    #[\Deprecated(message: "use Pdo\\Sqlite::createCollation() instead", since: "8.5")]
    public function sqliteCreateCollation(string $name, mixed $callback): bool {}
}"#;

const PDO_STATEMENT: &str = r#"<?php
#[NotSerializable]
#[NotCloneable]
class PDOStatement implements IteratorAggregate {
    public string $queryString;
    public function bindColumn(string|int $column, mixed &$var, int $type = PDO::PARAM_STR, int $maxLength = 0, mixed $driverOptions = null): bool {}
    public function bindParam(string|int $param, mixed &$var, int $type = PDO::PARAM_STR, int $maxLength = 0, mixed $driverOptions = null): bool {}
    public function bindValue(string|int $param, mixed $value, int $type = PDO::PARAM_STR): bool {}
    public function closeCursor(): bool {}
    public function columnCount(): int {}
    public function debugDumpParams(): ?bool {}
    public function errorCode(): ?string {}
    public function errorInfo(): array {}
    public function execute(?array $params = null): bool {}
    public function fetch(int $mode = PDO::FETCH_DEFAULT, int $cursorOrientation = PDO::FETCH_ORI_NEXT, int $cursorOffset = 0): mixed {}
    public function fetchAll(int $mode = PDO::FETCH_DEFAULT, mixed ...$args): array {}
    public function fetchColumn(int $column = 0): mixed {}
    public function fetchObject(?string $class = "stdClass", array $constructorArgs = []): object|false {}
    public function getAttribute(int $name): mixed {}
    public function getColumnMeta(int $column): array|false {}
    public function nextRowset(): bool {}
    public function rowCount(): int {}
    public function setAttribute(int $attribute, mixed $value): bool {}
    public function setFetchMode(int $mode, mixed ...$args): true {}
    public function getIterator(): Iterator {}
}"#;

const PDO_ROW: &str = r#"<?php
#[NotSerializable]
final class PDORow {
    public string $queryString;
    public function __construct() {}
}"#;

const PDO_SQLITE: &str = r#"<?php
namespace Pdo;
class Sqlite extends \PDO {
    public const int DETERMINISTIC = 2048;
    public const int OPEN_READONLY = 1;
    public const int OPEN_READWRITE = 2;
    public const int OPEN_CREATE = 4;
    public const int ATTR_OPEN_FLAGS = 1000;
    public const int ATTR_READONLY_STATEMENT = 1001;
    public const int ATTR_EXTENDED_RESULT_CODES = 1002;
    public const int ATTR_BUSY_STATEMENT = 1003;
    public const int ATTR_EXPLAIN_STATEMENT = 1004;
    public const int ATTR_TRANSACTION_MODE = 1005;
    public const int EXPLAIN_MODE_PREPARED = 0;
    public const int EXPLAIN_MODE_EXPLAIN = 1;
    public const int EXPLAIN_MODE_EXPLAIN_QUERY_PLAN = 2;
    public const int TRANSACTION_MODE_DEFERRED = 0;
    public const int TRANSACTION_MODE_IMMEDIATE = 1;
    public const int TRANSACTION_MODE_EXCLUSIVE = 2;
    public const int OK = 0;
    public const int DENY = 1;
    public const int IGNORE = 2;
    public function createAggregate(string $name, callable $step, callable $finalize, int $numArgs = -1): bool {}
    public function createCollation(string $name, callable $callback): bool {}
    public function createFunction(string $function_name, callable $callback, int $num_args = -1, int $flags = 0): bool {}
    public function loadExtension(string $name): void {}
    public function openBlob(string $table, string $column, int $rowid, ?string $dbname = "main", int $flags = \Pdo\Sqlite::OPEN_READONLY) {}
    public function setAuthorizer(?callable $callback): void {}
}"#;

pub static MODULE: Module = Module {
    name: "PDO",
    functions: &[("function pdo_drivers(): array", core::pdo_drivers)],
    classes: &[
        ("pdoexception", PDO_EXCEPTION),
        ("pdo", PDO_CLASS),
        ("pdostatement", PDO_STATEMENT),
        ("pdorow", PDO_ROW),
    ],
    methods: &[
        ("pdo::__construct", core::pdo_construct),
        ("pdo::connect", core::pdo_connect),
        ("pdo::begintransaction", core::pdo_begin),
        ("pdo::commit", core::pdo_commit),
        ("pdo::errorcode", core::pdo_error_code),
        ("pdo::errorinfo", core::pdo_error_info),
        ("pdo::exec", core::pdo_exec),
        ("pdo::getattribute", core::pdo_get_attribute),
        ("pdo::getavailabledrivers", core::pdo_drivers),
        ("pdo::intransaction", core::pdo_in_transaction),
        ("pdo::lastinsertid", core::pdo_last_insert_id),
        ("pdo::prepare", core::pdo_prepare),
        ("pdo::query", core::pdo_query),
        ("pdo::quote", core::pdo_quote),
        ("pdo::rollback", core::pdo_rollback),
        ("pdo::setattribute", core::pdo_set_attribute),
        (
            "pdo::sqlitecreatefunction",
            sqlite_ext::legacy_create_function,
        ),
        (
            "pdo::sqlitecreateaggregate",
            sqlite_ext::legacy_create_aggregate,
        ),
        (
            "pdo::sqlitecreatecollation",
            sqlite_ext::legacy_create_collation,
        ),
        ("pdostatement::bindcolumn", stmt::bind_column),
        ("pdostatement::bindparam", stmt::bind_param),
        ("pdostatement::bindvalue", stmt::bind_value),
        ("pdostatement::closecursor", stmt::close_cursor),
        ("pdostatement::columncount", stmt::column_count),
        ("pdostatement::debugdumpparams", stmt::debug_dump_params),
        ("pdostatement::errorcode", stmt::error_code),
        ("pdostatement::errorinfo", stmt::error_info),
        ("pdostatement::execute", stmt::execute),
        ("pdostatement::fetch", fetch::fetch),
        ("pdostatement::fetchall", fetch::fetch_all),
        ("pdostatement::fetchcolumn", fetch::fetch_column),
        ("pdostatement::fetchobject", fetch::fetch_object),
        ("pdostatement::getattribute", stmt::get_attribute),
        ("pdostatement::getcolumnmeta", stmt::get_column_meta),
        ("pdostatement::nextrowset", stmt::next_rowset),
        ("pdostatement::rowcount", stmt::row_count),
        ("pdostatement::setattribute", stmt::set_attribute),
        ("pdostatement::setfetchmode", fetch::set_fetch_mode),
        ("pdostatement::getiterator", fetch::get_iterator),
        ("pdorow::__construct", fetch::row_construct),
    ],
    hooks: &[
        ("pdo", core::dbh_hooks),
        ("pdostatement", stmt::stmt_hooks),
        ("pdorow", fetch::row_hooks),
    ],
    ..Module::EMPTY
};

pub static SQLITE_MODULE: Module = Module {
    name: "pdo_sqlite",
    classes: &[("pdo\\sqlite", PDO_SQLITE)],
    methods: &[
        ("pdo\\sqlite::createaggregate", sqlite_ext::create_aggregate),
        ("pdo\\sqlite::createcollation", sqlite_ext::create_collation),
        ("pdo\\sqlite::createfunction", sqlite_ext::create_function),
        ("pdo\\sqlite::loadextension", sqlite_ext::load_extension),
        ("pdo\\sqlite::openblob", sqlite_ext::open_blob),
        ("pdo\\sqlite::setauthorizer", sqlite_ext::set_authorizer),
    ],
    constants: &[],
    info: Some(sqlite_info),
    ..Module::EMPTY
};

fn sqlite_info(_vm: &mut crate::vm::Vm) -> String {
    format!(
        "PDO Driver for SQLite 3.x => enabled\nSQLite Library => {}\n",
        goro_ext_sqlite::VERSION
    )
}
