//! Port of vtab.c: the virtual table machinery (module registry, VTable
//! objects, CREATE VIRTUAL TABLE, sqlite3_declare_vtab(), transactions,
//! eponymous virtual tables).
//!
//! # Rust design
//!
//! * A module is an `Rc<dyn VtabModule>` (C `sqlite3_module*` + `pAux`:
//!   the implementation object captures its auxiliary data).  The
//!   connection's registry `db.aModule` maps names to [`ModuleRef`]s.
//! * A virtual table instance (C `sqlite3_vtab*`) is a `Box<dyn Vtab>`
//!   shared as [`VtabRef`]; the per-connection [`VTable`] object holds it.
//!   Without shared cache every Table belongs to a single connection, so
//!   `Table::u.vtab.p` holds at most one VTable (no `VTable.db` field).
//! * A cursor (C `sqlite3_vtab_cursor*`) is a `Box<dyn VtabCursor>` stored
//!   in the VDBE cursor (`VdbeCursor::pVCur`), next to the VTable it
//!   belongs to (`VdbeCursor::pVCurTab`, C `pVCur->pVtab`).
//! * C `pVtab->nRef` (number of open cursors) is `VTable::nCursor`.

#![allow(unused_variables, unused_mut, dead_code, unused_imports)]

use crate::int::*;
use crate::printf::PfArg;
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// The virtual table interface (sqlite3_module / sqlite3_vtab /
// sqlite3_vtab_cursor / sqlite3_index_info) as Rust traits.  This is the
// contract between the vtab machinery and module implementations (pragma
// eponymous tables, json_each/json_tree, application modules).
//
// Return values are SQLite result codes; error messages go into the
// `zErrMsg` of the vtab (C pVtab->zErrMsg) via `VtabBase`.
// ---------------------------------------------------------------------------

/// struct sqlite3_index_constraint
#[derive(Clone, Copy, Default, Debug)]
pub struct IndexConstraint {
    pub iColumn: i32,
    pub op: u8,
    pub usable: u8,
    pub iTermOffset: i32,
}

/// struct sqlite3_index_orderby
#[derive(Clone, Copy, Default, Debug)]
pub struct IndexOrderBy {
    pub iColumn: i32,
    pub desc: u8,
}

/// struct sqlite3_index_constraint_usage
#[derive(Clone, Copy, Default, Debug)]
pub struct IndexConstraintUsage {
    pub argvIndex: i32,
    pub omit: u8,
}

/// The HiddenIndexInfo of where.c: the private part of an
/// sqlite3_index_info used by sqlite3_vtab_collation(), sqlite3_vtab_in(),
/// sqlite3_vtab_rhs_value() and sqlite3_vtab_distinct().
#[derive(Clone, Default)]
pub struct HiddenIndexInfo {
    /// Value to return from sqlite3_vtab_distinct()
    pub eDistinct: i32,
    /// Mask of terms that are <col> IN (...)
    pub mIn: u32,
    /// Terms that vtab will handle as <col> IN (...)
    pub mHandleIn: u32,
    /// The expression of the WHERE term of each constraint (C
    /// termFromWhereClause(pHidden->pWC, iTermOffset)->pExpr); valid in
    /// db.arena while xBestIndex runs.
    pub aExpr: Vec<ExprId>,
    /// RHS values for constraints (computed on demand).
    pub aRhs: Vec<Option<Box<Mem>>>,
}

impl std::fmt::Debug for HiddenIndexInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiddenIndexInfo")
            .field("eDistinct", &self.eDistinct)
            .field("mIn", &self.mIn)
            .field("mHandleIn", &self.mHandleIn)
            .finish()
    }
}

/// sqlite3_index_info (nConstraint/nOrderBy are the Vec lengths).
#[derive(Clone, Default, Debug)]
pub struct IndexInfo {
    pub aConstraint: Vec<IndexConstraint>,
    pub aOrderBy: Vec<IndexOrderBy>,
    pub aConstraintUsage: Vec<IndexConstraintUsage>,
    pub idxNum: i32,
    pub idxStr: Option<ZStr>,
    /// C needToFreeIdxStr (the string is always owned in Rust; kept for
    /// fidelity of WhereLoop.u.vtab.needFree).
    pub needToFreeIdxStr: i32,
    pub orderByConsumed: i32,
    pub estimatedCost: f64,
    pub estimatedRows: i64,
    pub idxFlags: i32,
    pub colUsed: u64,
    /// Private planner data (C HiddenIndexInfo, &pIdxInfo[1]).
    pub hidden: HiddenIndexInfo,
}

impl IndexInfo {
    #[inline]
    pub fn nConstraint(&self) -> i32 {
        self.aConstraint.len() as i32
    }
    #[inline]
    pub fn nOrderBy(&self) -> i32 {
        self.aOrderBy.len() as i32
    }
}

/// A virtual table instance (sqlite3_vtab).  Optional methods have default
/// implementations equivalent to a NULL function pointer in C; the
/// `has_*` methods report whether the C pointer would be non-NULL.
pub trait Vtab {
    fn xBestIndex(&mut self, db: &mut Sqlite3, info: &mut IndexInfo) -> i32;
    fn xDisconnect(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        SQLITE_OK
    }
    fn xDestroy(&mut self, db: &mut Sqlite3) -> i32 {
        self.xDisconnect(db)
    }
    fn xOpen(&mut self, db: &mut Sqlite3) -> Result<Box<dyn VtabCursor>, i32>;
    fn has_xUpdate(&self) -> bool {
        false
    }
    /// argv as in C (argv[0] = old rowid, argv[1] = new rowid, ...).
    fn xUpdate(&mut self, db: &mut Sqlite3, argv: &mut [Mem], pRowid: &mut i64) -> i32 {
        let _ = (db, argv, pRowid);
        SQLITE_READONLY
    }
    fn has_xBegin(&self) -> bool {
        false
    }
    fn xBegin(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        SQLITE_OK
    }
    fn xSync(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        SQLITE_OK
    }
    fn xCommit(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        SQLITE_OK
    }
    fn xRollback(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        SQLITE_OK
    }
    /// xFindFunction: Some((score, function, user data)) to overload.
    fn xFindFunction(&mut self, nArg: i32, zName: &[u8]) -> Option<(i32, XSFunc, FuncUserData)> {
        let _ = (nArg, zName);
        None
    }
    fn has_xRename(&self) -> bool {
        false
    }
    fn xRename(&mut self, db: &mut Sqlite3, zNew: &[u8]) -> i32 {
        let _ = (db, zNew);
        SQLITE_OK
    }
    /// Whether the C xSavepoint/xRelease/xRollbackTo pointers are set
    /// (only consulted for modules with iVersion>=2).
    fn has_xSavepoint(&self) -> bool {
        true
    }
    fn xSavepoint(&mut self, db: &mut Sqlite3, i: i32) -> i32 {
        let _ = (db, i);
        SQLITE_OK
    }
    fn xRelease(&mut self, db: &mut Sqlite3, i: i32) -> i32 {
        let _ = (db, i);
        SQLITE_OK
    }
    fn xRollbackTo(&mut self, db: &mut Sqlite3, i: i32) -> i32 {
        let _ = (db, i);
        SQLITE_OK
    }
    fn has_xIntegrity(&self) -> bool {
        false
    }
    fn xIntegrity(
        &mut self,
        db: &mut Sqlite3,
        zSchema: &[u8],
        zTab: &[u8],
        mFlags: i32,
        pzErr: &mut Option<ZStr>,
    ) -> i32 {
        let _ = (db, zSchema, zTab, mFlags, pzErr);
        SQLITE_OK
    }
    /// C pVtab->zErrMsg
    fn zErrMsg(&mut self) -> &mut Option<ZStr>;
}

/// A cursor on a virtual table (sqlite3_vtab_cursor).
pub trait VtabCursor {
    fn xFilter(
        &mut self,
        db: &mut Sqlite3,
        idxNum: i32,
        idxStr: Option<&[u8]>,
        argv: &mut [Mem],
    ) -> i32;
    fn xNext(&mut self, db: &mut Sqlite3) -> i32;
    fn xEof(&mut self) -> bool;
    fn xColumn(&mut self, ctx: &mut Context, i: i32) -> i32;
    fn xRowid(&mut self, pRowid: &mut i64) -> i32;
    fn xClose(&mut self) -> i32 {
        SQLITE_OK
    }
    /// Rust extension: xClose with access to the connection (for cursors
    /// that own prepared statements).  The VDBE calls this method; the
    /// default forwards to xClose().
    fn xCloseDb(&mut self, db: &mut Sqlite3) -> i32 {
        let _ = db;
        self.xClose()
    }
    /// Rust extension: a cursor cannot reach its sqlite3_vtab, so a cursor
    /// method that would set C `pCur->pVtab->zErrMsg` stores the message
    /// here instead.  The VDBE imports it (before the vtab's own message)
    /// after each cursor method call.
    fn zErrMsg(&mut self) -> Option<&mut Option<ZStr>> {
        None
    }
}

/// A virtual table module (sqlite3_module).  `xCreate` returning None
/// means the C xCreate pointer is NULL (eponymous-only module); a module
/// whose xCreate is the same function as xConnect sets `create_is_connect`.
pub trait VtabModule {
    fn iVersion(&self) -> i32 {
        1
    }
    /// argv: module name, database name, table name, then the arguments.
    /// The implementation must call sqlite3_declare_vtab(db, ...).
    fn xConnect(
        &self,
        db: &mut Sqlite3,
        argv: &[&[u8]],
        pzErr: &mut Option<ZStr>,
    ) -> Result<Box<dyn Vtab>, i32>;
    fn has_xCreate(&self) -> bool {
        true
    }
    fn create_is_connect(&self) -> bool {
        true
    }
    fn xCreate(
        &self,
        db: &mut Sqlite3,
        argv: &[&[u8]],
        pzErr: &mut Option<ZStr>,
    ) -> Result<Box<dyn Vtab>, i32> {
        self.xConnect(db, argv, pzErr)
    }
    /// Whether the C xDestroy pointer is non-NULL (sqlite3VtabCallCreate()
    /// refuses modules without one).
    fn has_xDestroy(&self) -> bool {
        self.has_xCreate()
    }
    /// Whether the C xUpdate pointer of the module is non-NULL (consulted
    /// by sqlite3_declare_vtab() before any instance exists; instances
    /// report it through Vtab::has_xUpdate()).
    fn has_xUpdate(&self) -> bool {
        false
    }
    /// Whether the C xShadowName pointer is non-NULL.
    fn has_xShadowName(&self) -> bool {
        false
    }
    fn xShadowName(&self, zName: &[u8]) -> bool {
        let _ = zName;
        false
    }
}

// ---------------------------------------------------------------------------
// vtab.c
// ---------------------------------------------------------------------------

/// C: `Module * sqlite3VtabCreateModule( sqlite3 *db, const char *zName, const sqlite3_module *pModule, void *pAux, void (*xDestroy)(void *) )`
///
/// Construct and install a Module object for a virtual table.  If there
/// already exists a module with zName, replace it with the new one.  If
/// pModule==None, then delete the module zName if it exists.
///
/// Rust: pAux is captured by the module object; xDestroy is the module
/// destructor closure.
pub fn sqlite3VtabCreateModule(
    db: &mut Sqlite3,
    zName: &[u8],
    pModule: Option<Rc<dyn VtabModule>>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> Option<ModuleRef> {
    let pMod = pModule.map(|m| {
        Rc::new(RefCell::new(Module {
            pModule: m,
            zName: zName.to_vec(),
            nRefModule: 1,
            xDestroy,
            pEpoTab: None,
        }))
    });
    let pDel = match &pMod {
        Some(m) => db.aModule.insert(zName, m.clone()),
        None => db.aModule.remove(zName),
    };
    if let Some(pDel) = pDel {
        sqlite3VtabEponymousTableClear(db, &pDel);
        sqlite3VtabModuleUnref(db, &pDel);
    }
    pMod
}

/// The actual function that does the work of creating a new module.
/// This function implements the sqlite3_create_module() and
/// sqlite3_create_module_v2() interfaces.
fn createModule(
    db: &mut Sqlite3,
    zName: &[u8],
    pModule: Option<Rc<dyn VtabModule>>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let _ = sqlite3VtabCreateModule(db, zName, pModule, xDestroy);
    rc = crate::mainapi::sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_create_module( sqlite3 *db, const char *zName, const sqlite3_module *pModule, void *pAux )`
///
/// External API function used to create a new virtual-table module.
pub fn sqlite3_create_module(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    pModule: Option<Rc<dyn VtabModule>>,
) -> i32 {
    let Some(zName) = zName else {
        return SQLITE_MISUSE;
    };
    createModule(db, zName, pModule, None)
}

/// C: `int sqlite3_create_module_v2( sqlite3 *db, const char *zName, const sqlite3_module *pModule, void *pAux, void (*xDestroy)(void *) )`
///
/// External API function used to create a new virtual-table module.
pub fn sqlite3_create_module_v2(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    pModule: Option<Rc<dyn VtabModule>>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> i32 {
    let Some(zName) = zName else {
        return SQLITE_MISUSE;
    };
    createModule(db, zName, pModule, xDestroy)
}

/// C: `int sqlite3_drop_modules(sqlite3 *db, const char** azNames)`
///
/// External API to drop all virtual-table modules, except those named on
/// the azNames list.
pub fn sqlite3_drop_modules(db: &mut Sqlite3, azNames: Option<&[&[u8]]>) -> i32 {
    let names: Vec<ZStr> = db
        .aModule
        .iter()
        .map(|(_, m)| m.borrow().zName.clone())
        .collect();
    for zName in names {
        if let Some(az) = azNames {
            if az.iter().any(|n| *n == &zName[..]) {
                continue;
            }
        }
        createModule(db, &zName, None, None);
    }
    SQLITE_OK
}

/// C: `void sqlite3VtabModuleUnref(sqlite3 *db, Module *pMod)`
///
/// Decrement the reference count on a Module object.  Destroy the module
/// when the reference count reaches zero.
pub fn sqlite3VtabModuleUnref(db: &mut Sqlite3, pMod: &ModuleRef) {
    let (n, xDestroy) = {
        let mut m = pMod.borrow_mut();
        debug_assert!(m.nRefModule > 0);
        m.nRefModule -= 1;
        (m.nRefModule, m.xDestroy.clone())
    };
    if n == 0 {
        if let Some(x) = xDestroy {
            x();
        }
        debug_assert!(pMod.borrow().pEpoTab.is_none());
    }
}

/// C: `void sqlite3VtabLock(VTable *pVTab)`
///
/// Lock the virtual table so that it cannot be disconnected.  Locks nest.
pub fn sqlite3VtabLock(pVTab: &VTableRef) {
    pVTab.borrow_mut().nRef += 1;
}

/// C: `VTable * sqlite3GetVTable(sqlite3 *db, Table *pTab)`
///
/// pTab is a virtual table.  Return the VTable object used by connection
/// db to access this virtual table, if one has been created.
pub fn sqlite3GetVTable(db: &Sqlite3, pTab: &TableRef) -> Option<VTableRef> {
    let t = pTab.borrow();
    debug_assert!(t.IsVirtual());
    t.u.vtab.p.first().cloned()
}

/// The sqlite3_vtab of the VTable of pTab (C
/// `sqlite3GetVTable(db, pTab)->pVtab`).
pub fn sqlite3GetVtab(db: &Sqlite3, pTab: &TableRef) -> Option<VtabRef> {
    sqlite3GetVTable(db, pTab).and_then(|v| v.borrow().pVtab.clone())
}

/// The P4_VTAB operand for virtual table pTab (C
/// `(const char*)sqlite3GetVTable(db, pTab)`; P4::None for C NULL).
pub fn sqlite3VtabP4(db: &Sqlite3, pTab: &TableRef) -> P4 {
    match sqlite3GetVTable(db, pTab) {
        Some(v) => P4::Vtab(v),
        None => P4::None,
    }
}

/// C: `void sqlite3VtabUnlock(VTable *pVTab)`
///
/// Decrement the ref-count on a virtual table object. When the ref-count
/// reaches zero, call the xDisconnect() method to delete the object.
pub fn sqlite3VtabUnlock(db: &mut Sqlite3, pVTab: &VTableRef) {
    let n = {
        let mut v = pVTab.borrow_mut();
        debug_assert!(v.nRef > 0);
        v.nRef -= 1;
        v.nRef
    };
    if n == 0 {
        let (p, pMod) = {
            let mut v = pVTab.borrow_mut();
            (v.pVtab.take(), v.pMod.clone())
        };
        if let Some(p) = p {
            p.borrow_mut().xDisconnect(db);
        }
        sqlite3VtabModuleUnref(db, &pMod);
    }
}

/// Table p is a virtual table.  This function moves all elements in the
/// p->u.vtab.p list to the sqlite3.pDisconnect list to be disconnected at
/// the next opportunity.  Except, if `keep` (C: db!=0), then the entry of
/// connection db is left in the list and returned.
fn vtabDisconnectAll(db: &mut Sqlite3, keep: bool, p: &TableRef) -> Option<VTableRef> {
    let list = {
        let mut t = p.borrow_mut();
        debug_assert!(t.IsVirtual());
        std::mem::take(&mut t.u.vtab.p)
    };
    let mut pRet = None;
    for pVTable in list {
        if keep && pRet.is_none() {
            p.borrow_mut().u.vtab.p = vec![pVTable.clone()];
            pRet = Some(pVTable);
        } else {
            // C: pVTable->pNext = db2->pDisconnect; db2->pDisconnect = pVTable;
            db.pDisconnect.push(pVTable);
        }
    }
    debug_assert!(!keep || pRet.is_some());
    pRet
}

/// C: `void sqlite3VtabDisconnect(sqlite3 *db, Table *p)`
///
/// Table *p is a virtual table.  This function removes the VTable object
/// for table *p associated with database connection db from the list in
/// p->pVTab.  It also decrements the VTable ref count.
pub fn sqlite3VtabDisconnect(db: &mut Sqlite3, p: &TableRef) {
    let pVTab = {
        let mut t = p.borrow_mut();
        debug_assert!(t.IsVirtual());
        if t.u.vtab.p.is_empty() {
            None
        } else {
            Some(t.u.vtab.p.remove(0))
        }
    };
    if let Some(pVTab) = pVTab {
        sqlite3VtabUnlock(db, &pVTab);
    }
}

/// C: `void sqlite3VtabUnlockList(sqlite3 *db)`
///
/// Disconnect all the virtual table objects in the sqlite3.pDisconnect
/// list.
pub fn sqlite3VtabUnlockList(db: &mut Sqlite3) {
    if !db.pDisconnect.is_empty() {
        let list = std::mem::take(&mut db.pDisconnect);
        // The C list is head-first: the most recently added entry first.
        for p in list.iter().rev() {
            sqlite3VtabUnlock(db, p);
        }
    }
}

/// C: `void sqlite3VtabClear(sqlite3 *db, Table *p)`
///
/// Clear any and all virtual-table information from the Table record.
/// This routine is called, for example, just before deleting the Table
/// record.
pub fn sqlite3VtabClear(db: &mut Sqlite3, p: &TableRef) {
    debug_assert!(p.borrow().IsVirtual());
    vtabDisconnectAll(db, false, p);
}

/// Add a new module argument to pTable->u.vtab.azArg[].
fn addModuleArgument(db: &mut Sqlite3, pTable: &TableRef, zArg: ZStr) {
    let (nArg, zName) = {
        let t = pTable.borrow();
        debug_assert!(t.IsVirtual());
        (t.u.vtab.nArg, t.zName.clone())
    };
    if nArg + 3 >= db.aLimit[SQLITE_LIMIT_COLUMN as usize] {
        crate::errmsg!(db, "too many columns on %s", &zName);
    }
    let mut t = pTable.borrow_mut();
    t.u.vtab.azArg.push(zArg);
    t.u.vtab.nArg += 1;
}

/// C: `void sqlite3VtabBeginParse( Parse *pParse, Token *pName1, Token *pName2, Token *pModuleName, int ifNotExists )`
///
/// The parser calls this routine when it first sees a CREATE VIRTUAL TABLE
/// statement.  The module name has been parsed, but the optional list of
/// parameters that follow the module name are still pending.
pub fn sqlite3VtabBeginParse(
    db: &mut Sqlite3,
    pName1: &Token,
    pName2: &Token,
    pModuleName: &Token,
    ifNotExists: i32,
) {
    crate::build::sqlite3StartTable(db, pName1, pName2, 0, 0, 1, ifNotExists);
    let Some(pTable) = db.ppr().pNewTable.clone() else {
        return;
    };
    debug_assert!(pTable.borrow().pIndex.is_empty());
    pTable.borrow_mut().eTabType = TABTYP_VTAB;

    debug_assert!(pTable.borrow().u.vtab.nArg == 0);
    let zMod = crate::build::sqlite3NameFromToken(db, pModuleName).unwrap_or_default();
    addModuleArgument(db, &pTable, zMod);
    addModuleArgument(db, &pTable, Vec::new());
    let zName = pTable.borrow().zName.clone();
    addModuleArgument(db, &pTable, zName);
    {
        let p = db.pp();
        p.sNameToken.n = pModuleName.off + pModuleName.n - p.sNameToken.off;
    }

    // Creating a virtual table invokes the authorization callback twice.
    // The first invocation, to obtain permission to INSERT a row into the
    // sqlite_schema table, has already been made by sqlite3StartTable().
    // The second call, to obtain permission to create the table, is made
    // now.
    let (pSchema, zName, zArg0) = {
        let t = pTable.borrow();
        (t.pSchema, t.zName.clone(), t.u.vtab.azArg.first().cloned())
    };
    if let Some(zArg0) = zArg0 {
        let iDb = db.schema_index(pSchema);
        debug_assert!(iDb >= 0);
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        crate::auth::sqlite3AuthCheck(
            db,
            SQLITE_CREATE_VTABLE,
            Some(&zName),
            Some(&zArg0),
            zDb.as_deref(),
        );
    }
}

/// This routine takes the module argument that has been accumulating in
/// pParse->zArg[] and appends it to the list of arguments on the virtual
/// table currently under construction in pParse->pTable.
fn addArgumentToVtab(db: &mut Sqlite3) {
    let (z, pNew) = {
        let p = db.ppr();
        (
            if p.sArg.z.is_some() {
                Some(p.sArg.text().to_vec())
            } else {
                None
            },
            p.pNewTable.clone(),
        )
    };
    if let (Some(z), Some(pNew)) = (z, pNew) {
        addModuleArgument(db, &pNew, z);
    }
}

/// C: `void sqlite3VtabFinishParse(Parse *pParse, Token *pEnd)`
///
/// The parser calls this routine after the CREATE VIRTUAL TABLE statement
/// has been completely parsed.
pub fn sqlite3VtabFinishParse(db: &mut Sqlite3, pEnd: Option<&Token>) {
    let Some(pTab) = db.ppr().pNewTable.clone() else {
        return;
    };
    debug_assert!(pTab.borrow().IsVirtual());
    addArgumentToVtab(db);
    db.pp().sArg.z = None;
    if pTab.borrow().u.vtab.nArg < 1 {
        return;
    }

    // If the CREATE VIRTUAL TABLE statement is being entered for the first
    // time (in other words if the virtual table is actually being created
    // now instead of just being read out of sqlite_schema) then do
    // additional initialization work and store the statement text in the
    // sqlite_schema table.
    if db.init.busy == 0 {
        crate::build::sqlite3MayAbort(db);

        // Compute the complete text of the CREATE VIRTUAL TABLE statement
        if let Some(pEnd) = pEnd {
            let p = db.pp();
            p.sNameToken.n = pEnd.off - p.sNameToken.off + pEnd.n;
        }
        let sName = db.ppr().sNameToken.clone();
        let zStmt = crate::mprintf!(db, "CREATE VIRTUAL TABLE %T", &sName);

        // A slot for the record has already been allocated in the schema
        // table.  We just need to update that slot with all the information
        // we've collected.
        //
        // The VM register number pParse->u1.cr.regRowid holds the rowid of
        // an entry in the sqlite_schema table that was created for this
        // vtab by sqlite3StartTable().
        let (pSchema, zName) = {
            let t = pTab.borrow();
            (t.pSchema, t.zName.clone())
        };
        let iDb = db.schema_index(pSchema);
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        let regRowid = db.ppr().u1.cr.regRowid;
        crate::build::sqlite3NestedParse(
            db,
            Some(
                b"UPDATE %Q.sqlite_master SET type='table', name=%Q, tbl_name=%Q, rootpage=0, sql=%Q WHERE rowid=#%d",
            ),
            &[
                PfArg::Str(zDb.as_deref()),
                PfArg::Str(Some(&zName)),
                PfArg::Str(Some(&zName)),
                PfArg::Str(Some(&zStmt)),
                PfArg::Int(regRowid as i64),
            ],
        );
        crate::select::sqlite3GetVdbe(db);
        crate::build::sqlite3ChangeCookie(db, iDb);

        crate::vdbeaux::sqlite3VdbeAddOp0(db.v(), OP_Expire);
        let zWhere = crate::mprintf!(db, "name=%Q AND sql=%Q", &zName, &zStmt);
        crate::vdbeaux::sqlite3VdbeAddParseSchemaOp(db, iDb, Some(zWhere), 0);

        let iReg = {
            let p = db.pp();
            p.nMem += 1;
            p.nMem
        };
        crate::vdbeaux::sqlite3VdbeLoadString(db.v(), iReg, &zName);
        crate::vdbeaux::sqlite3VdbeAddOp2(db.v(), OP_VCreate, iDb, iReg);
    } else {
        // If we are rereading the sqlite_schema table create the in-memory
        // record of the table.
        let (pSchema, zName) = {
            let t = pTab.borrow();
            (t.pSchema, t.zName.clone())
        };
        crate::build::sqlite3MarkAllShadowTablesOf(db, &pTab);
        let iDb = db.schema_index(pSchema);
        let pOld = db.aDb[iDb as usize]
            .pSchema
            .borrow_mut()
            .tblHash
            .insert(&zName, pTab.clone());
        if pOld.is_some() {
            debug_assert!(false);
            return;
        }
        db.pp().pNewTable = None;
    }
}

/// C: `void sqlite3VtabArgInit(Parse *pParse)`
///
/// The parser calls this routine when it sees the first token of an
/// argument to the module name in a CREATE VIRTUAL TABLE statement.
pub fn sqlite3VtabArgInit(db: &mut Sqlite3) {
    addArgumentToVtab(db);
    let p = db.pp();
    p.sArg.z = None;
    p.sArg.off = 0;
    p.sArg.n = 0;
}

/// C: `void sqlite3VtabArgExtend(Parse *pParse, Token *p)`
///
/// The parser calls this routine for each token after the first token in
/// an argument to the module name in a CREATE VIRTUAL TABLE statement.
pub fn sqlite3VtabArgExtend(db: &mut Sqlite3, p: &Token) {
    let pArg = &mut db.pp().sArg;
    if pArg.z.is_none() {
        pArg.z = p.z.clone();
        pArg.off = p.off;
        pArg.n = p.n;
    } else {
        debug_assert!(pArg.off <= p.off);
        pArg.n = p.off + p.n - pArg.off;
    }
}

/// Invoke a virtual table constructor (either xCreate or xConnect, as
/// selected by `bCreate`).
fn vtabCallConstructor(
    db: &mut Sqlite3,
    pTab: &TableRef,
    pMod: &ModuleRef,
    bCreate: bool,
    pzErr: &mut Option<ZStr>,
) -> i32 {
    debug_assert!(pTab.borrow().IsVirtual());

    // Check that the virtual-table is not already being initialized
    if db.pVtabCtx.iter().any(|c| Rc::ptr_eq(&c.pTab, pTab)) {
        let zName = pTab.borrow().zName.clone();
        *pzErr = Some(crate::mprintf!(
            db,
            "vtable constructor called recursively: %s",
            &zName
        ));
        return SQLITE_LOCKED;
    }

    let zModuleName = pTab.borrow().zName.clone();

    let pVTable = Rc::new(RefCell::new(VTable {
        pMod: pMod.clone(),
        pVtab: None,
        nRef: 0,
        bConstraint: 0,
        bAllSchemas: 0,
        eVtabRisk: SQLITE_VTABRISK_Normal as u8,
        iSavepoint: 0,
        nCursor: 0,
    }));

    let pSchema = pTab.borrow().pSchema;
    let iDb = db.schema_index(pSchema);
    let zDbSName = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();
    let azArg: Vec<ZStr> = {
        let mut t = pTab.borrow_mut();
        if t.u.vtab.azArg.len() > 1 {
            t.u.vtab.azArg[1] = zDbSName.clone();
        }
        t.u.vtab.azArg.clone()
    };

    // Invoke the virtual table constructor
    db.pVtabCtx.push(VtabCtx {
        pVTable: pVTable.clone(),
        pTab: pTab.clone(),
        bDeclared: 0,
    });
    pTab.borrow_mut().nTabRef += 1;
    let pModule = pMod.borrow().pModule.clone();
    let argv: Vec<&[u8]> = azArg.iter().map(|a| &a[..]).collect();
    let mut zErr: Option<ZStr> = None;
    let res = if bCreate {
        pModule.xCreate(db, &argv, &mut zErr)
    } else {
        pModule.xConnect(db, &argv, &mut zErr)
    };
    crate::build::sqlite3DeleteTable(db, pTab);
    let sCtx = db.pVtabCtx.pop().expect("vtab context");
    debug_assert!(Rc::ptr_eq(&sCtx.pTab, pTab));

    let mut rc;
    match res {
        Err(e) => {
            rc = e;
            if rc == SQLITE_NOMEM {
                db.mallocFailed = 1;
            }
            match zErr {
                None => {
                    *pzErr = Some(crate::mprintf!(
                        db,
                        "vtable constructor failed: %s",
                        &zModuleName
                    ));
                }
                Some(z) => {
                    *pzErr = Some(crate::mprintf!(db, "%s", &z));
                }
            }
        }
        Ok(mut vt) => {
            rc = SQLITE_OK;
            // C: memset(pVTable->pVtab, 0, sizeof(pVTable->pVtab[0]));
            *vt.zErrMsg() = None;
            {
                let mut v = pVTable.borrow_mut();
                v.pVtab = Some(Rc::new(RefCell::new(vt)));
                v.nRef = 1;
            }
            pMod.borrow_mut().nRefModule += 1;
            if sCtx.bDeclared == 0 {
                *pzErr = Some(crate::mprintf!(
                    db,
                    "vtable constructor did not declare schema: %s",
                    &zModuleName
                ));
                sqlite3VtabUnlock(db, &pVTable);
                rc = SQLITE_ERROR;
            } else {
                // If everything went according to plan, link the new VTable
                // structure into the list headed by pTab->u.vtab.p.  Then
                // loop through the columns of the table to see if any of
                // them contain the token "hidden".  If so, set the Column
                // COLFLAG_HIDDEN flag and remove the token from the type
                // string.
                let mut t = pTab.borrow_mut();
                t.u.vtab.p.insert(0, pVTable.clone());
                let mut oooHidden: u32 = 0;
                for iCol in 0..t.nCol.max(0) as usize {
                    let zType = crate::util::sqlite3ColumnType(&t.aCol[iCol], Some(b""))
                        .unwrap_or_default();
                    let nType = zType.len();
                    let mut i = 0usize;
                    while i < nType {
                        if zType.len() - i >= 6
                            && zType[i..i + 6].eq_ignore_ascii_case(b"hidden")
                            && (i == 0 || zType[i - 1] == b' ')
                            && (i + 6 == nType || zType[i + 6] == b' ')
                        {
                            break;
                        }
                        i += 1;
                    }
                    if i < nType {
                        let nDel = 6 + if i + 6 < nType { 1 } else { 0 };
                        let mut z = zType[..i].to_vec();
                        z.extend_from_slice(&zType[i + nDel..]);
                        if z.len() == i && i > 0 {
                            debug_assert!(z[i - 1] == b' ');
                            z.truncate(i - 1);
                        }
                        let col = &mut t.aCol[iCol];
                        if (col.colFlags & COLFLAG_HASTYPE) != 0 {
                            col.zCnType = Some(z);
                        }
                        col.colFlags |= COLFLAG_HIDDEN;
                        t.tabFlags |= TF_HasHidden;
                        oooHidden = TF_OOOHidden;
                    } else {
                        t.tabFlags |= oooHidden;
                    }
                }
            }
        }
    }
    rc
}

/// C: `int sqlite3VtabCallConnect(Parse *pParse, Table *pTab)`
///
/// This function is invoked by the parser to call the xConnect() method of
/// the virtual table pTab. If an error occurs, an error code is returned
/// and an error left in pParse.
pub fn sqlite3VtabCallConnect(db: &mut Sqlite3, pTab: &TableRef) -> i32 {
    debug_assert!(pTab.borrow().IsVirtual());
    if sqlite3GetVTable(db, pTab).is_some() {
        return SQLITE_OK;
    }

    // Locate the required virtual table module
    let zMod = pTab
        .borrow()
        .u
        .vtab
        .azArg
        .first()
        .cloned()
        .unwrap_or_default();
    let pMod = db.aModule.find(&zMod).cloned();

    let rc;
    match pMod {
        None => {
            crate::errmsg!(db, "no such module: %s", &zMod);
            rc = SQLITE_ERROR;
        }
        Some(pMod) => {
            let mut zErr = None;
            rc = vtabCallConstructor(db, pTab, &pMod, false, &mut zErr);
            if rc != SQLITE_OK {
                crate::errmsg!(db, "%s", zErr.as_deref());
                db.pp().rc = rc;
            } else {
                crate::build::sqlite3MarkAllShadowTablesOf(db, pTab);
            }
        }
    }
    rc
}

/// Grow the db->aVTrans[] array so that there is room for at least one
/// more v-table.
fn growVTrans(db: &mut Sqlite3) -> i32 {
    if db.aVTrans.is_none() {
        db.aVTrans = Some(Vec::new());
    }
    SQLITE_OK
}

/// Add the virtual table pVTab to the array sqlite3.aVTrans[].
fn addToVTrans(db: &mut Sqlite3, pVTab: &VTableRef) {
    db.aVTrans.get_or_insert_with(Vec::new).push(pVTab.clone());
    db.nVTrans += 1;
    sqlite3VtabLock(pVTab);
}

/// C: `int sqlite3VtabCallCreate(sqlite3 *db, int iDb, const char *zTab, char **pzErr)`
///
/// This function is invoked by the vdbe to call the xCreate method of the
/// virtual table named zTab in database iDb.
pub fn sqlite3VtabCallCreate(
    db: &mut Sqlite3,
    iDb: i32,
    zTab: Option<&[u8]>,
    pzErr: &mut Option<ZStr>,
) -> i32 {
    let mut rc;
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let Some(pTab) = crate::build::sqlite3FindTable(db, zTab, zDb.as_deref()) else {
        debug_assert!(false);
        return SQLITE_ERROR;
    };
    debug_assert!(pTab.borrow().IsVirtual() && pTab.borrow().u.vtab.p.is_empty());

    // Locate the required virtual table module
    let zMod = pTab
        .borrow()
        .u
        .vtab
        .azArg
        .first()
        .cloned()
        .unwrap_or_default();
    let pMod = db.aModule.find(&zMod).cloned();

    // If the module has been registered and includes a Create method,
    // invoke it now. If the module has not been registered, return an
    // error. Otherwise, do nothing.
    let usable = pMod.as_ref().is_some_and(|m| {
        let m = m.borrow();
        m.pModule.has_xCreate() && m.pModule.has_xDestroy()
    });
    if !usable {
        *pzErr = Some(crate::mprintf!(db, "no such module: %s", &zMod));
        rc = SQLITE_ERROR;
    } else {
        rc = vtabCallConstructor(db, &pTab, pMod.as_ref().unwrap(), true, pzErr);
    }

    // Justification of ALWAYS():  The xConstructor method is required to
    // create a valid sqlite3_vtab if it returns SQLITE_OK.
    if rc == SQLITE_OK {
        if let Some(pVTab) = sqlite3GetVTable(db, &pTab) {
            rc = growVTrans(db);
            if rc == SQLITE_OK {
                addToVTrans(db, &pVTab);
            }
        }
    }
    rc
}

/// C: `int sqlite3_declare_vtab(sqlite3 *db, const char *zCreateTable)`
///
/// This function is used to set the schema of a virtual table.  It is only
/// valid to call this function from within the xCreate() or xConnect() of
/// a virtual table module.
pub fn sqlite3_declare_vtab(db: &mut Sqlite3, zCreateTable: Option<&[u8]>) -> i32 {
    let Some(zCreateTable) = zCreateTable else {
        return SQLITE_MISUSE;
    };
    let mut rc = SQLITE_OK;

    // Verify that the first two keywords in the CREATE TABLE statement
    // really are "CREATE" and "TABLE".  If this is not the case, then
    // sqlite3_declare_vtab() is being misused.
    {
        let mut z = 0usize;
        for kw in [TK_CREATE, TK_TABLE] {
            let mut tokenType = 0i32;
            loop {
                if z >= zCreateTable.len() || zCreateTable[z] == 0 {
                    tokenType = TK_ILLEGAL as i32;
                    break;
                }
                z += crate::tokenize::sqlite3GetToken(&zCreateTable[z..], &mut tokenType) as usize;
                if tokenType != TK_SPACE as i32 && tokenType != TK_COMMENT as i32 {
                    break;
                }
            }
            if tokenType != kw as i32 {
                crate::util::sqlite3ErrorWithMsg(db, SQLITE_ERROR, Some(b"syntax error"), &[]);
                return SQLITE_ERROR;
            }
        }
    }

    let pTab = match db.pVtabCtx.last() {
        Some(c) if c.bDeclared == 0 => c.pTab.clone(),
        _ => {
            crate::util::sqlite3Error(db, SQLITE_MISUSE);
            return SQLITE_MISUSE;
        }
    };
    debug_assert!(pTab.borrow().IsVirtual());
    let pVTable = db.pVtabCtx.last().unwrap().pVTable.clone();

    crate::prepare::sqlite3ParseObjectInit(db);
    {
        let p = db.pp();
        p.eParseMode = PARSE_MODE_DECLARE_VTAB;
        p.disableTriggers = 1;
    }
    // We should never be able to reach this point while loading the schema.
    // Nevertheless, defend against that (turn off db->init.busy) in case a
    // bug arises.
    debug_assert!(db.init.busy == 0);
    let initBusy = db.init.busy;
    db.init.busy = 0;
    db.pp().nQueryLoop = 1;
    let nSql = zCreateTable
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(zCreateTable.len());
    let zSql: Rc<[u8]> = Rc::from(&zCreateTable[..nSql]);
    if SQLITE_OK == crate::tokenize::sqlite3RunParser(db, &zSql, 0) {
        let pNew = db.ppr().pNewTable.clone().expect("declare_vtab table");
        debug_assert!(pNew.borrow().IsOrdinaryTable());
        debug_assert!(db.ppr().zErrMsg.is_none());
        if pTab.borrow().aCol.is_empty() {
            let (aCol, nCol, tabFlags) = {
                let mut n = pNew.borrow_mut();
                let aCol = std::mem::take(&mut n.aCol);
                let nCol = n.nCol;
                n.nCol = 0;
                n.u.tab.pDfltList = Tree::None;
                (aCol, nCol, n.tabFlags)
            };
            {
                let mut t = pTab.borrow_mut();
                t.aCol = aCol;
                t.nCol = nCol;
                t.nNVCol = nCol;
                t.tabFlags |= tabFlags & (TF_WithoutRowid | TF_NoVisibleRowid);
            }
            let hasRowid = pNew.borrow().HasRowid();
            debug_assert!(pTab.borrow().pIndex.is_empty());
            if !hasRowid {
                let hasUpdate = pVTable.borrow().pMod.borrow().pModule.has_xUpdate();
                let nKey = crate::build::sqlite3PrimaryKeyIndex(&pNew)
                    .map(|i| i.borrow().nKeyCol)
                    .unwrap_or(0);
                if hasUpdate && nKey != 1 {
                    // WITHOUT ROWID virtual tables must either be read-only
                    // (xUpdate==0) or else must have a single-column PRIMARY
                    // KEY
                    rc = SQLITE_ERROR;
                }
            }
            let pIdx = {
                let mut n = pNew.borrow_mut();
                if n.pIndex.is_empty() {
                    None
                } else {
                    debug_assert!(n.pIndex.len() == 1);
                    Some(n.pIndex.remove(0))
                }
            };
            if let Some(pIdx) = pIdx {
                pIdx.borrow_mut().pTable = Rc::downgrade(&pTab);
                pTab.borrow_mut().pIndex.push(pIdx);
            }
        }
        db.pVtabCtx.last_mut().unwrap().bDeclared = 1;
    } else {
        let zErrMsg = db.pp().zErrMsg.take();
        match zErrMsg {
            Some(z) => crate::util::sqlite3ErrorWithMsg(
                db,
                SQLITE_ERROR,
                Some(b"%s"),
                &[PfArg::Str(Some(&z))],
            ),
            None => crate::util::sqlite3ErrorWithMsg(db, SQLITE_ERROR, None, &[]),
        }
        rc = SQLITE_ERROR;
    }
    db.pp().eParseMode = PARSE_MODE_NORMAL;

    if let Some(mut v) = db.pp().pVdbe.take() {
        crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v);
    }
    if let Some(pNew) = db.pp().pNewTable.take() {
        crate::build::sqlite3DeleteTable(db, &pNew);
    }
    crate::prepare::sqlite3ParseObjectReset(db);
    db.init.busy = initBusy;

    debug_assert!((rc & 0xff) == rc);
    rc = crate::mainapi::sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3VtabCallDestroy(sqlite3 *db, int iDb, const char *zTab)`
///
/// This function is invoked by the vdbe to call the xDestroy method of the
/// virtual table named zTab in database iDb. This occurs when a DROP TABLE
/// is mentioned.
///
/// This call is a no-op if zTab is not a virtual table.
pub fn sqlite3VtabCallDestroy(db: &mut Sqlite3, iDb: i32, zTab: Option<&[u8]>) -> i32 {
    let mut rc = SQLITE_OK;
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let pTab = crate::build::sqlite3FindTable(db, zTab, zDb.as_deref());
    if let Some(pTab) = pTab {
        let ok = {
            let t = pTab.borrow();
            t.IsVirtual() && !t.u.vtab.p.is_empty()
        };
        if ok {
            let list = pTab.borrow().u.vtab.p.clone();
            for p in list.iter() {
                debug_assert!(p.borrow().pVtab.is_some());
                if p.borrow().nCursor > 0 {
                    return SQLITE_LOCKED;
                }
            }
            let p = vtabDisconnectAll(db, true, &pTab).expect("vtable");
            pTab.borrow_mut().nTabRef += 1;
            let pVtab = p.borrow().pVtab.clone().expect("sqlite3_vtab");
            rc = pVtab.borrow_mut().xDestroy(db);
            // Remove the sqlite3_vtab* from the aVTrans[] array, if
            // applicable
            if rc == SQLITE_OK {
                p.borrow_mut().pVtab = None;
                pTab.borrow_mut().u.vtab.p.clear();
                sqlite3VtabUnlock(db, &p);
            }
            crate::build::sqlite3DeleteTable(db, &pTab);
        }
    }
    rc
}

/// The xCommit/xRollback selector of callFinaliser().
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finaliser {
    Commit,
    Rollback,
}

/// This function invokes either the xRollback or xCommit method of each of
/// the virtual tables in the sqlite3.aVTrans array.  The array is cleared
/// after invoking the callbacks.
fn callFinaliser(db: &mut Sqlite3, which: Finaliser) {
    if let Some(aVTrans) = db.aVTrans.take() {
        for pVTab in aVTrans.iter() {
            let p = pVTab.borrow().pVtab.clone();
            if let Some(p) = p {
                let has = p.borrow().has_xBegin();
                if has {
                    match which {
                        Finaliser::Commit => p.borrow_mut().xCommit(db),
                        Finaliser::Rollback => p.borrow_mut().xRollback(db),
                    };
                }
            }
            pVTab.borrow_mut().iSavepoint = 0;
            sqlite3VtabUnlock(db, pVTab);
        }
        db.nVTrans = 0;
    }
}

/// sqlite3VtabInSync(db): true while the xSync methods run (C
/// `db->nVTrans>0 && db->aVTrans==0`).
#[inline]
pub fn sqlite3VtabInSync(db: &Sqlite3) -> bool {
    db.nVTrans > 0 && db.aVTrans.is_none()
}

/// C: `int sqlite3VtabSync(sqlite3 *db, Vdbe *p)`
///
/// Invoke the xSync method of all virtual tables in the sqlite3.aVTrans
/// array. Return the error code for the first error that occurs, or
/// SQLITE_OK if all xSync operations are successful.
///
/// If an error message is available, leave it in p->zErrMsg.
pub fn sqlite3VtabSync(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut rc = SQLITE_OK;
    let aVTrans = db.aVTrans.take();
    if let Some(a) = &aVTrans {
        let mut i = 0;
        while rc == SQLITE_OK && i < db.nVTrans as usize && i < a.len() {
            let pVtab = a[i].borrow().pVtab.clone();
            if let Some(pVtab) = pVtab {
                let has = pVtab.borrow().has_xBegin();
                if has {
                    rc = pVtab.borrow_mut().xSync(db);
                    crate::vdbeaux::sqlite3VtabImportErrmsg(db, p, &pVtab);
                }
            }
            i += 1;
        }
    }
    db.aVTrans = aVTrans;
    rc
}

/// C: `int sqlite3VtabRollback(sqlite3 *db)`
///
/// Invoke the xRollback method of all virtual tables in the
/// sqlite3.aVTrans array. Then clear the array itself.
pub fn sqlite3VtabRollback(db: &mut Sqlite3) -> i32 {
    callFinaliser(db, Finaliser::Rollback);
    SQLITE_OK
}

/// C: `int sqlite3VtabCommit(sqlite3 *db)`
///
/// Invoke the xCommit method of all virtual tables in the sqlite3.aVTrans
/// array. Then clear the array itself.
pub fn sqlite3VtabCommit(db: &mut Sqlite3) -> i32 {
    callFinaliser(db, Finaliser::Commit);
    SQLITE_OK
}

/// C: `int sqlite3VtabBegin(sqlite3 *db, VTable *pVTab)`
///
/// If the virtual table pVtab supports the transaction interface
/// (xBegin/xRollback/xCommit and optionally xSync) and a transaction is not
/// currently open, invoke the xBegin method now.
///
/// If the xBegin call is successful, place the sqlite3_vtab pointer in the
/// sqlite3.aVTrans array.
pub fn sqlite3VtabBegin(db: &mut Sqlite3, pVTab: Option<&VTableRef>) -> i32 {
    let mut rc = SQLITE_OK;

    // Special case: If db->aVTrans is NULL and db->nVTrans is greater than
    // zero, then this function is being called from within a virtual
    // module xSync() callback. It is illegal to write to virtual module
    // tables in this case, so return SQLITE_LOCKED.
    if sqlite3VtabInSync(db) {
        return SQLITE_LOCKED;
    }
    let Some(pVTab) = pVTab else {
        return SQLITE_OK;
    };
    let Some(pVtab) = pVTab.borrow().pVtab.clone() else {
        return SQLITE_OK;
    };
    let has = pVtab.borrow().has_xBegin();
    if has {
        // If pVtab is already in the aVTrans array, return early
        if let Some(a) = &db.aVTrans {
            if a.iter().any(|x| Rc::ptr_eq(x, pVTab)) {
                return SQLITE_OK;
            }
        }

        // Invoke the xBegin method. If successful, add the vtab to the
        // sqlite3.aVTrans[] array.
        rc = growVTrans(db);
        if rc == SQLITE_OK {
            rc = pVtab.borrow_mut().xBegin(db);
            if rc == SQLITE_OK {
                let iSvpt = db.nStatement + db.nSavepoint;
                addToVTrans(db, pVTab);
                let iVersion = pVTab.borrow().pMod.borrow().pModule.iVersion();
                let hasSvpt = iVersion >= 2 && pVtab.borrow().has_xSavepoint();
                if iSvpt != 0 && hasSvpt {
                    pVTab.borrow_mut().iSavepoint = iSvpt;
                    rc = pVtab.borrow_mut().xSavepoint(db, iSvpt - 1);
                }
            }
        }
    }
    rc
}

/// C: `int sqlite3VtabSavepoint(sqlite3 *db, int op, int iSavepoint)`
///
/// Invoke either the xSavepoint, xRollbackTo or xRelease method of all
/// virtual tables that currently have an open transaction. Pass iSavepoint
/// as the second argument to the virtual table method invoked.
pub fn sqlite3VtabSavepoint(db: &mut Sqlite3, op: i32, iSavepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK || op == SAVEPOINT_BEGIN);
    debug_assert!(iSavepoint >= -1);
    let Some(a) = db.aVTrans.clone() else {
        return SQLITE_OK;
    };
    let mut i = 0;
    while rc == SQLITE_OK && i < db.nVTrans as usize && i < a.len() {
        let pVTab = a[i].clone();
        i += 1;
        let (pVtab, iVersion) = {
            let v = pVTab.borrow();
            (v.pVtab.clone(), v.pMod.borrow().pModule.iVersion())
        };
        let Some(pVtab) = pVtab else { continue };
        if iVersion >= 2 {
            sqlite3VtabLock(&pVTab);
            if op == SAVEPOINT_BEGIN {
                pVTab.borrow_mut().iSavepoint = iSavepoint + 1;
            }
            let has = pVtab.borrow().has_xSavepoint();
            if has && pVTab.borrow().iSavepoint > iSavepoint {
                let savedFlags = db.flags & SQLITE_Defensive;
                db.flags &= !SQLITE_Defensive;
                rc = match op {
                    SAVEPOINT_BEGIN => pVtab.borrow_mut().xSavepoint(db, iSavepoint),
                    SAVEPOINT_ROLLBACK => pVtab.borrow_mut().xRollbackTo(db, iSavepoint),
                    _ => pVtab.borrow_mut().xRelease(db, iSavepoint),
                };
                db.flags |= savedFlags;
            }
            sqlite3VtabUnlock(db, &pVTab);
        }
    }
    rc
}

/// C: `FuncDef * sqlite3VtabOverloadFunction( sqlite3 *db, FuncDef *pDef, int nArg, Expr *pExpr )`
///
/// The first parameter (pDef) is a function implementation.  The second
/// parameter (pExpr) is the first argument to this function.  If pExpr is
/// a column in a virtual table, then let the virtual table implementation
/// have an opportunity to overload the function.
///
/// Return either the pDef argument (indicating no change) or a new FuncDef
/// structure that is marked as ephemeral using the SQLITE_FUNC_EPHEM flag.
pub fn sqlite3VtabOverloadFunction(
    db: &mut Sqlite3,
    pDef: &FuncDefRef,
    nArg: i32,
    pExpr: ExprId,
) -> Option<FuncDefRef> {
    // Check to see the left operand is a column in a virtual table
    if pExpr.is_null() {
        return Some(pDef.clone());
    }
    let (op, pTab) = {
        let e = db.ex(pExpr);
        (e.op, e.y.pTab.clone())
    };
    if op != TK_COLUMN {
        return Some(pDef.clone());
    }
    let Some(pTab) = pTab else {
        return Some(pDef.clone());
    };
    if !pTab.borrow().IsVirtual() {
        return Some(pDef.clone());
    }
    let Some(pVtab) = sqlite3GetVtab(db, &pTab) else {
        return Some(pDef.clone());
    };

    // Call the xFindFunction method on the virtual table implementation to
    // see if the implementation wants to overload this function.
    let r = pVtab.borrow_mut().xFindFunction(nArg, &pDef.zName);
    let Some((rc, xSFunc, pArg)) = r else {
        return Some(pDef.clone());
    };
    if rc == 0 {
        return Some(pDef.clone());
    }

    // Create a new ephemeral function definition for the overloaded
    // function
    let mut pNew = (**pDef).clone();
    pNew.xSFunc = Some(xSFunc);
    pNew.pUserData = pArg;
    pNew.funcFlags |= SQLITE_FUNC_EPHEM;
    Some(Rc::new(pNew))
}

/// The score returned by xFindFunction for (nArg, zName) on the virtual
/// table of the column expression pExpr, or 0 (used by
/// isAuxiliaryVtabOperator() in whereexpr.c).
pub fn sqlite3VtabFindFunctionScore(
    db: &mut Sqlite3,
    pCol: ExprId,
    nArg: i32,
    zName: &[u8],
) -> i32 {
    let pTab = db.ex(pCol).y.pTab.clone();
    let Some(pTab) = pTab else { return 0 };
    let Some(pVtab) = sqlite3GetVtab(db, &pTab) else {
        return 0;
    };
    let r = pVtab.borrow_mut().xFindFunction(nArg, zName);
    r.map_or(0, |(s, _, _)| s)
}

/// C: `void sqlite3VtabMakeWritable(Parse *pParse, Table *pTab)`
///
/// Make sure virtual table pTab is contained in the pParse->apVirtualLock[]
/// array so that an OP_VBegin will get generated for it.  Add pTab to the
/// array if it is missing.  If pTab is already in the array, this routine
/// is a no-op.
pub fn sqlite3VtabMakeWritable(db: &mut Sqlite3, pTab: &TableRef) {
    debug_assert!(pTab.borrow().IsVirtual());
    let pToplevel = db.toplevel();
    if pToplevel.apVtabLock.iter().any(|t| Rc::ptr_eq(t, pTab)) {
        return;
    }
    pToplevel.apVtabLock.push(pTab.clone());
    pToplevel.nVtabLock += 1;
}

/// C: `int sqlite3VtabEponymousTableInit(Parse *pParse, Module *pMod)`
///
/// Check to see if virtual table module pMod can be have an eponymous
/// virtual table instance.  If it can, create one if one does not already
/// exist. Return non-zero if either the eponymous virtual table instance
/// exists when this routine returns or if an attempt to create it failed
/// and an error message was left in pParse.
pub fn sqlite3VtabEponymousTableInit(db: &mut Sqlite3, pMod: &ModuleRef) -> i32 {
    let (pModule, zName) = {
        let m = pMod.borrow();
        if m.pEpoTab.is_some() {
            return 1;
        }
        (m.pModule.clone(), m.zName.clone())
    };
    if pModule.has_xCreate() && !pModule.create_is_connect() {
        return 0;
    }
    let pSchema = db.aDb[0].pSchema.borrow().id;
    let pTab = Rc::new(RefCell::new(Table {
        zName: zName.clone(),
        nTabRef: 1,
        eTabType: TABTYP_VTAB,
        pSchema,
        iPKey: -1,
        tabFlags: TF_Eponymous,
        ..Default::default()
    }));
    pMod.borrow_mut().pEpoTab = Some(pTab.clone());
    debug_assert!(pTab.borrow().u.vtab.nArg == 0);
    addModuleArgument(db, &pTab, zName.clone());
    addModuleArgument(db, &pTab, Vec::new());
    addModuleArgument(db, &pTab, zName);
    db.nSchemaLock += 1;
    let mut zErr = None;
    let rc = vtabCallConstructor(db, &pTab, pMod, false, &mut zErr);
    db.nSchemaLock -= 1;
    if rc != 0 {
        crate::errmsg!(db, "%s", zErr.as_deref());
        db.pp().rc = rc;
        sqlite3VtabEponymousTableClear(db, pMod);
    }
    1
}

/// C: `void sqlite3VtabEponymousTableClear(sqlite3 *db, Module *pMod)`
///
/// Erase the eponymous virtual table instance associated with virtual
/// table module pMod, if it exists.
pub fn sqlite3VtabEponymousTableClear(db: &mut Sqlite3, pMod: &ModuleRef) {
    let pTab = pMod.borrow_mut().pEpoTab.take();
    if let Some(pTab) = pTab {
        // Mark the table as Ephemeral prior to deleting it, so that the
        // sqlite3DeleteTable() routine will know that it is not stored in
        // the schema.
        pTab.borrow_mut().tabFlags |= TF_Ephemeral;
        crate::build::sqlite3DeleteTable(db, &pTab);
    }
}

/// C: `void sqlite3VtabEponymousTableClearAll(sqlite3 *db)`
///
/// Erase all eponymous virtual table instances associated with database
/// handle db.
pub fn sqlite3VtabEponymousTableClearAll(db: &mut Sqlite3) {
    let mods: Vec<ModuleRef> = db.aModule.iter().map(|(_, m)| m.clone()).collect();
    for m in mods.iter() {
        sqlite3VtabEponymousTableClear(db, m);
    }
}

/// C: `int sqlite3_vtab_on_conflict(sqlite3 *db)`
///
/// Return the ON CONFLICT resolution mode in effect for the virtual table
/// update operation currently in progress.
pub fn sqlite3_vtab_on_conflict(db: &mut Sqlite3) -> i32 {
    const aMap: [i32; 5] = [
        SQLITE_ROLLBACK,
        SQLITE_ABORT,
        SQLITE_FAIL,
        SQLITE_IGNORE,
        SQLITE_REPLACE,
    ];
    debug_assert!(db.vtabOnConflict >= 1 && db.vtabOnConflict <= 5);
    let i = (db.vtabOnConflict as usize).clamp(1, 5) - 1;
    aMap[i]
}

/// C: `int sqlite3_vtab_config(sqlite3 *db, int op, ...)`
///
/// Call from within the xCreate() or xConnect() methods to provide the
/// SQLite core with additional information about the behavior of the
/// virtual table being implemented.
pub fn sqlite3_vtab_config(db: &mut Sqlite3, op: i32, args: &[PfArg]) -> i32 {
    let mut rc = SQLITE_OK;
    match db.pVtabCtx.last() {
        None => rc = SQLITE_MISUSE,
        Some(p) => {
            let mut v = p.pVTable.borrow_mut();
            match op {
                SQLITE_VTAB_CONSTRAINT_SUPPORT => {
                    let x = match args.first() {
                        Some(PfArg::Int(i)) => *i,
                        _ => 0,
                    };
                    v.bConstraint = x as u8;
                }
                SQLITE_VTAB_INNOCUOUS => v.eVtabRisk = SQLITE_VTABRISK_Low as u8,
                SQLITE_VTAB_DIRECTONLY => v.eVtabRisk = SQLITE_VTABRISK_High as u8,
                SQLITE_VTAB_USES_ALL_SCHEMAS => v.bAllSchemas = 1,
                _ => rc = SQLITE_MISUSE,
            }
        }
    }
    if rc != SQLITE_OK {
        crate::util::sqlite3Error(db, rc);
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vdbeapi::*;

    /// Run every statement of zSql; rows as "a|b", errors as "Error: msg".
    fn run(db: &mut Sqlite3, zSql: &str) -> Vec<String> {
        let sql = zSql.as_bytes().to_vec();
        let mut out = Vec::new();
        let mut off = 0usize;
        while off < sql.len() {
            let rest = sql[off..].to_vec();
            let mut stmt: Option<StmtId> = None;
            let mut tail = 0usize;
            let rc = crate::prepare::sqlite3_prepare_v2(
                db,
                Some(&rest),
                rest.len() as i32,
                &mut stmt,
                &mut tail,
            );
            if rc != SQLITE_OK {
                let m = crate::mainapi::sqlite3_errmsg(db).unwrap_or_default();
                out.push(format!("Error: {}", String::from_utf8_lossy(&m)));
            }
            if tail == 0 {
                break;
            }
            off += tail;
            let Some(id) = stmt else { continue };
            loop {
                let rc = sqlite3_step(db, id);
                if rc != SQLITE_ROW {
                    if rc != SQLITE_DONE {
                        let m = crate::mainapi::sqlite3_errmsg(db).unwrap_or_default();
                        out.push(format!("Error: {}", String::from_utf8_lossy(&m)));
                    }
                    break;
                }
                let n = sqlite3_column_count(db, id);
                let cols: Vec<String> = (0..n)
                    .map(|i| {
                        let t = sqlite3_column_text(db, id, i).unwrap_or_default();
                        String::from_utf8_lossy(&t).into_owned()
                    })
                    .collect();
                out.push(cols.join("|"));
            }
            sqlite3_finalize(db, id);
        }
        out
    }

    /// A read-only module "seq": seq(n) returns the rows 1..=n (hidden
    /// argument column "n", default 3).
    struct SeqModule;
    struct Seq {
        zErrMsg: Option<ZStr>,
    }
    struct SeqCur {
        i: i64,
        n: i64,
    }
    impl VtabModule for SeqModule {
        fn xConnect(
            &self,
            db: &mut Sqlite3,
            _argv: &[&[u8]],
            _pzErr: &mut Option<ZStr>,
        ) -> Result<Box<dyn Vtab>, i32> {
            let rc = sqlite3_declare_vtab(db, Some(b"CREATE TABLE x(value, n HIDDEN)"));
            if rc != SQLITE_OK {
                return Err(rc);
            }
            Ok(Box::new(Seq { zErrMsg: None }))
        }
        fn has_xCreate(&self) -> bool {
            false
        }
    }
    impl Vtab for Seq {
        fn xBestIndex(&mut self, _db: &mut Sqlite3, info: &mut IndexInfo) -> i32 {
            for i in 0..info.aConstraint.len() {
                let c = info.aConstraint[i];
                if c.iColumn == 1 && c.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                    if c.usable == 0 {
                        return SQLITE_CONSTRAINT;
                    }
                    info.aConstraintUsage[i].argvIndex = 1;
                    info.aConstraintUsage[i].omit = 1;
                    info.idxNum = 1;
                }
            }
            info.estimatedCost = 10.0;
            SQLITE_OK
        }
        fn xOpen(&mut self, _db: &mut Sqlite3) -> Result<Box<dyn VtabCursor>, i32> {
            Ok(Box::new(SeqCur { i: 1, n: 3 }))
        }
        fn zErrMsg(&mut self) -> &mut Option<ZStr> {
            &mut self.zErrMsg
        }
    }
    impl VtabCursor for SeqCur {
        fn xFilter(
            &mut self,
            _db: &mut Sqlite3,
            idx: i32,
            _s: Option<&[u8]>,
            argv: &mut [Mem],
        ) -> i32 {
            self.i = 1;
            self.n = if idx == 1 {
                sqlite3_value_int64(&mut argv[0])
            } else {
                3
            };
            SQLITE_OK
        }
        fn xNext(&mut self, _db: &mut Sqlite3) -> i32 {
            self.i += 1;
            SQLITE_OK
        }
        fn xEof(&mut self) -> bool {
            self.i > self.n
        }
        fn xColumn(&mut self, ctx: &mut Context, i: i32) -> i32 {
            sqlite3_result_int64(ctx, if i == 0 { self.i } else { self.n });
            SQLITE_OK
        }
        fn xRowid(&mut self, p: &mut i64) -> i32 {
            *p = self.i;
            SQLITE_OK
        }
    }

    fn open() -> Box<Sqlite3> {
        let mut pdb = None;
        let rc = crate::mainapi::sqlite3_open_v2(
            Some(b":memory:"),
            &mut pdb,
            (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as i32,
            None,
        );
        assert_eq!(rc, SQLITE_OK);
        pdb.unwrap()
    }

    #[test]
    fn eponymous_module() {
        let mut db = open();
        assert_eq!(
            sqlite3_create_module(&mut db, Some(b"seq"), Some(Rc::new(SeqModule))),
            SQLITE_OK
        );
        assert_eq!(run(&mut db, "select * from seq;"), ["1", "2", "3"]);
        assert_eq!(run(&mut db, "select value, n from seq(2);"), ["1|2", "2|2"]);
        assert_eq!(
            run(
                &mut db,
                "select a.value, b.value from seq(2) a, seq(a.value) b;"
            ),
            ["1|1", "2|1", "2|2"]
        );
        assert_eq!(
            run(&mut db, "create virtual table s using seq;"),
            ["Error: no such module: seq"]
        );
        assert_eq!(run(&mut db, "pragma module_list;"), ["seq"]);
        sqlite3_drop_modules(&mut db, None);
        assert_eq!(
            run(&mut db, "select * from seq;"),
            ["Error: no such table: seq"]
        );
        crate::mainapi::sqlite3_close(&mut db);
    }

    #[test]
    fn pragma_vtabs() {
        let mut db = open();
        assert_eq!(
            run(
                &mut db,
                "create table t(a integer primary key, b text);\
                 select name, type, pk from pragma_table_info('t');\
                 select count(*) from sqlite_master m, pragma_table_info(m.name);"
            ),
            ["a|INTEGER|1", "b|TEXT|0", "2"]
        );
        assert_eq!(
            run(&mut db, "create virtual table v using nosuch(x);"),
            ["Error: no such module: nosuch"]
        );
        crate::mainapi::sqlite3_close(&mut db);
    }
}
