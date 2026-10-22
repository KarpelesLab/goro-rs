//! A pure-Rust port of the parts of libpq (PostgreSQL's C client library)
//! that ext/pgsql and pdo_pgsql use: connection establishment (with
//! cleartext/MD5/SCRAM-SHA-256 authentication and TLS), the v3
//! frontend/backend protocol in blocking and non-blocking mode, COPY,
//! notifications, notices, cancellation, large objects (over the fastpath
//! function call interface), escaping and protocol tracing.

pub mod auth;
pub mod conn;
pub mod conninfo;
pub mod encoding;
pub mod escape;
pub mod exec;
pub mod lo;
pub mod result;
pub mod trace;

pub use conn::PgConn;
pub use result::{AttDesc, PgResult};

pub type Oid = u32;

pub const INVALID_OID: Oid = 0;

/// PQlibVersion(): the libpq release this port follows
pub const LIB_VERSION: i32 = 180000;

/// ConnStatusType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnStatus {
    Ok = 0,
    Bad = 1,
    Started = 2,
    Made = 3,
    AwaitingResponse = 4,
    AuthOk = 5,
    Setenv = 6,
    SslStartup = 7,
    Needed = 8,
    CheckWritable = 9,
    Consume = 10,
    GssStartup = 11,
    CheckTarget = 12,
    CheckStandby = 13,
    Allocated = 14,
}

/// PostgresPollingStatusType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PollingStatus {
    Failed = 0,
    Reading = 1,
    Writing = 2,
    Ok = 3,
    Active = 4,
}

/// ExecStatusType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecStatus {
    EmptyQuery = 0,
    CommandOk = 1,
    TuplesOk = 2,
    CopyOut = 3,
    CopyIn = 4,
    BadResponse = 5,
    NonfatalError = 6,
    FatalError = 7,
    CopyBoth = 8,
    SingleTuple = 9,
    PipelineSync = 10,
    PipelineAborted = 11,
    TuplesChunk = 12,
}

/// PGTransactionStatusType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransStatus {
    Idle = 0,
    Active = 1,
    InTrans = 2,
    InError = 3,
    Unknown = 4,
}

/// PGVerbosity
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verbosity {
    Terse = 0,
    Default = 1,
    Verbose = 2,
    Sqlstate = 3,
}

impl Verbosity {
    pub fn from_i64(v: i64) -> Verbosity {
        match v {
            0 => Verbosity::Terse,
            2 => Verbosity::Verbose,
            3 => Verbosity::Sqlstate,
            _ => Verbosity::Default,
        }
    }
}

/// PGContextVisibility
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContextVisibility {
    Never = 0,
    Errors = 1,
    Always = 2,
}

impl ContextVisibility {
    pub fn from_i64(v: i64) -> ContextVisibility {
        match v {
            0 => ContextVisibility::Never,
            2 => ContextVisibility::Always,
            _ => ContextVisibility::Errors,
        }
    }
}

/// PGPing
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ping {
    Ok = 0,
    Reject = 1,
    NoResponse = 2,
    NoAttempt = 3,
}

// postgres_ext.h PG_DIAG_*
pub const PG_DIAG_SEVERITY: u8 = b'S';
pub const PG_DIAG_SEVERITY_NONLOCALIZED: u8 = b'V';
pub const PG_DIAG_SQLSTATE: u8 = b'C';
pub const PG_DIAG_MESSAGE_PRIMARY: u8 = b'M';
pub const PG_DIAG_MESSAGE_DETAIL: u8 = b'D';
pub const PG_DIAG_MESSAGE_HINT: u8 = b'H';
pub const PG_DIAG_STATEMENT_POSITION: u8 = b'P';
pub const PG_DIAG_INTERNAL_POSITION: u8 = b'p';
pub const PG_DIAG_INTERNAL_QUERY: u8 = b'q';
pub const PG_DIAG_CONTEXT: u8 = b'W';
pub const PG_DIAG_SCHEMA_NAME: u8 = b's';
pub const PG_DIAG_TABLE_NAME: u8 = b't';
pub const PG_DIAG_COLUMN_NAME: u8 = b'c';
pub const PG_DIAG_DATATYPE_NAME: u8 = b'd';
pub const PG_DIAG_CONSTRAINT_NAME: u8 = b'n';
pub const PG_DIAG_SOURCE_FILE: u8 = b'F';
pub const PG_DIAG_SOURCE_LINE: u8 = b'L';
pub const PG_DIAG_SOURCE_FUNCTION: u8 = b'R';

/// PQTRACE_SUPPRESS_TIMESTAMPS / PQTRACE_REGRESS_MODE
pub const PQTRACE_SUPPRESS_TIMESTAMPS: i32 = 1 << 0;
pub const PQTRACE_REGRESS_MODE: i32 = 1 << 1;

/// pgNotify
#[derive(Clone, Debug)]
pub struct Notify {
    pub relname: Vec<u8>,
    pub be_pid: i32,
    pub extra: Vec<u8>,
}

/// PQunescapeBytea and friends live in escape.rs; re-export the common
/// ones.
pub use escape::{escape_bytea, escape_string, unescape_bytea};
