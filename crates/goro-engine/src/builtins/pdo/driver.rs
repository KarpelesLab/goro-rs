//! The interface between PDO and its database drivers (pdo_sqlite now;
//! pdo_mysql and pdo_pgsql later). PDO keeps the PHP-facing state
//! (attributes, error modes, bound parameters, fetch modes); a driver
//! runs statements and reports errors as SQLSTATE + driver code + text
//! (PHP manual: PDO::errorInfo).

use std::any::Any;

use goro_runtime::{PhpArray, Value};

use crate::vm::{R, Vm};

/// A driver error: what `errorInfo()` reports.
#[derive(Clone, Debug, PartialEq)]
pub struct DrvError {
    /// The five-character SQLSTATE.
    pub sqlstate: String,
    /// The driver-specific error code.
    pub code: Option<i64>,
    /// The driver-specific message.
    pub msg: Option<String>,
}

impl DrvError {
    pub fn new(sqlstate: &str, code: Option<i64>, msg: Option<String>) -> DrvError {
        DrvError {
            sqlstate: sqlstate.to_string(),
            code,
            msg,
        }
    }

    /// An error raised by PDO itself ("HY093", "parameter was not
    /// defined"): no driver code.
    pub fn pdo(sqlstate: &str, msg: &str) -> DrvError {
        DrvError::new(sqlstate, None, Some(msg.to_string()))
    }
}

/// How a statement failed: a driver/PDO error, or a PHP exception raised
/// while running it (a user function, a stream read).
pub enum Failure {
    Db(DrvError),
    Php(crate::vm::Unwind),
}

impl From<DrvError> for Failure {
    fn from(e: DrvError) -> Failure {
        Failure::Db(e)
    }
}

impl From<crate::vm::Unwind> for Failure {
    fn from(u: crate::vm::Unwind) -> Failure {
        Failure::Php(u)
    }
}

pub type DResult<T> = Result<T, Failure>;

/// The placeholder styles a driver prepares natively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholders {
    /// None: PDO substitutes quoted values into the query (emulation).
    None,
    /// `?` only: PDO rewrites `:name` to `?`.
    Positional,
    /// Both `?` and `:name`.
    Both,
}

/// A parameter value at execute time.
#[derive(Clone, Debug)]
pub struct ExecParam {
    /// 0-based position, or the name with its leading `:`.
    pub key: ParamKey,
    /// The value (dereferenced).
    pub value: Value,
    /// The PDO::PARAM_* type, flags included.
    pub ptype: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamKey {
    Pos(usize),
    Name(Vec<u8>),
}

/// A connection.
pub trait Driver {
    fn name(&self) -> &'static str;
    /// The placeholder styles of [`Driver::prepare`].
    fn placeholders(&self) -> Placeholders;
    /// Prepares one statement. `opts` are the driver options of
    /// `PDO::prepare()`.
    fn prepare(&mut self, vm: &mut Vm, sql: &[u8], opts: &PhpArray)
    -> DResult<Box<dyn DriverStmt>>;
    /// Runs SQL without a result set; the number of affected rows.
    fn exec(&mut self, vm: &mut Vm, sql: &[u8]) -> DResult<i64>;
    /// Whether the driver quotes strings (PDO::quote()).
    fn supports_quote(&self) -> bool {
        true
    }
    /// Quotes a string for SQL text; `Ok(None)` when it can't be (the
    /// driver has reported why).
    fn quote(&mut self, vm: &mut Vm, s: &[u8], ptype: i64) -> R<Option<Vec<u8>>>;
    fn begin(&mut self, vm: &mut Vm) -> DResult<()>;
    fn commit(&mut self, vm: &mut Vm) -> DResult<()>;
    fn rollback(&mut self, vm: &mut Vm) -> DResult<()>;
    /// Whether a transaction is open (`None`: PDO's own bookkeeping).
    fn in_transaction(&self) -> Option<bool>;
    fn last_insert_id(&mut self, name: Option<&[u8]>) -> DResult<Vec<u8>>;
    /// A driver attribute: `Ok(None)` when the driver has no such
    /// attribute.
    fn set_attribute(&mut self, vm: &mut Vm, attr: i64, v: &Value) -> R<Option<bool>>;
    fn get_attribute(&mut self, vm: &mut Vm, attr: i64) -> R<Option<Value>>;
    /// The last error of the connection, for `errorInfo()` of an error
    /// raised by PDO itself.
    fn last_error(&self) -> Option<DrvError> {
        None
    }
    /// PHP values the connection holds (callbacks), for the cycle
    /// collector.
    fn trace(&self, _visit: &mut dyn FnMut(&Value)) {}
    fn clear(&mut self) {}
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A prepared statement of a driver.
pub trait DriverStmt {
    /// Runs the statement with its bound parameters.
    fn execute(&mut self, vm: &mut Vm, params: &[ExecParam]) -> DResult<()>;
    /// The columns of the current result set.
    fn column_count(&self) -> usize;
    fn column_name(&self, i: usize) -> Vec<u8>;
    /// The next row, in each column's native PHP type.
    fn fetch(&mut self, vm: &mut Vm) -> DResult<Option<Vec<Value>>>;
    /// Affected rows of the last execution.
    fn row_count(&self) -> i64;
    /// `getColumnMeta()` keys of the driver (native_type, flags, table,
    /// driver:decl_type…) for column `i`.
    fn column_meta(&mut self, vm: &mut Vm, i: usize) -> DResult<Option<PhpArray>>;
    fn close_cursor(&mut self) {}
    /// Moves to the next result set (`Ok(false)`: none).
    fn next_rowset(&mut self) -> DResult<bool> {
        Ok(false)
    }
    /// Whether the statement is in the middle of a result set.
    fn active(&self) -> bool {
        false
    }
    fn set_attribute(&mut self, _vm: &mut Vm, _attr: i64, _v: &Value) -> R<Option<bool>> {
        Ok(None)
    }
    fn get_attribute(&mut self, _vm: &mut Vm, _attr: i64) -> R<Option<Value>> {
        Ok(None)
    }
    /// The number of parameters, when the driver knows it.
    fn param_count(&self) -> Option<usize> {
        None
    }
}

/// SQLSTATE texts (the ODBC SQLSTATE appendix and SQL standard classes),
/// as PDO prints them after "SQLSTATE[xxxxx]: ".
pub fn sqlstate_message(state: &str) -> &'static str {
    match state {
        "00000" => "No error",
        "01000" => "Warning",
        "01001" => "Cursor operation conflict",
        "01002" => "Disconnect error",
        "01003" => "NULL value eliminated in set function",
        "01004" => "String data, right truncated",
        "01006" => "Privilege not revoked",
        "01007" => "Privilege not granted",
        "01008" => "Implicit zero bit padding",
        "0100C" => "Dynamic result sets returned",
        "01P01" => "Deprecated feature",
        "01S00" => "Invalid connection string attribute",
        "01S01" => "Error in row",
        "01S02" => "Option value changed",
        "01S06" => "Attempt to fetch before the result set returned the first rowset",
        "01S07" => "Fractional truncation",
        "01S08" => "Error saving File DSN",
        "01S09" => "Invalid keyword",
        "03000" => "SQL statement not yet complete",
        "07001" => "Wrong number of parameters",
        "07002" => "COUNT field incorrect",
        "07005" => "Prepared statement not a cursor-specification",
        "07006" => "Restricted data type attribute violation",
        "07009" => "Invalid descriptor index",
        "07S01" => "Invalid use of default parameter",
        "08000" => "Connection exception",
        "08001" => "Client unable to establish connection",
        "08002" => "Connection name in use",
        "08003" => "Connection does not exist",
        "08004" => "Server rejected the connection",
        "08006" => "Connection failure",
        "08007" => "Connection failure during transaction",
        "08S01" => "Communication link failure",
        "09000" => "Triggered action exception",
        "0A000" => "Feature not supported",
        "0B000" => "Invalid transaction initiation",
        "0F000" => "Locator exception",
        "0F001" => "Invalid locator specification",
        "0L000" => "Invalid grantor",
        "0LP01" => "Invalid grant operation",
        "0P000" => "Invalid role specification",
        "21000" => "Cardinality violation",
        "21S01" => "Insert value list does not match column list",
        "21S02" => "Degree of derived table does not match column list",
        "22000" => "Data exception",
        "22001" => "String data, right truncated",
        "22002" => "Indicator variable required but not supplied",
        "22003" => "Numeric value out of range",
        "22004" => "Null value not allowed",
        "22005" => "Error in assignment",
        "22007" => "Invalid datetime format",
        "22008" => "Datetime field overflow",
        "22009" => "Invalid time zone displacement value",
        "2200B" => "Escape character conflict",
        "2200C" => "Invalid use of escape character",
        "2200D" => "Invalid escape octet",
        "2200F" => "Zero length character string",
        "2200G" => "Most specific type mismatch",
        "22010" => "Invalid indicator parameter value",
        "22011" => "Substring error",
        "22012" => "Division by zero",
        "22015" => "Interval field overflow",
        "22018" => "Invalid character value for cast specification",
        "22019" => "Invalid escape character",
        "2201B" => "Invalid regular expression",
        "2201E" => "Invalid argument for logarithm",
        "2201F" => "Invalid argument for power function",
        "2201G" => "Invalid argument for width bucket function",
        "22020" => "Invalid limit value",
        "22021" => "Character not in repertoire",
        "22022" => "Indicator overflow",
        "22023" => "Invalid parameter value",
        "22024" => "Unterminated C string",
        "22025" => "Invalid escape sequence",
        "22026" => "String data, length mismatch",
        "22027" => "Trim error",
        "2202E" => "Array subscript error",
        "22P01" => "Floating point exception",
        "22P02" => "Invalid text representation",
        "22P03" => "Invalid binary representation",
        "22P04" => "Bad copy file format",
        "22P05" => "Untranslatable character",
        "23000" => "Integrity constraint violation",
        "23001" => "Restrict violation",
        "23502" => "Not null violation",
        "23503" => "Foreign key violation",
        "23505" => "Unique violation",
        "23514" => "Check violation",
        "24000" => "Invalid cursor state",
        "25000" => "Invalid transaction state",
        "25001" => "Active SQL transaction",
        "25002" => "Branch transaction already active",
        "25003" => "Inappropriate access mode for branch transaction",
        "25004" => "Inappropriate isolation level for branch transaction",
        "25005" => "No active SQL transaction for branch transaction",
        "25006" => "Read only SQL transaction",
        "25007" => "Schema and data statement mixing not supported",
        "25008" => "Held cursor requires same isolation level",
        "25P01" => "No active SQL transaction",
        "25P02" => "In failed SQL transaction",
        "25S01" => "Transaction state",
        "25S02" => "Transaction is still active",
        "25S03" => "Transaction is rolled back",
        "26000" => "Invalid SQL statement name",
        "27000" => "Triggered data change violation",
        "28000" => "Invalid authorization specification",
        "2B000" => "Dependent privilege descriptors still exist",
        "2BP01" => "Dependent objects still exist",
        "2D000" => "Invalid transaction termination",
        "2F000" => "SQL routine exception",
        "2F002" => "Modifying SQL data not permitted",
        "2F003" => "Prohibited SQL statement attempted",
        "2F004" => "Reading SQL data not permitted",
        "2F005" => "Function executed no return statement",
        "34000" => "Invalid cursor name",
        "38000" => "External routine exception",
        "38001" => "Containing SQL not permitted",
        "38002" => "Modifying SQL data not permitted",
        "38003" => "Prohibited SQL statement attempted",
        "38004" => "Reading SQL data not permitted",
        "39000" => "External routine invocation exception",
        "39001" => "Invalid SQLSTATE returned",
        "39004" => "Null value not allowed",
        "39P01" => "Trigger protocol violated",
        "39P02" => "SRF protocol violated",
        "3B000" => "Savepoint exception",
        "3B001" => "Invalid savepoint specification",
        "3C000" => "Duplicate cursor name",
        "3D000" => "Invalid catalog name",
        "3F000" => "Invalid schema name",
        "40000" => "Transaction rollback",
        "40001" => "Serialization failure",
        "40002" => "Transaction integrity constraint violation",
        "40003" => "Statement completion unknown",
        "40P01" => "Deadlock detected",
        "42000" => "Syntax error or access violation",
        "42501" => "Insufficient privilege",
        "42601" => "Syntax error",
        "42602" => "Invalid name",
        "42611" => "Invalid column definition",
        "42622" => "Name too long",
        "42701" => "Duplicate column",
        "42702" => "Ambiguous column",
        "42703" => "Undefined column",
        "42704" => "Undefined object",
        "42710" => "Duplicate object",
        "42712" => "Duplicate alias",
        "42723" => "Duplicate function",
        "42725" => "Ambiguous function",
        "42803" => "Grouping error",
        "42804" => "Datatype mismatch",
        "42809" => "Wrong object type",
        "42830" => "Invalid foreign key",
        "42846" => "Cannot coerce",
        "42883" => "Undefined function",
        "42939" => "Reserved name",
        "42P01" => "Undefined table",
        "42P02" => "Undefined parameter",
        "42P03" => "Duplicate cursor",
        "42P04" => "Duplicate database",
        "42P05" => "Duplicate prepared statement",
        "42P06" => "Duplicate schema",
        "42P07" => "Duplicate table",
        "42P08" => "Ambiguous parameter",
        "42P09" => "Ambiguous alias",
        "42P10" => "Invalid column reference",
        "42P11" => "Invalid cursor definition",
        "42P12" => "Invalid database definition",
        "42P13" => "Invalid function definition",
        "42P14" => "Invalid prepared statement definition",
        "42P15" => "Invalid schema definition",
        "42P16" => "Invalid table definition",
        "42P17" => "Invalid object definition",
        "42P18" => "Indeterminate datatype",
        "42S01" => "Base table or view already exists",
        "42S02" => "Base table or view not found",
        "42S11" => "Index already exists",
        "42S12" => "Index not found",
        "42S21" => "Column already exists",
        "42S22" => "Column not found",
        "44000" => "WITH CHECK OPTION violation",
        "53000" => "Insufficient resources",
        "53100" => "Disk full",
        "53200" => "Out of memory",
        "53300" => "Too many connections",
        "54000" => "Program limit exceeded",
        "54001" => "Statement too complex",
        "54011" => "Too many columns",
        "54023" => "Too many arguments",
        "55000" => "Object not in prerequisite state",
        "55006" => "Object in use",
        "55P02" => "Cant change runtime param",
        "55P03" => "Lock not available",
        "57000" => "Operator intervention",
        "57014" => "Query canceled",
        "57P01" => "Admin shutdown",
        "57P02" => "Crash shutdown",
        "57P03" => "Cannot connect now",
        "58030" => "Io error",
        "58P01" => "Undefined file",
        "58P02" => "Duplicate file",
        "F0000" => "Config file error",
        "F0001" => "Lock file exists",
        "HY000" => "General error",
        "HY001" => "Memory allocation error",
        "HY003" => "Invalid application buffer type",
        "HY004" => "Invalid SQL data type",
        "HY007" => "Associated statement is not prepared",
        "HY008" => "Operation canceled",
        "HY009" => "Invalid use of null pointer",
        "HY010" => "Function sequence error",
        "HY011" => "Attribute cannot be set now",
        "HY012" => "Invalid transaction operation code",
        "HY013" => "Memory management error",
        "HY014" => "Limit on the number of handles exceeded",
        "HY015" => "No cursor name available",
        "HY016" => "Cannot modify an implementation row descriptor",
        "HY017" => "Invalid use of an automatically allocated descriptor handle",
        "HY018" => "Server declined cancel request",
        "HY019" => "Non-character and non-binary data sent in pieces",
        "HY020" => "Attempt to concatenate a null value",
        "HY021" => "Inconsistent descriptor information",
        "HY024" => "Invalid attribute value",
        "HY090" => "Invalid string or buffer length",
        "HY091" => "Invalid descriptor field identifier",
        "HY092" => "Invalid attribute/option identifier",
        "HY093" => "Invalid parameter number",
        "HY095" => "Function type out of range",
        "HY096" => "Invalid information type",
        "HY097" => "Column type out of range",
        "HY098" => "Scope type out of range",
        "HY099" => "Nullable type out of range",
        "HY100" => "Uniqueness option type out of range",
        "HY101" => "Accuracy option type out of range",
        "HY103" => "Invalid retrieval code",
        "HY104" => "Invalid precision or scale value",
        "HY105" => "Invalid parameter type",
        "HY106" => "Fetch type out of range",
        "HY107" => "Row value out of range",
        "HY109" => "Invalid cursor position",
        "HY110" => "Invalid driver completion",
        "HY111" => "Invalid bookmark value",
        "HYC00" => "Optional feature not implemented",
        "HYT00" => "Timeout expired",
        "HYT01" => "Connection timeout expired",
        "IM001" => "Driver does not support this function",
        "IM002" => "Data source name not found and no default driver specified",
        "IM003" => "Specified driver could not be loaded",
        "IM004" => "Driver's SQLAllocHandle on SQL_HANDLE_ENV failed",
        "IM005" => "Driver's SQLAllocHandle on SQL_HANDLE_DBC failed",
        "IM006" => "Driver's SQLSetConnectAttr failed",
        "IM007" => "No data source or driver specified; dialog prohibited",
        "IM008" => "Dialog failed",
        "IM009" => "Unable to load translation DLL",
        "IM010" => "Data source name too long",
        "IM011" => "Driver name too long",
        "IM012" => "DRIVER keyword syntax error",
        "IM013" => "Trace file error",
        "IM014" => "Invalid name of File DSN",
        "IM015" => "Corrupt file data source",
        "P0000" => "PLSQL error",
        "P0001" => "Raise exception",
        "XX000" => "Internal error",
        "XX001" => "Data corrupted",
        "XX002" => "Index corrupted",
        _ => "<<Unknown error>>",
    }
}

/// The message PDO raises for an error: "SQLSTATE[HY000]: General
/// error: 1 no such table: t" (driver code and text when known).
pub fn error_message(e: &DrvError) -> String {
    let mut m = format!(
        "SQLSTATE[{}]: {}",
        e.sqlstate,
        sqlstate_message(&e.sqlstate)
    );
    match (&e.code, &e.msg) {
        (Some(c), Some(t)) => m.push_str(&format!(": {c} {t}")),
        (None, Some(t)) => m.push_str(&format!(": {t}")),
        (Some(c), None) => m.push_str(&format!(": {c}")),
        (None, None) => {}
    }
    m
}
