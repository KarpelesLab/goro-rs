//! Core data structures of the engine (port of sqliteInt.h, vdbeInt.h and
//! vdbe.h).  See PORTING.md for the conventions used throughout the crate.
//!
//! Naming follows the C sources one-to-one (struct, field and function
//! names), so that the Rust code can be compared line by line with SQLite.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub use crate::arena::*;
pub use crate::consts::*;
pub use crate::hash::Hash;
pub use crate::pager::DbPage;

pub type LogEst = i16;
pub type Pgno = u32;
pub type Bitmask = u64;
pub type yDbMask = u32;
pub type ynVar = i16;
pub type tRowcnt = u64;
pub type i64_ = i64;

/// Number of bits in a Bitmask.
pub const BMS: i32 = 64;
pub const MASKBIT0: Bitmask = 1;
#[inline]
pub fn MASKBIT(n: i32) -> Bitmask {
    1u64 << n
}
#[inline]
pub fn MASKBIT32(n: i32) -> u32 {
    1u32 << n
}
pub const ALLBITS: Bitmask = u64::MAX;
pub const TOPBIT: Bitmask = 1u64 << 63;

pub type TableRef = Rc<RefCell<Table>>;
pub type TableWeak = Weak<RefCell<Table>>;
pub type IndexRef = Rc<RefCell<Index>>;
pub type FKeyRef = Rc<RefCell<FKey>>;
pub type TriggerRef = Rc<RefCell<Trigger>>;
pub type KeyInfoRef = Rc<RefCell<KeyInfo>>;
pub type CollSeqRef = Rc<CollSeq>;
pub type FuncDefRef = Rc<FuncDef>;
pub type BtreeP = Rc<RefCell<crate::btree::Btree>>;
pub type BtCursorP = Rc<RefCell<crate::btree::BtCursor>>;

/// Record comparison function (sqlite3VdbeFindCompare()): compares the
/// record `aKey` with the unpacked record.
pub type RecordCompare = fn(&[u8], &mut UnpackedRecord) -> i32;

/// Error message / name strings are byte strings (SQLite is 8-bit clean).
pub type ZStr = Vec<u8>;

// ---------------------------------------------------------------------------
// Callback types.  Application callbacks are reference-counted closures; any
// user data the C API passes as `void*` is captured by the closure instead.
// ---------------------------------------------------------------------------

/// Scalar function / xStep / xInverse: (context, argv).
pub type XSFunc = Rc<dyn Fn(&mut Context, &mut [Mem])>;
/// xFinal / xValue.
pub type XFinal = Rc<dyn Fn(&mut Context)>;
/// Collating function: compare two byte strings.
pub type XCmp = Rc<dyn Fn(&[u8], &[u8]) -> i32>;
/// Authorizer: (action, arg1, arg2, dbname, trigger/view) -> SQLITE_OK/DENY/IGNORE.
pub type XAuth = Rc<dyn Fn(i32, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>) -> i32>;
/// Busy handler: number of prior invocations -> nonzero to retry.
pub type XBusy = Rc<dyn Fn(i32) -> i32>;

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A token of SQL text.  `z` is `None` for a NULL token (C `z==0`);
/// otherwise the token is `src[off..off+n]`.  Two tokens can be compared by
/// offset when they come from the same source (pointer arithmetic in C).
#[derive(Clone, Default, Debug)]
pub struct Token {
    pub z: Option<Rc<[u8]>>,
    pub off: u32,
    pub n: u32,
}

impl Token {
    /// The text of the token (empty for a NULL token).
    pub fn text(&self) -> &[u8] {
        match &self.z {
            Some(s) => &s[self.off as usize..(self.off + self.n) as usize],
            None => b"",
        }
    }
    pub fn is_null(&self) -> bool {
        self.z.is_none()
    }
    /// Build a token that covers a whole stand-alone string
    /// (sqlite3TokenInit()).
    pub fn from_bytes(s: &[u8]) -> Token {
        Token {
            n: s.len() as u32,
            z: Some(Rc::from(s)),
            off: 0,
        }
    }
    /// Byte at offset `i` from the start of the token (may run past the end
    /// of the token, like C pointer indexing; returns 0 past the source).
    pub fn at(&self, i: isize) -> u8 {
        match &self.z {
            Some(s) => {
                let p = self.off as isize + i;
                if p >= 0 && (p as usize) < s.len() {
                    s[p as usize]
                } else {
                    0
                }
            }
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Functions and collations
// ---------------------------------------------------------------------------

/// User data attached to a function (the C `void *pUserData`).
#[derive(Clone, Default)]
pub enum FuncUserData {
    #[default]
    None,
    /// SQLITE_INT_TO_PTR(n)
    Int(i64),
    Any(Rc<dyn Any>),
}

impl FuncUserData {
    pub fn as_int(&self) -> i64 {
        match self {
            FuncUserData::Int(i) => *i,
            _ => 0,
        }
    }
}

pub struct FuncDestructor {
    pub xDestroy: Option<Rc<dyn Fn()>>,
}

/// Each SQL function is defined by an instance of this structure.
#[derive(Clone)]
pub struct FuncDef {
    pub nArg: i16,
    pub funcFlags: u32,
    pub pUserData: FuncUserData,
    pub xSFunc: Option<XSFunc>,
    pub xFinalize: Option<XFinal>,
    pub xValue: Option<XFinal>,
    pub xInverse: Option<XSFunc>,
    pub zName: ZStr,
    pub pDestructor: Option<Rc<FuncDestructor>>,
}

/// A collating sequence.
pub struct CollSeq {
    pub zName: ZStr,
    pub enc: u8,
    /// None means "no comparison function yet" (C `xCmp==0`).
    pub xCmp: RefCell<Option<XCmp>>,
    pub xDel: RefCell<Option<Rc<dyn Fn()>>>,
}

pub struct BusyHandler {
    pub xBusyHandler: Option<XBusy>,
    pub nBusy: i32,
}

// ---------------------------------------------------------------------------
// Schema objects
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Column {
    pub zCnName: ZStr,
    /// Declared type (C: stored after the name when COLFLAG_HASTYPE).
    pub zCnType: Option<ZStr>,
    /// Collating sequence name (C: stored after the type, COLFLAG_HASCOLL).
    pub zCnColl: Option<ZStr>,
    pub notNull: u8,
    pub eCType: u8,
    pub affinity: u8,
    pub szEst: u8,
    pub hName: u8,
    pub iDflt: u16,
    pub colFlags: u16,
}

#[derive(Clone, Default)]
pub struct TableTab {
    pub addColOffset: i32,
    /// Foreign keys whose child table is this table (C pFKey/pNextFrom list).
    pub pFKey: Vec<FKeyRef>,
    pub pDfltList: Tree<ExprListId>,
}

#[derive(Clone, Default)]
pub struct TableView {
    pub pSelect: Tree<SelectId>,
}

#[derive(Clone, Default)]
pub struct TableVtab {
    pub nArg: i32,
    pub azArg: Vec<ZStr>,
}

#[derive(Clone, Default)]
pub struct TableU {
    pub tab: TableTab,
    pub view: TableView,
    pub vtab: TableVtab,
}

/// The schema for each SQL table, virtual table, and view.
#[derive(Clone, Default)]
pub struct Table {
    pub zName: ZStr,
    pub aCol: Vec<Column>,
    /// Indices on this table, in the order of the C pIndex/pNext list.
    pub pIndex: Vec<IndexRef>,
    pub zColAff: Option<ZStr>,
    pub pCheck: Tree<ExprListId>,
    pub tnum: Pgno,
    pub nTabRef: u32,
    pub tabFlags: u32,
    pub iPKey: i16,
    /// Number of columns (kept in sync with aCol.len(); can be negative as
    /// a "view is being expanded" marker, as in C).
    pub nCol: i16,
    pub nNVCol: i16,
    pub nRowLogEst: LogEst,
    pub szTabRow: LogEst,
    pub keyConf: u8,
    pub eTabType: u8,
    pub u: TableU,
    /// Triggers on this table (C pTrigger/pNext list).
    pub pTrigger: Vec<TriggerRef>,
    pub pSchema: SchemaId,
    pub aHx: [u8; 16],
}

impl Table {
    pub fn IsView(&self) -> bool {
        self.eTabType == TABTYP_VIEW
    }
    pub fn IsOrdinaryTable(&self) -> bool {
        self.eTabType == TABTYP_NORM
    }
    pub fn IsVirtual(&self) -> bool {
        self.eTabType == TABTYP_VTAB
    }
    pub fn HasRowid(&self) -> bool {
        (self.tabFlags & TF_WithoutRowid) == 0
    }
    pub fn VisibleRowid(&self) -> bool {
        (self.tabFlags & TF_NoVisibleRowid) == 0
    }
}

#[inline]
pub fn IsHiddenColumn(c: &Column) -> bool {
    (c.colFlags & COLFLAG_HIDDEN) != 0
}
#[inline]
pub fn IsOrdinaryHiddenColumn(_c: &Column) -> bool {
    false
}

#[derive(Clone, Default)]
pub struct FKeyCol {
    pub iFrom: i32,
    pub zCol: Option<ZStr>,
}

/// A foreign key constraint.
#[derive(Clone, Default)]
pub struct FKey {
    pub pFrom: TableWeak,
    pub zTo: ZStr,
    pub nCol: i32,
    pub isDeferred: u8,
    pub aAction: [u8; 2],
    pub apTrigger: [Option<TriggerRef>; 2],
    pub aCol: Vec<FKeyCol>,
}

/// Information about the keys of an index or ORDER BY.
#[derive(Clone, Default)]
pub struct KeyInfo {
    pub enc: u8,
    pub nKeyField: u16,
    pub nAllField: u16,
    pub aSortFlags: Vec<u8>,
    pub aColl: Vec<Option<CollSeqRef>>,
}

/// An index record unpacked into an array of Mem values.
#[derive(Clone, Default)]
pub struct UnpackedRecord {
    pub pKeyInfo: Option<KeyInfoRef>,
    pub aMem: Vec<Mem>,
    /// u.z (string key for the fast-path comparator)
    pub u_z: Vec<u8>,
    /// u.i (integer key for the fast-path comparator)
    pub u_i: i64,
    pub n: i32,
    pub nField: u16,
    pub default_rc: i8,
    pub errCode: u8,
    pub r1: i8,
    pub r2: i8,
    pub eqSeen: u8,
}

/// The schema of an index.
#[derive(Clone, Default)]
pub struct Index {
    pub zName: ZStr,
    pub aiColumn: Vec<i16>,
    pub aiRowLogEst: Vec<LogEst>,
    pub pTable: TableWeak,
    pub zColAff: Option<ZStr>,
    pub pSchema: SchemaId,
    pub aSortOrder: Vec<u8>,
    /// Collation names (C azColl).
    pub azColl: Vec<ZStr>,
    pub pPartIdxWhere: Tree<ExprId>,
    pub aColExpr: Tree<ExprListId>,
    pub tnum: Pgno,
    pub szIdxRow: LogEst,
    pub nKeyCol: u16,
    pub nColumn: u16,
    pub onError: u8,
    pub idxType: u8,
    pub bUnordered: u8,
    pub uniqNotNull: u8,
    pub isResized: u8,
    pub isCovering: u8,
    pub noSkipScan: u8,
    pub hasStat1: u8,
    pub bNoQuery: u8,
    pub bAscKeyBug: u8,
    pub bHasVCol: u8,
    pub bHasExpr: u8,
    pub colNotIdxed: Bitmask,
}

impl Index {
    /// The table this index belongs to.
    pub fn table(&self) -> TableRef {
        self.pTable.upgrade().expect("index without table")
    }
    pub fn IsPrimaryKeyIndex(&self) -> bool {
        self.idxType == SQLITE_IDXTYPE_PRIMARYKEY
    }
    pub fn IsUniqueIndex(&self) -> bool {
        self.onError != OE_None
    }
}

/// A trigger.  Parse-tree members are `Tree`s: live (in db.arena) while the
/// trigger is being built, owned once it is part of a schema.
#[derive(Clone, Default)]
pub struct Trigger {
    pub zName: ZStr,
    pub table: ZStr,
    pub op: u8,
    pub tr_tm: u8,
    pub bReturning: u8,
    pub pWhen: Tree<ExprId>,
    pub pColumns: Tree<IdListId>,
    pub pSchema: SchemaId,
    pub pTabSchema: SchemaId,
    pub step_list: Tree<TriggerStepId>,
}

/// Identifies a Schema object.  Db::pSchema.id; sqlite3SchemaToIndex()
/// maps it back to an index into db.aDb.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct SchemaId(pub u32);

/// An instance of this structure stores the database schema.
#[derive(Default)]
pub struct Schema {
    pub id: SchemaId,
    pub schema_cookie: i32,
    pub iGeneration: i32,
    pub tblHash: Hash<TableRef>,
    pub idxHash: Hash<IndexRef>,
    pub trigHash: Hash<TriggerRef>,
    /// FKeys by name of the parent table (C pNextTo/pPrevTo lists).
    pub fkeyHash: Hash<Vec<FKeyRef>>,
    pub pSeqTab: Option<TableRef>,
    pub file_format: u8,
    pub enc: u8,
    pub schemaFlags: u16,
    pub cache_size: i32,
}

/// Each database file to be accessed by the system is an instance of this
/// structure.
pub struct Db {
    pub zDbSName: Option<ZStr>,
    pub pBt: Option<BtreeP>,
    pub safety_level: u8,
    pub bSyncSet: u8,
    pub pSchema: Schema,
}

#[derive(Clone, Default)]
pub struct Savepoint {
    pub zName: ZStr,
    pub nDeferredCons: i64,
    pub nDeferredImmCons: i64,
}

/// Information about an in-progress sqlite3_initialize/schema load.
#[derive(Clone, Default)]
pub struct sqlite3InitInfo {
    pub newTnum: Pgno,
    pub iDb: u8,
    pub busy: u8,
    pub orphanTrigger: u8,
    pub imposterTable: u8,
    pub reopenMemdb: u8,
    pub azInit: Vec<ZStr>,
}

/// A prepared statement slot in Sqlite3::aVdbe (the C db->pVdbe list).
pub struct VdbeSlot {
    /// The statement; `None` while it is being executed (taken out).
    pub v: Option<Box<Vdbe>>,
    /// Bumped each time the slot is reused, so stale handles are detected.
    pub generation: u32,
    /// Slot is in use (a statement exists, possibly taken out).
    pub used: bool,
    /// sqlite3ExpirePreparedStatements() value recorded while the
    /// statement was taken out; the effective `expired` of a statement is
    /// max(v.expired, slot.expired) and is merged back by stmt_put().
    pub expired: u8,
}

/// Handle on a prepared statement: an index into Sqlite3::aVdbe.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct StmtId {
    pub idx: u32,
    pub generation: u32,
}

/// Each database connection is an instance of the following structure.
pub struct Sqlite3 {
    // --- Rust-port state (see PORTING.md) ---
    /// Arena holding all parse-tree nodes of the parses in progress.
    pub arena: Arena,
    /// Stack of Parse objects; the last one is the current parse
    /// (C db->pParse).
    pub parses: Vec<Parse>,
    /// Prepared statements (C db->pVdbe list).
    pub aVdbe: Vec<VdbeSlot>,
    /// Built-in SQL functions (C sqlite3BuiltinFunctions).
    pub builtinFuncs: Rc<crate::callback::FuncDefHash>,
    /// Source of SchemaId values.
    pub nextSchemaId: u32,

    // --- sqlite3 struct ---
    pub pVfs: Rc<crate::os::Vfs>,
    pub pDfltColl: Option<CollSeqRef>,
    pub aDb: Vec<Db>,
    pub mDbFlags: u32,
    pub flags: u64,
    pub lastRowid: i64,
    pub szMmap: i64,
    pub nSchemaLock: u32,
    pub openFlags: u32,
    pub errCode: i32,
    pub errByteOffset: i32,
    pub errMask: i32,
    pub iSysErrno: i32,
    pub dbOptFlags: u32,
    pub enc: u8,
    pub autoCommit: u8,
    pub temp_store: u8,
    pub mallocFailed: u8,
    pub bBenignMalloc: u8,
    pub dfltLockMode: u8,
    pub nextAutovac: i8,
    pub suppressErr: u8,
    pub vtabOnConflict: u8,
    pub isTransactionSavepoint: u8,
    pub mTrace: u8,
    pub noSharedCache: u8,
    pub nSqlExec: u8,
    pub eOpenState: u8,
    pub nFpDigit: u8,
    pub nextPagesize: i32,
    pub nChange: i64,
    pub nTotalChange: i64,
    pub aLimit: [i32; SQLITE_N_LIMIT],
    pub nMaxSorterMmap: i32,
    pub init: sqlite3InitInfo,
    pub nVdbeActive: i32,
    pub nVdbeRead: i32,
    pub nVdbeWrite: i32,
    pub nVdbeExec: i32,
    pub nVDestroy: i32,
    pub nExtension: i32,
    /// sqlite3_trace_v2 callback: (mask, stmt sql / info) -> int
    pub xTrace: Option<Rc<dyn Fn(u32, &[u8]) -> i32>>,
    pub xCommitCallback: Option<Rc<dyn Fn() -> i32>>,
    pub xRollbackCallback: Option<Rc<dyn Fn()>>,
    /// (op, dbname, table, rowid)
    pub xUpdateCallback: Option<Rc<dyn Fn(i32, &[u8], &[u8], i64)>>,
    pub xAutovacPages: Option<Rc<dyn Fn(&[u8], u32, u32, u32) -> u32>>,
    /// (db, eTextRep, name)
    pub xCollNeeded: Option<Rc<dyn Fn(&mut Sqlite3, i32, &[u8])>>,
    pub pErr: Option<Box<Mem>>,
    pub isInterrupted: Rc<std::cell::Cell<i32>>,
    pub xAuth: Option<XAuth>,
    pub xProgress: Option<Rc<dyn Fn() -> i32>>,
    pub nProgressOps: u32,
    /// Application-defined functions (C db->aFunc).
    pub aFunc: Hash<Vec<FuncDefRef>>,
    /// Collating sequences: name -> [UTF8, UTF16LE, UTF16BE] (C aCollSeq).
    pub aCollSeq: Hash<[CollSeqRef; 3]>,
    pub busyHandler: Rc<RefCell<BusyHandler>>,
    /// Savepoints, most recent first (C pSavepoint list).
    pub pSavepoint: Vec<Savepoint>,
    pub nAnalysisLimit: i32,
    pub busyTimeout: i32,
    pub nSavepoint: i32,
    pub nStatement: i32,
    pub nDeferredCons: i64,
    pub nDeferredImmCons: i64,
    pub nSpill: u64,
}

impl Sqlite3 {
    /// The current Parse (C pParse).  Panics if no parse is in progress.
    #[inline]
    pub fn pp(&mut self) -> &mut Parse {
        self.parses.last_mut().expect("no parse in progress")
    }
    /// The current Parse, read only.
    #[inline]
    pub fn ppr(&self) -> &Parse {
        self.parses.last().expect("no parse in progress")
    }
    /// The Vdbe being built by the current parse (C pParse->pVdbe).
    /// Panics if there is none: call sqlite3GetVdbe() first.
    #[inline]
    pub fn v(&mut self) -> &mut Vdbe {
        self.pp().pVdbe.as_deref_mut().expect("no vdbe")
    }
    /// The toplevel parse of the current parse (sqlite3ParseToplevel()).
    pub fn toplevel(&mut self) -> &mut Parse {
        let i = self.ppr().iToplevel;
        &mut self.parses[i]
    }
    /// Expression node accessors.
    #[inline]
    pub fn ex(&self, e: ExprId) -> &Expr {
        self.arena.ex(e)
    }
    #[inline]
    pub fn exm(&mut self, e: ExprId) -> &mut Expr {
        self.arena.exm(e)
    }
    #[inline]
    pub fn el(&self, l: ExprListId) -> &ExprList {
        self.arena.el(l)
    }
    #[inline]
    pub fn elm(&mut self, l: ExprListId) -> &mut ExprList {
        self.arena.elm(l)
    }
    #[inline]
    pub fn sel(&self, s: SelectId) -> &Select {
        self.arena.sel(s)
    }
    #[inline]
    pub fn selm(&mut self, s: SelectId) -> &mut Select {
        self.arena.selm(s)
    }
    #[inline]
    pub fn src(&self, s: SrcListId) -> &SrcList {
        self.arena.src(s)
    }
    #[inline]
    pub fn srcm(&mut self, s: SrcListId) -> &mut SrcList {
        self.arena.srcm(s)
    }
    #[inline]
    pub fn item(&self, it: SrcItemId) -> &SrcItem {
        &self.arena.src(it.0).a[it.1 as usize]
    }
    #[inline]
    pub fn itemm(&mut self, it: SrcItemId) -> &mut SrcItem {
        &mut self.arena.srcm(it.0).a[it.1 as usize]
    }
    #[inline]
    pub fn idl(&self, l: IdListId) -> &IdList {
        self.arena.idl(l)
    }
    #[inline]
    pub fn idlm(&mut self, l: IdListId) -> &mut IdList {
        self.arena.idlm(l)
    }
    #[inline]
    pub fn win(&self, w: WindowId) -> &Window {
        self.arena.win(w)
    }
    #[inline]
    pub fn winm(&mut self, w: WindowId) -> &mut Window {
        self.arena.winm(w)
    }
    /// Index of the schema `id` in aDb (sqlite3SchemaToIndex()); -1000000 if
    /// not found (as in C for a NULL schema).
    pub fn schema_index(&self, id: SchemaId) -> i32 {
        for (i, d) in self.aDb.iter().enumerate() {
            if d.pSchema.id == id {
                return i as i32;
            }
        }
        -1000000
    }
    #[inline]
    pub fn nDb(&self) -> i32 {
        self.aDb.len() as i32
    }
    /// ENC(db)
    #[inline]
    pub fn ENC(&self) -> u8 {
        self.enc
    }
    #[inline]
    pub fn OptimizationDisabled(&self, mask: u32) -> bool {
        (self.dbOptFlags & mask) != 0
    }
    #[inline]
    pub fn OptimizationEnabled(&self, mask: u32) -> bool {
        (self.dbOptFlags & mask) == 0
    }
    /// DbHasProperty(db, i, p)
    pub fn DbHasProperty(&self, i: i32, p: u16) -> bool {
        (self.aDb[i as usize].pSchema.schemaFlags & p) == p
    }
    pub fn DbHasAnyProperty(&self, i: i32, p: u16) -> bool {
        (self.aDb[i as usize].pSchema.schemaFlags & p) != 0
    }
    pub fn DbSetProperty(&mut self, i: i32, p: u16) {
        self.aDb[i as usize].pSchema.schemaFlags |= p;
    }
    pub fn DbClearProperty(&mut self, i: i32, p: u16) {
        self.aDb[i as usize].pSchema.schemaFlags &= !p;
    }
    /// The parse tree of a Tree<> field, imported into the arena if owned.
    pub fn tree<I: ArenaId>(&mut self, t: &Tree<I>) -> I {
        t.get(&mut self.arena)
    }

    // --- prepared statement slots (the C db->pVdbe list) ---

    /// Register a new statement (sqlite3VdbeCreate links it into db->pVdbe).
    pub fn stmt_new(&mut self, v: Box<Vdbe>) -> StmtId {
        for (i, s) in self.aVdbe.iter_mut().enumerate() {
            if !s.used {
                s.used = true;
                s.generation = s.generation.wrapping_add(1);
                s.v = Some(v);
                s.expired = 0;
                return StmtId {
                    idx: i as u32,
                    generation: s.generation,
                };
            }
        }
        self.aVdbe.push(VdbeSlot {
            v: Some(v),
            generation: 1,
            used: true,
            expired: 0,
        });
        StmtId {
            idx: (self.aVdbe.len() - 1) as u32,
            generation: 1,
        }
    }
    /// True if `id` designates a live statement.
    pub fn stmt_valid(&self, id: StmtId) -> bool {
        match self.aVdbe.get(id.idx as usize) {
            Some(s) => s.used && s.generation == id.generation,
            None => false,
        }
    }
    /// Take a statement out of its slot to run it.
    pub fn stmt_take(&mut self, id: StmtId) -> Option<Box<Vdbe>> {
        if !self.stmt_valid(id) {
            return None;
        }
        self.aVdbe[id.idx as usize].v.take()
    }
    /// Put back a statement taken with stmt_take().
    pub fn stmt_put(&mut self, id: StmtId, mut v: Box<Vdbe>) {
        if !self.stmt_valid(id) {
            return;
        }
        let s = &mut self.aVdbe[id.idx as usize];
        if s.expired > v.expired {
            v.expired = s.expired;
        }
        s.expired = 0;
        s.v = Some(v);
    }
    /// Remove a statement (finalize): the slot becomes free.
    pub fn stmt_remove(&mut self, id: StmtId) -> Option<Box<Vdbe>> {
        if !self.stmt_valid(id) {
            return None;
        }
        let s = &mut self.aVdbe[id.idx as usize];
        s.used = false;
        s.expired = 0;
        s.v.take()
    }
    /// Run `f` on statement `id` (taking it out of its slot meanwhile).
    pub fn with_stmt<R>(
        &mut self,
        id: StmtId,
        f: impl FnOnce(&mut Sqlite3, &mut Vdbe) -> R,
    ) -> Option<R> {
        let mut v = self.stmt_take(id)?;
        let r = f(self, &mut v);
        self.stmt_put(id, v);
        Some(r)
    }
    /// Ids of all live statements, in creation-slot order.
    pub fn stmt_ids(&self) -> Vec<StmtId> {
        self.aVdbe
            .iter()
            .enumerate()
            .filter(|(_, s)| s.used)
            .map(|(i, s)| StmtId {
                idx: i as u32,
                generation: s.generation,
            })
            .collect()
    }
    /// Number of live statements (C: db->pVdbe != 0 test).
    pub fn stmt_count(&self) -> usize {
        self.aVdbe.iter().filter(|s| s.used).count()
    }
}

// ---------------------------------------------------------------------------
// Parse tree
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct ExprU {
    pub zToken: Option<ZStr>,
    pub iValue: i32,
}

#[derive(Clone, Copy, Default)]
pub struct ExprX {
    pub pList: ExprListId,
    pub pSelect: SelectId,
}

#[derive(Clone, Copy, Default)]
pub struct ExprW {
    pub iJoin: i32,
    pub iOfst: i32,
}

#[derive(Clone, Copy, Default)]
pub struct ExprYSub {
    pub iAddr: i32,
    pub regReturn: i32,
}

#[derive(Clone, Default)]
pub struct ExprY {
    pub pTab: Option<TableRef>,
    pub pWin: WindowId,
    pub nReg: i32,
    pub sub: ExprYSub,
}

/// Each node of an expression in the parse tree is an instance of this
/// structure.  The C unions are flattened into structs with the same
/// member names (e.g. `pExpr->u.zToken` is `e.u.zToken`).
#[derive(Clone, Default)]
pub struct Expr {
    pub op: u8,
    pub affExpr: u8,
    pub op2: u8,
    pub flags: u32,
    pub u: ExprU,
    pub pLeft: ExprId,
    pub pRight: ExprId,
    pub x: ExprX,
    pub nHeight: i32,
    pub iTable: i32,
    pub iColumn: ynVar,
    pub iAgg: i16,
    pub w: ExprW,
    pub pAggInfo: AggInfoId,
    pub y: ExprY,
}

impl Expr {
    #[inline]
    pub fn HasProperty(&self, p: u32) -> bool {
        (self.flags & p) != 0
    }
    #[inline]
    pub fn HasAllProperty(&self, p: u32) -> bool {
        (self.flags & p) == p
    }
    #[inline]
    pub fn SetProperty(&mut self, p: u32) {
        self.flags |= p;
    }
    #[inline]
    pub fn ClearProperty(&mut self, p: u32) {
        self.flags &= !p;
    }
    #[inline]
    pub fn AlwaysTrue(&self) -> bool {
        (self.flags & (EP_OuterON | EP_IsTrue)) == EP_IsTrue
    }
    #[inline]
    pub fn AlwaysFalse(&self) -> bool {
        (self.flags & (EP_OuterON | EP_IsFalse)) == EP_IsFalse
    }
    #[inline]
    pub fn UseUToken(&self) -> bool {
        (self.flags & EP_IntValue) == 0
    }
    #[inline]
    pub fn UseUValue(&self) -> bool {
        (self.flags & EP_IntValue) != 0
    }
    #[inline]
    pub fn UseWOfst(&self) -> bool {
        (self.flags & (EP_InnerON | EP_OuterON)) == 0
    }
    #[inline]
    pub fn UseWJoin(&self) -> bool {
        (self.flags & (EP_InnerON | EP_OuterON)) != 0
    }
    #[inline]
    pub fn UseXList(&self) -> bool {
        (self.flags & EP_xIsSelect) == 0
    }
    #[inline]
    pub fn UseXSelect(&self) -> bool {
        (self.flags & EP_xIsSelect) != 0
    }
    #[inline]
    pub fn UseYTab(&self) -> bool {
        (self.flags & (EP_WinFunc | EP_Subrtn)) == 0
    }
    #[inline]
    pub fn UseYWin(&self) -> bool {
        (self.flags & EP_WinFunc) != 0
    }
    #[inline]
    pub fn UseYSub(&self) -> bool {
        (self.flags & EP_Subrtn) != 0
    }
    /// The token text (u.zToken) or empty.
    pub fn token(&self) -> &[u8] {
        self.u.zToken.as_deref().unwrap_or(b"")
    }
}

#[derive(Clone, Copy, Default)]
pub struct ExprListItemFg {
    pub sortFlags: u8,
    pub eEName: u8,
    pub done: u8,
    pub reusable: u8,
    pub bSorterRef: u8,
    pub bNulls: u8,
    pub bUsed: u8,
    pub bUsingTerm: u8,
    pub bNoExpand: u8,
}

#[derive(Clone, Copy, Default)]
pub struct ExprListItemUX {
    pub iOrderByCol: u16,
    pub iAlias: u16,
}

#[derive(Clone, Copy, Default)]
pub struct ExprListItemU {
    pub x: ExprListItemUX,
    pub iConstExprReg: i32,
}

#[derive(Clone, Default)]
pub struct ExprListItem {
    pub pExpr: ExprId,
    pub zEName: Option<ZStr>,
    pub fg: ExprListItemFg,
    pub u: ExprListItemU,
}

/// A list of expressions.  nExpr is `a.len()`.
#[derive(Clone, Default)]
pub struct ExprList {
    pub a: Vec<ExprListItem>,
}

impl ExprList {
    #[inline]
    pub fn nExpr(&self) -> i32 {
        self.a.len() as i32
    }
}

#[derive(Clone, Default)]
pub struct IdListItem {
    pub zName: Option<ZStr>,
}

/// A list of identifiers.  nId is `a.len()`.
#[derive(Clone, Default)]
pub struct IdList {
    pub a: Vec<IdListItem>,
}

impl IdList {
    #[inline]
    pub fn nId(&self) -> i32 {
        self.a.len() as i32
    }
}

/// Details of the implementation of a subquery.
#[derive(Clone, Copy, Default)]
pub struct Subquery {
    pub pSelect: SelectId,
    pub addrFillSub: i32,
    pub regReturn: i32,
    pub regResult: i32,
}

#[derive(Clone, Copy, Default)]
pub struct SrcItemFg {
    pub jointype: u8,
    pub notIndexed: u8,
    pub isIndexedBy: u8,
    pub isSubquery: u8,
    pub isTabFunc: u8,
    pub isCorrelated: u8,
    pub isMaterialized: u8,
    pub viaCoroutine: u8,
    pub isRecursive: u8,
    pub fromDDL: u8,
    pub isCte: u8,
    pub notCte: u8,
    pub isUsing: u8,
    pub isOn: u8,
    pub isSynthUsing: u8,
    pub isNestedFrom: u8,
    pub rowidUsed: u8,
    pub fixedSchema: u8,
    pub hadSchema: u8,
    pub fromExists: u8,
}

#[derive(Clone, Default)]
pub struct SrcItemU1 {
    pub zIndexedBy: Option<ZStr>,
    pub pFuncArg: ExprListId,
    pub nRow: u32,
}

#[derive(Clone, Default)]
pub struct SrcItemU2 {
    pub pIBIndex: Option<IndexRef>,
    pub pCteUse: CteUseId,
}

#[derive(Clone, Copy, Default)]
pub struct SrcItemU3 {
    pub pOn: ExprId,
    pub pUsing: IdListId,
}

#[derive(Clone, Default)]
pub struct SrcItemU4 {
    pub pSchema: Option<SchemaId>,
    pub zDatabase: Option<ZStr>,
    pub pSubq: Option<Box<Subquery>>,
}

/// One term of a FROM clause.
#[derive(Clone, Default)]
pub struct SrcItem {
    pub zName: Option<ZStr>,
    pub zAlias: Option<ZStr>,
    pub pSTab: Option<TableRef>,
    pub fg: SrcItemFg,
    pub iCursor: i32,
    pub colUsed: Bitmask,
    pub u1: SrcItemU1,
    pub u2: SrcItemU2,
    pub u3: SrcItemU3,
    pub u4: SrcItemU4,
}

/// ON or USING clause.
#[derive(Clone, Copy, Default)]
pub struct OnOrUsing {
    pub pOn: ExprId,
    pub pUsing: IdListId,
}

/// The FROM clause.  nSrc is `a.len()`.
#[derive(Clone, Default)]
pub struct SrcList {
    pub a: Vec<SrcItem>,
}

impl SrcList {
    #[inline]
    pub fn nSrc(&self) -> i32 {
        self.a.len() as i32
    }
}

/// Reference to one SrcItem: (list, index).  C code that holds a
/// `SrcItem*` holds one of these instead.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SrcItemId(pub SrcListId, pub u32);

#[derive(Clone, Copy, Default)]
pub struct NameContextU {
    pub pEList: ExprListId,
    pub pAggInfo: AggInfoId,
    pub pUpsert: UpsertId,
    pub iBaseReg: i32,
}

/// A NameContext defines a context in which to resolve table and column
/// names.  Allocated in the arena (NcId) because they form a chain through
/// pNext.
#[derive(Clone, Copy, Default)]
pub struct NameContext {
    pub pSrcList: SrcListId,
    pub uNC: NameContextU,
    pub pNext: NcId,
    pub nRef: i32,
    pub nNcErr: i32,
    pub ncFlags: i32,
    pub nNestedSelect: u32,
    pub pWinSelect: SelectId,
}

/// An instance of this object describes a single UPSERT clause.
#[derive(Clone, Default)]
pub struct Upsert {
    pub pUpsertTarget: ExprListId,
    pub pUpsertTargetWhere: ExprId,
    pub pUpsertSet: ExprListId,
    pub pUpsertWhere: ExprId,
    pub pNextUpsert: UpsertId,
    pub isDoUpdate: u8,
    pub isDup: u8,
    pub pUpsertIdx: Option<IndexRef>,
    pub pUpsertSrc: SrcListId,
    pub regData: i32,
    pub iDataCur: i32,
    pub iIdxCur: i32,
}

/// A SELECT statement.
#[derive(Clone, Default)]
pub struct Select {
    pub op: u8,
    pub nSelectRow: LogEst,
    pub selFlags: u32,
    pub iLimit: i32,
    pub iOffset: i32,
    pub selId: u32,
    pub pEList: ExprListId,
    pub pSrc: SrcListId,
    pub pWhere: ExprId,
    pub pGroupBy: ExprListId,
    pub pHaving: ExprId,
    pub pOrderBy: ExprListId,
    pub pPrior: SelectId,
    pub pNext: SelectId,
    pub pLimit: ExprId,
    pub pWith: WithId,
    pub pWin: WindowId,
    pub pWinDefn: WindowId,
}

/// Where to send the results of a SELECT.
#[derive(Clone, Default)]
pub struct SelectDest {
    pub eDest: u8,
    pub iSDParm: i32,
    pub iSDParm2: i32,
    pub iSdst: i32,
    pub nSdst: i32,
    pub zAffSdst: Option<ZStr>,
    pub pOrderBy: ExprListId,
}

#[derive(Clone, Default)]
pub struct AggInfoCol {
    pub pTab: Option<TableRef>,
    pub pCExpr: ExprId,
    pub iTable: i32,
    pub iColumn: i32,
    pub iSorterColumn: i32,
}

#[derive(Clone, Default)]
pub struct AggInfoFunc {
    pub pFExpr: ExprId,
    pub pFunc: Option<FuncDefRef>,
    pub iDistinct: i32,
    pub iDistAddr: i32,
    pub iOBTab: i32,
    pub bOBPayload: u8,
    pub bOBUnique: u8,
    pub bUseSubtype: u8,
}

/// Information about aggregates.  nColumn is aCol.len(), nFunc is
/// aFunc.len().
#[derive(Clone, Default)]
pub struct AggInfo {
    pub directMode: u8,
    pub useSortingIdx: u8,
    pub nSortingColumn: u32,
    pub sortingIdx: i32,
    pub sortingIdxPTab: i32,
    pub iFirstReg: i32,
    pub pGroupBy: ExprListId,
    pub aCol: Vec<AggInfoCol>,
    pub nAccumulator: i32,
    pub aFunc: Vec<AggInfoFunc>,
    pub selId: u32,
}

impl AggInfo {
    #[inline]
    pub fn nColumn(&self) -> i32 {
        self.aCol.len() as i32
    }
    #[inline]
    pub fn nFunc(&self) -> i32 {
        self.aFunc.len() as i32
    }
    #[inline]
    pub fn AggInfoColumnReg(&self, i: i32) -> i32 {
        self.iFirstReg + i
    }
    #[inline]
    pub fn AggInfoFuncReg(&self, i: i32) -> i32 {
        self.iFirstReg + self.aCol.len() as i32 + i
    }
}

/// A common table expression.
#[derive(Clone, Default)]
pub struct Cte {
    pub zName: ZStr,
    pub pCols: ExprListId,
    pub pSelect: SelectId,
    pub zCteErr: Option<&'static str>,
    pub pUse: CteUseId,
    pub eM10d: u8,
}

/// The WITH clause.  nCte is `a.len()`.
#[derive(Clone, Default)]
pub struct With {
    pub bView: i32,
    pub pOuter: WithId,
    pub a: Vec<Cte>,
}

/// Information about CTE usage.
#[derive(Clone, Copy, Default)]
pub struct CteUse {
    pub nUse: i32,
    pub addrM9e: i32,
    pub regRtn: i32,
    pub iCur: i32,
    pub nRowEst: LogEst,
    pub eM10d: u8,
}

/// Where a Window is linked: the C `Window **ppThis` pointer.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum WinLink {
    #[default]
    None,
    /// ppThis == &pSelect->pWin
    Select(SelectId),
    /// ppThis == &pWindow->pNextWin
    Win(WindowId),
}

/// A window definition / window function invocation.
#[derive(Clone, Default)]
pub struct Window {
    pub zName: Option<ZStr>,
    pub zBase: Option<ZStr>,
    pub pPartition: ExprListId,
    pub pOrderBy: ExprListId,
    pub eFrmType: u8,
    pub eStart: u8,
    pub eEnd: u8,
    pub bImplicitFrame: u8,
    pub eExclude: u8,
    pub pStart: ExprId,
    pub pEnd: ExprId,
    pub ppThis: WinLink,
    pub pNextWin: WindowId,
    pub pFilter: ExprId,
    pub pWFunc: Option<FuncDefRef>,
    pub iEphCsr: i32,
    pub regAccum: i32,
    pub regResult: i32,
    pub csrApp: i32,
    pub regApp: i32,
    pub regPart: i32,
    pub pOwner: ExprId,
    pub nBufferCol: i32,
    pub iArgCol: i32,
    pub regOne: i32,
    pub regStartRowid: i32,
    pub regEndRowid: i32,
    pub bExprArgs: u8,
}

/// A trigger step (one statement of a trigger body).
#[derive(Clone, Default)]
pub struct TriggerStep {
    pub op: u8,
    pub orconf: u8,
    pub pTrig: Weak<RefCell<Trigger>>,
    pub pSelect: SelectId,
    pub pSrc: SrcListId,
    pub pWhere: ExprId,
    pub pExprList: ExprListId,
    pub pIdList: IdListId,
    pub pUpsert: UpsertId,
    pub zSpan: Option<ZStr>,
    pub pNext: TriggerStepId,
    pub pLast: TriggerStepId,
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct AutoincInfo {
    pub pTab: Option<TableRef>,
    pub iDb: i32,
    pub regCtr: i32,
}

/// A compiled trigger program, cached per Parse.
#[derive(Clone)]
pub struct TriggerPrg {
    pub pTrigger: TriggerRef,
    pub pProgram: Rc<SubProgram>,
    pub orconf: i32,
    pub aColmask: [u32; 2],
}

#[derive(Clone, Default)]
pub struct IndexedExpr {
    pub pExpr: ExprId,
    pub iDataCur: i32,
    pub iIdxCur: i32,
    pub iIdxCol: i32,
    pub bMaybeNullRow: u8,
    pub aff: u8,
}

#[derive(Clone, Default)]
pub struct TableLock {
    pub iDb: i32,
    pub iTab: Pgno,
    pub isWriteLock: u8,
    pub zLockName: ZStr,
}

#[derive(Clone, Default)]
pub struct ParseCr {
    pub addrCrTab: i32,
    pub regRowid: i32,
    pub regRoot: i32,
    pub constraintName: Token,
}

/// The RETURNING clause.
pub struct Returning {
    pub pReturnEL: ExprListId,
    pub retTrig: TriggerRef,
    pub iRetCur: i32,
    pub nRetCol: i32,
    pub iRetReg: i32,
    pub zName: ZStr,
}

#[derive(Default)]
pub struct ParseD {
    pub pReturning: Option<Box<Returning>>,
}

#[derive(Default)]
pub struct ParseU1 {
    pub cr: ParseCr,
    pub d: ParseD,
}

/// A map from a parse-tree node to the token that created it (ALTER TABLE
/// RENAME support).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameKey {
    Expr(ExprId),
    /// A Column's name token: (table pointer identity, column index).
    Col(usize, i32),
    /// Any other node, identified by an opaque integer.
    Other(usize),
}

#[derive(Clone)]
pub struct RenameToken {
    pub p: RenameKey,
    pub t: Token,
}

/// Name/number list for SQL parameters (C VList).
#[derive(Clone, Default, Debug)]
pub struct VList {
    /// (parameter number, name)
    pub a: Vec<(i32, ZStr)>,
}

/// An SQL parser context.
#[derive(Default)]
pub struct Parse {
    /// Index of this parse's toplevel parse in db.parses (pToplevel; equals
    /// this parse's own index when it is a toplevel parse).
    pub iToplevel: usize,
    pub zErrMsg: Option<ZStr>,
    pub pVdbe: Option<Box<Vdbe>>,
    pub rc: i32,
    pub nQueryLoop: LogEst,
    pub nested: u8,
    pub nTempReg: u8,
    pub isMultiWrite: u8,
    pub disableLookaside: u8,
    pub prepFlags: u8,
    pub withinRJSubrtn: u8,
    pub mSubrtnSig: u8,
    pub eTriggerOp: u8,
    pub eOrconf: u8,
    pub disableTriggers: u8,
    pub mayAbort: u8,
    pub hasCompound: u8,
    pub bReturning: u8,
    pub bHasExists: u8,
    pub colNamesSet: u8,
    pub bHasWith: u8,
    pub okConstFactor: u8,
    pub checkSchema: u8,
    pub usesAinc: u8,
    pub nRangeReg: i32,
    pub iRangeReg: i32,
    pub nErr: i32,
    pub nTab: i32,
    pub nMem: i32,
    pub szOpAlloc: i32,
    pub iSelfTab: i32,
    pub nNestSel: i32,
    /// Label counter (labels are negative numbers); the label table itself
    /// lives in Vdbe::aLabel.
    pub nLabel: i32,
    pub pConstExpr: ExprListId,
    pub pIdxEpr: Vec<IndexedExpr>,
    pub pIdxPartExpr: Vec<IndexedExpr>,
    pub writeMask: yDbMask,
    pub cookieMask: yDbMask,
    pub nMaxArg: i32,
    pub nSelect: i32,
    pub nProgressSteps: u32,
    pub pTriggerTab: Option<TableRef>,
    pub pTriggerPrg: Vec<TriggerPrg>,
    pub aTempReg: [i32; 8],
    pub sNameToken: Token,
    pub oldmask: u32,
    pub newmask: u32,
    pub u1: ParseU1,
    pub pAinc: Vec<AutoincInfo>,
    pub aTableLock: Vec<TableLock>,
    pub sLastToken: Token,
    pub nVar: ynVar,
    pub aVnbmc: [u64; 2],
    pub iPkSortOrder: u8,
    pub explain: u8,
    pub eParseMode: u8,
    pub nVtabLock: i32,
    pub nHeight: i32,
    pub addrExplain: i32,
    pub pVList: VList,
    pub pReprepare: Option<StmtId>,
    /// The SQL text being parsed and the offset of the unparsed tail.
    pub zSql: Option<Rc<[u8]>>,
    pub zTail: usize,
    pub pNewTable: Option<TableRef>,
    pub pNewIndex: Option<IndexRef>,
    pub pNewTrigger: Option<TriggerRef>,
    pub zAuthContext: Option<ZStr>,
    pub sArg: Token,
    pub apVtabLock: Vec<TableRef>,
    pub pWith: WithId,
    pub pRename: Vec<RenameToken>,
}

impl Parse {
    #[inline]
    pub fn IN_DECLARE_VTAB(&self) -> bool {
        self.eParseMode == PARSE_MODE_DECLARE_VTAB
    }
    #[inline]
    pub fn IN_RENAME_OBJECT(&self) -> bool {
        self.eParseMode >= PARSE_MODE_RENAME
    }
    #[inline]
    pub fn IN_SPECIAL_PARSE(&self) -> bool {
        self.eParseMode != PARSE_MODE_NORMAL
    }
    #[inline]
    pub fn ConstFactorOk(&self) -> bool {
        self.okConstFactor != 0
    }
}

/// Saved authorization context (sqlite3AuthContextPush/Pop).
#[derive(Clone, Default)]
pub struct AuthContext {
    pub zAuthContext: Option<ZStr>,
}

/// Used by sqlite3InitCallback().
#[derive(Clone, Default)]
pub struct InitData {
    pub iDb: i32,
    pub rc: i32,
    pub mInitFlags: u32,
    pub nInitRow: u32,
    pub mxPage: Pgno,
    pub pzErrMsg: Option<ZStr>,
}

/// Dynamic string accumulator (sqlite3_str / StrAccum).  nChar is
/// zText.len().
#[derive(Clone, Default)]
pub struct StrAccum {
    pub zText: Vec<u8>,
    /// Size limit of a fixed buffer (mxAlloc==0, e.g. sqlite3_snprintf);
    /// unused otherwise.
    pub nAlloc: u32,
    /// Maximum allowed string length (0: fixed buffer of nAlloc bytes)
    pub mxAlloc: u32,
    pub accError: u8,
    pub printfFlags: u8,
}

// ---------------------------------------------------------------------------
// Walker
// ---------------------------------------------------------------------------

pub type XExprCallback = fn(&mut Walker<'_>, ExprId) -> i32;
pub type XSelectCallback = fn(&mut Walker<'_>, SelectId) -> i32;
pub type XSelectCallback2 = fn(&mut Walker<'_>, SelectId);

/// The C `Walker.u` union, flattened.  File-local context structures go in
/// `any` (downcast with `w.u.any.as_mut().unwrap().downcast_mut::<T>()`).
#[derive(Default)]
pub struct WalkerU {
    pub pNC: NcId,
    pub n: i32,
    pub iCur: i32,
    pub sz: i32,
    pub pSrcList: SrcListId,
    pub aiCol: Vec<i32>,
    pub pGroupBy: ExprListId,
    pub pSelect: SelectId,
    pub pTab: Option<TableRef>,
    pub pSrcItem: SrcItemId,
    pub any: Option<Box<dyn Any>>,
}

/// Context pointer passed down through the tree-walk.
pub struct Walker<'a> {
    pub db: &'a mut Sqlite3,
    pub xExprCallback: Option<XExprCallback>,
    pub xSelectCallback: Option<XSelectCallback>,
    pub xSelectCallback2: Option<XSelectCallback2>,
    pub walkerDepth: i32,
    pub eCode: u16,
    pub mWFlags: u16,
    pub u: WalkerU,
}

impl<'a> Walker<'a> {
    pub fn new(db: &'a mut Sqlite3) -> Walker<'a> {
        Walker {
            db,
            xExprCallback: None,
            xSelectCallback: None,
            xSelectCallback2: None,
            walkerDepth: 0,
            eCode: 0,
            mWFlags: 0,
            u: WalkerU::default(),
        }
    }
}

/// A DbFixer is used to check that all names in a trigger/view/... refer to
/// the right database.
#[derive(Clone, Default)]
pub struct DbFixer {
    pub pSchema: SchemaId,
    pub bTemp: u8,
    pub zDb: ZStr,
    pub zType: &'static str,
    pub pName: Token,
}

// ---------------------------------------------------------------------------
// VDBE (vdbe.h / vdbeInt.h)
// ---------------------------------------------------------------------------

/// Signature of a subroutine (P4_SUBRTNSIG).
#[derive(Clone, Default, Debug)]
pub struct SubrtnSig {
    pub selId: i32,
    pub bComplete: u8,
    pub zAff: Option<ZStr>,
    pub iTable: i32,
    pub iAddr: i32,
    pub regReturn: i32,
}

/// The P4 operand of an opcode.  `VdbeOp::p4type` tells which C union
/// member is in use; this enum holds the value.
#[derive(Clone, Default)]
pub enum P4 {
    #[default]
    None,
    /// P4_INT32
    I(i32),
    /// P4_DYNAMIC / P4_STATIC / P4_TRANSIENT strings (p4.z)
    Z(ZStr),
    /// P4_FUNCDEF
    Func(FuncDefRef),
    /// P4_FUNCCTX: the function and the number of arguments.
    FuncCtx(FuncDefRef, u16),
    /// P4_COLLSEQ
    Coll(CollSeqRef),
    /// P4_MEM
    Mem(Box<Mem>),
    /// P4_KEYINFO
    KeyInfo(KeyInfoRef),
    /// P4_INTARRAY
    Ai(Rc<Vec<u32>>),
    /// P4_SUBPROGRAM
    Program(Rc<SubProgram>),
    /// P4_TABLE / P4_TABLEREF
    Tab(TableRef),
    /// P4_INDEX
    Idx(IndexRef),
    /// P4_SUBRTNSIG
    SubrtnSig(Box<SubrtnSig>),
}

/// A single instruction of the virtual machine.
#[derive(Clone, Default)]
pub struct VdbeOp {
    pub opcode: u8,
    pub p4type: i8,
    pub p5: u16,
    pub p1: i32,
    pub p2: i32,
    pub p3: i32,
    pub p4: P4,
    pub zComment: Option<ZStr>,
}

pub type Op = VdbeOp;

/// A sub-routine used to implement a trigger program.
#[derive(Default)]
pub struct SubProgram {
    pub aOp: Vec<VdbeOp>,
    pub nMem: i32,
    pub nCsr: i32,
    pub nOnce: i32,
    /// Identifies the trigger this program implements (C `void *token`).
    pub token: usize,
}

/// Item of a VdbeOpList (sqlite3VdbeAddOpList()).
#[derive(Clone, Copy)]
pub struct VdbeOpList {
    pub opcode: u8,
    pub p1: i8,
    pub p2: i8,
    pub p3: i8,
}

#[inline]
pub fn ADDR(x: i32) -> i32 {
    !x
}

#[inline]
pub fn INT32_TO_64(a: i32, b: i32) -> i64 {
    (((b as u32 as u64) << 32) | (a as u32 as u64)) as i64
}

/// The C union MemValue, flattened.
#[derive(Clone, Default)]
pub struct MemValue {
    pub r: f64,
    pub i: i64,
    pub nZero: i32,
    /// Pointer type name (sqlite3_result_pointer)
    pub zPType: Option<Rc<str>>,
    /// Aggregate function (MEM_Agg)
    pub pDef: Option<FuncDefRef>,
}

/// Internally, the vdbe manipulates nearly all SQL values as Mem
/// structures (sqlite3_value).
///
/// String and blob content is owned (`z`); `n` is the number of valid
/// bytes (z.len() >= n always holds).  The MEM_Dyn/MEM_Static/MEM_Ephem
/// flags are kept for fidelity but storage is always owned.
#[derive(Default)]
pub struct Mem {
    pub u: MemValue,
    pub z: Vec<u8>,
    pub n: i32,
    pub flags: u16,
    pub enc: u8,
    pub eSubtype: u8,
    pub uTemp: u32,
    /// Aggregate context (sqlite3_aggregate_context) for MEM_Agg cells.
    pub pAgg: Option<Box<dyn Any>>,
    /// Pointer value (sqlite3_result_pointer / sqlite3_bind_pointer).
    pub pPtr: Option<Rc<dyn Any>>,
}

impl Clone for Mem {
    fn clone(&self) -> Mem {
        Mem {
            u: self.u.clone(),
            z: self.z.clone(),
            n: self.n,
            flags: self.flags,
            enc: self.enc,
            eSubtype: self.eSubtype,
            uTemp: self.uTemp,
            pAgg: None,
            pPtr: self.pPtr.clone(),
        }
    }
}

impl Mem {
    /// The string/blob content: z[..n].
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.z[..(self.n.max(0) as usize).min(self.z.len())]
    }
    #[inline]
    pub fn VdbeMemDynamic(&self) -> bool {
        (self.flags & (MEM_Agg | MEM_Dyn)) != 0
    }
    #[inline]
    pub fn MemSetTypeFlag(&mut self, f: u16) {
        self.flags = (self.flags & !(MEM_TypeMask | MEM_Zero)) | f;
    }
    #[inline]
    pub fn MemNullNochng(&self) -> bool {
        (self.flags & MEM_TypeMask) == (MEM_Null | MEM_Zero) && self.n == 0 && self.u.nZero == 0
    }
}

/// Auxiliary data attached to a function argument (sqlite3_set_auxdata).
pub struct AuxData {
    pub iAuxOp: i32,
    pub iAuxArg: i32,
    pub pAux: Rc<dyn Any>,
}

/// The "context" argument for an installable function (sqlite3_context).
pub struct Context<'a> {
    pub db: &'a mut Sqlite3,
    /// The return value is stored here.
    pub pOut: &'a mut Mem,
    pub pFunc: FuncDefRef,
    /// Memory cell used to store aggregate context (None for scalars).
    pub pMem: Option<&'a mut Mem>,
    /// Auxiliary data of the statement (C pVdbe->pAuxData); None when the
    /// function runs outside a statement.
    pub pAuxData: Option<&'a mut Vec<AuxData>>,
    pub iOp: i32,
    pub isError: i32,
    pub enc: u8,
    pub skipFlag: u8,
    pub argc: u16,
    /// Error message set by sqlite3_result_error (copied into pOut too).
    pub zErr: Option<ZStr>,
}

/// A cursor is a pointer into a single BTree within a database file.
#[derive(Default)]
pub struct VdbeCursor {
    pub eCurType: u8,
    pub iDb: i8,
    pub nullRow: u8,
    pub deferredMoveto: u8,
    pub isTable: u8,
    pub isEphemeral: u8,
    pub useRandomRowid: u8,
    pub isOrdered: u8,
    pub noReuse: u8,
    pub colCache: u8,
    pub seekHit: u16,
    /// ub.pBtx: separate btree for an ephemeral table
    pub pBtx: Option<BtreeP>,
    /// ub.aAltMap: mapping from table to index column numbers
    pub aAltMap: Vec<u32>,
    pub seqCount: i64,
    pub cacheStatus: u32,
    pub seekResult: i32,
    /// pAltCursor: index (in apCsr) of the associated index cursor, or -1
    pub pAltCursor: i32,
    /// uc.pCursor
    pub pCursor: Option<BtCursorP>,
    /// uc.pSorter
    pub pSorter: Option<Box<crate::vdbesort::VdbeSorter>>,
    pub pKeyInfo: Option<KeyInfoRef>,
    pub iHdrOffset: u32,
    pub pgnoRoot: Pgno,
    pub nField: i16,
    pub nHdrParsed: u16,
    pub movetoTarget: i64,
    /// Cell offsets (C aOffset == &aType[nField])
    pub aOffset: Vec<u32>,
    /// Record data of the current row (C aRow; owned copy of the local
    /// payload).
    pub aRow: Vec<u8>,
    pub payloadSize: u32,
    pub szRow: u32,
    pub pCache: Option<Box<VdbeTxtBlbCache>>,
    /// Pseudo-table cursor: register holding the record (C uses seekResult)
    pub aType: Vec<u32>,
}

#[derive(Clone, Default)]
pub struct VdbeTxtBlbCache {
    pub pCValue: Vec<u8>,
    pub iOffset: i64,
    pub iCol: i32,
    pub cacheStatus: u32,
    pub colCacheCtr: u32,
}

/// Scan status for EXPLAIN QUERY PLAN / stmt_scanstatus.
#[derive(Clone, Default)]
pub struct ScanStatus {
    pub addrExplain: i32,
    pub aAddrRange: [i32; 6],
    pub addrLoop: i32,
    pub addrVisit: i32,
    pub iSelectID: i32,
    pub nEst: LogEst,
    pub zName: Option<ZStr>,
}

/// An instance of the virtual machine (a prepared statement).
#[derive(Default)]
pub struct Vdbe {
    pub nVar: ynVar,
    pub nMem: i32,
    pub nCursor: i32,
    pub cacheCtr: u32,
    pub pc: i32,
    pub rc: i32,
    pub nChange: i64,
    pub iStatement: i32,
    pub iCurrentTime: i64,
    pub nFkConstraint: i64,
    pub nStmtDefCons: i64,
    pub nStmtDefImmCons: i64,
    pub aMem: Vec<Mem>,
    pub apCsr: Vec<Option<Box<VdbeCursor>>>,
    pub aVar: Vec<Mem>,
    pub aOp: Vec<VdbeOp>,
    /// Label table (C pParse->aLabel): aLabel[ADDR(x)] is the address of
    /// label x, or -1 if unresolved.
    pub aLabel: Vec<i32>,
    pub aColName: Vec<Mem>,
    /// Index of the first register of the current result row (C
    /// pResultRow), or -1.
    pub iResultRow: i32,
    pub zErrMsg: Option<ZStr>,
    pub pVList: VList,
    pub startTime: i64,
    pub nResColumn: u16,
    pub nResAlloc: u16,
    pub errorAction: u8,
    pub minWriteFileFormat: u8,
    pub prepFlags: u8,
    pub eVdbeState: u8,
    pub expired: u8,
    pub explain: u8,
    pub changeCntOn: u8,
    pub usesStmtJournal: u8,
    pub readOnly: u8,
    pub bIsReader: u8,
    pub haveEqpOps: u8,
    pub btreeMask: yDbMask,
    pub lockMask: yDbMask,
    pub aCounter: [u32; 9],
    pub zSql: Option<ZStr>,
    pub pFrame: Option<Box<crate::vdbe::VdbeFrame>>,
    pub nFrame: i32,
    pub expmask: u32,
    pub smimask: u32,
    /// Sub-programs used by this statement (C pProgram list)
    pub pProgram: Vec<Rc<SubProgram>>,
    pub pAuxData: Vec<AuxData>,
    pub aScan: Vec<ScanStatus>,
    /// aOnce flags of the main program
    pub aOnce: Vec<u8>,
}
