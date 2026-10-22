//! Port of alter.c: ALTER TABLE (RENAME TO, RENAME COLUMN, ADD COLUMN,
//! DROP COLUMN and the constraint forms), the RenameToken machinery used
//! by the parser in PARSE_MODE_RENAME, and the internal SQL functions
//! (sqlite_rename_column() & co) that rewrite the schema SQL.
//!
//! Rust notes:
//!
//! * The C code maps the *address* of a parse-tree element (an Expr, a
//!   name string, a Column ...) to the token that created it.  The port
//!   uses `RenameKey` values; the `rename_key_*()` helpers below define the
//!   key of every kind of element.  Code that creates or moves such
//!   elements while `IN_RENAME_OBJECT` must use these helpers (build.rs,
//!   expr.rs, trigger.rs, parse.rs).  A RenameToken whose key was cleared
//!   (C `p->p = 0`) holds `RENAME_KEY_NULL`.
//! * `Parse::pRename` is a Vec kept in C list order (most recent first).
//! * The internal SQL functions parse a CREATE statement with a separate
//!   Parse object: `sqlite3ParseObjectInit(db)` is assumed to push a fresh
//!   Parse onto `db.parses` (C links it as db->pParse) and
//!   `sqlite3ParseObjectReset(db)` to pop it again.

use crate::build::*;
use crate::expr::*;
use crate::int::*;
use crate::printf::PfArg;
use crate::resolve::*;
use crate::select::*;
use crate::util::*;
use crate::vdbeapi::*;
use crate::vdbeaux::*;
use crate::walker::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The C schema table name (LEGACY_SCHEMA_TABLE).
const LEGACY_SCHEMA_TABLE: &str = "sqlite_master";

/// `sqlite3NestedParse(pParse, zFmt, ...)`.
macro_rules! nested {
    ($db:expr, $fmt:expr $(, $a:expr)* $(,)?) => {
        crate::build::sqlite3NestedParse(
            $db,
            Some(AsRef::<[u8]>::as_ref(&$fmt)),
            &[$(crate::printf::PfArg::from($a)),*],
        )
    };
}

// ---------------------------------------------------------------------------
// Rename keys
// ---------------------------------------------------------------------------

/// The key of a RenameToken that no longer maps anything (C `p->p==0`).
pub const RENAME_KEY_NULL: RenameKey = RenameKey::Other(0);

/// Identity of a reference-counted object, for RenameKey values.
#[inline]
fn rc_addr<T>(r: &Rc<RefCell<T>>) -> usize {
    Rc::as_ptr(r) as *const u8 as usize
}

/// RenameKey of the name of a Table (C: the pTab->zName pointer).
pub fn rename_key_table_name(pTab: &TableRef) -> RenameKey {
    RenameKey::Other(rc_addr(pTab))
}
/// RenameKey of the name of column iCol of pTab (C: the Column pointer
/// / its zCnName).
pub fn rename_key_column(pTab: &TableRef, iCol: i32) -> RenameKey {
    RenameKey::Col(rc_addr(pTab), iCol)
}
/// RenameKey of &pTab->iPKey.
pub fn rename_key_ipkey(pTab: &TableRef) -> RenameKey {
    RenameKey::Other(rc_addr(pTab) + 1)
}
/// RenameKey of pFKey->zTo.
pub fn rename_key_fkey_to(pFKey: &FKeyRef) -> RenameKey {
    RenameKey::Other(rc_addr(pFKey))
}
/// RenameKey of &pFKey->aCol[i] (the child column).
pub fn rename_key_fkey_from_col(pFKey: &FKeyRef, i: i32) -> RenameKey {
    RenameKey::Other(rc_addr(pFKey) + 8 + 2 * i as usize)
}
/// RenameKey of pFKey->aCol[i].zCol (the parent column name).
pub fn rename_key_fkey_to_col(pFKey: &FKeyRef, i: i32) -> RenameKey {
    RenameKey::Other(rc_addr(pFKey) + 9 + 2 * i as usize)
}
/// RenameKey of pList->a[i].zEName of an ExprList.
pub fn rename_key_elist_item(pList: ExprListId, i: i32) -> RenameKey {
    RenameKey::Other((1usize << 60) | ((pList.0 as usize) << 20) | (i as usize))
}
/// RenameKey of pList->a[i].zName of an IdList.
pub fn rename_key_idlist_item(pList: IdListId, i: i32) -> RenameKey {
    RenameKey::Other((2usize << 60) | ((pList.0 as usize) << 20) | (i as usize))
}
/// RenameKey of pSrc->a[i].zName of a SrcList.
pub fn rename_key_srcitem_name(pItem: SrcItemId) -> RenameKey {
    RenameKey::SrcItemName(pItem)
}
/// RenameKey of pIdx->zName.
pub fn rename_key_index_name(pIdx: &IndexRef) -> RenameKey {
    RenameKey::IndexName(rc_addr(pIdx))
}
/// RenameKey of pTrigger->table (the name of the table of a trigger).
pub fn rename_key_trigger_table(pTrig: &TriggerRef) -> RenameKey {
    RenameKey::Other(rc_addr(pTrig))
}

#[inline]
fn tab_eq(a: &Option<TableRef>, b: &Option<TableRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// The text of a value as an owned string (C sqlite3_value_text()).
fn vtext(m: &mut Mem) -> Option<ZStr> {
    sqlite3_value_text(m).map(|s| s.to_vec())
}

/// Bytes of `z` up to (not including) the first NUL (C strlen view).
fn cstr(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// sqlite3GetToken() at offset `off` of `z` (past-the-end reads as NUL).
fn get_token(z: &[u8], off: usize, t: &mut i32) -> usize {
    let s = if off < z.len() {
        &z[off..]
    } else {
        &z[z.len()..]
    };
    crate::tokenize::sqlite3GetToken(s, t) as usize
}

/// In-place sqlite3Dequote() (util.c) on an owned string.
fn dequote(z: &mut Vec<u8>) {
    if z.is_empty() {
        return;
    }
    let mut quote = z[0];
    if !crate::global::sqlite3Isquote(quote) {
        return;
    }
    if quote == b'[' {
        quote = b']';
    }
    let mut out = Vec::with_capacity(z.len());
    let mut i = 1;
    while i < z.len() {
        if z[i] == quote {
            if i + 1 < z.len() && z[i + 1] == quote {
                out.push(quote);
                i += 1;
            } else {
                break;
            }
        } else {
            out.push(z[i]);
        }
        i += 1;
    }
    *z = out;
}

// ---------------------------------------------------------------------------
// ALTER TABLE code generation
// ---------------------------------------------------------------------------

/// Parameter zName is the name of a table that is about to be altered
/// (either with ALTER TABLE ... RENAME TO or ALTER TABLE ... ADD COLUMN).
/// If the table is a system table, this function leaves an error message
/// in pParse->zErr (system tables may not be altered) and returns non-zero.
///
/// Or, if zName is not a system table, zero is returned.
fn isAlterableTable(db: &mut Sqlite3, pTab: &TableRef) -> i32 {
    let (zName, flags) = {
        let t = pTab.borrow();
        (t.zName.clone(), t.tabFlags)
    };
    if sqlite3_strnicmp(Some(&zName), Some(b"sqlite_"), 7) == 0
        || (flags & TF_Eponymous) != 0
        || ((flags & TF_Shadow) != 0 && sqlite3ReadOnlyShadowTables(db) != 0)
    {
        errmsg!(db, "table %s may not be altered", &zName);
        return 1;
    }
    0
}

/// Generate code to verify that the schemas of database zDb and, if
/// bTemp is not true, database "temp", can still be parsed. This is
/// called at the end of the generation of an ALTER TABLE ... RENAME ...
/// statement to ensure that the operation has not rendered any schema
/// objects unusable.
fn renameTestSchema(db: &mut Sqlite3, zDb: Option<&[u8]>, bTemp: i32, zWhen: &str, bNoDQS: i32) {
    db.pp().colNamesSet = 1;
    nested!(
        db,
        format!(
            "SELECT 1 FROM \"%w\".{} WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%' AND sqlite_rename_test(%Q, sql, type, name, %d, %Q, %d)=NULL ",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        zDb,
        bTemp,
        zWhen,
        bNoDQS
    );

    if bTemp == 0 {
        nested!(
            db,
            format!(
                "SELECT 1 FROM temp.{} WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%' AND sqlite_rename_test(%Q, sql, type, name, 1, %Q, %d)=NULL ",
                LEGACY_SCHEMA_TABLE
            ),
            zDb,
            zWhen,
            bNoDQS
        );
    }
}

/// Generate VM code to replace any double-quoted strings (but not
/// double-quoted identifiers) within the "sql" column of the sqlite_schema
/// table in database zDb with their single-quoted equivalents. If argument
/// bTemp is not true, similarly update all SQL statements in the
/// sqlite_schema table of the temp db.
fn renameFixQuotes(db: &mut Sqlite3, zDb: Option<&[u8]>, bTemp: i32) {
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_rename_quotefix(%Q, sql)WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%'",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        zDb
    );
    if bTemp == 0 {
        nested!(
            db,
            format!(
                "UPDATE temp.{} SET sql = sqlite_rename_quotefix('temp', sql)WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%'",
                LEGACY_SCHEMA_TABLE
            )
        );
    }
}

/// Generate code to reload the schema for database iDb. And, if iDb!=1,
/// for the temp database as well.
fn renameReloadSchema(db: &mut Sqlite3, iDb: i32, p5: u16) {
    if db.ppr().pVdbe.is_some() {
        sqlite3ChangeCookie(db, iDb);
        sqlite3VdbeAddParseSchemaOp(db, iDb, None, p5);
        if iDb != 1 {
            sqlite3VdbeAddParseSchemaOp(db, 1, None, p5);
        }
    }
}

/// C: `void sqlite3AlterRenameTable( Parse *pParse, SrcList *pSrc, Token *pName )`
///
/// Generate code to implement the "ALTER TABLE xxx RENAME TO yyy"
/// command.
pub fn sqlite3AlterRenameTable(db: &mut Sqlite3, pSrc: SrcListId, pName: &Token) {
    alter_rename_table(db, pSrc, pName);
    // exit_rename_table:
    sqlite3SrcListDelete(db, pSrc);
}

fn alter_rename_table(db: &mut Sqlite3, pSrc: SrcListId, pName: &Token) {
    if db.mallocFailed != 0 {
        return;
    }
    debug_assert!(db.src(pSrc).nSrc() == 1);

    let pTab = match sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0)) {
        Some(t) => t,
        None => return,
    };
    let iDb = db.schema_index(pTab.borrow().pSchema);
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let zDb = zDb.as_deref();

    // Get a NULL terminated version of the new table name.
    let zName = match sqlite3NameFromToken(db, pName) {
        Some(z) => z,
        None => return,
    };

    // Check that a table or index named 'zName' does not already exist
    // in database iDb. If so, this is an error.
    if sqlite3FindTable(db, Some(&zName), zDb).is_some()
        || sqlite3FindIndex(db, Some(&zName), zDb).is_some()
        || sqlite3IsShadowTableOf(db, &pTab, Some(&zName)) != 0
    {
        errmsg!(
            db,
            "there is already another table or index with this name: %s",
            &zName
        );
        return;
    }

    // Make sure it is not a system table being altered, or a reserved name
    // that the table is being renamed to.
    if SQLITE_OK != isAlterableTable(db, &pTab) {
        return;
    }
    if SQLITE_OK != sqlite3CheckObjectName(db, Some(&zName), Some(b"table"), Some(&zName)) {
        return;
    }

    let zTabName = pTab.borrow().zName.clone();
    if pTab.borrow().IsView() {
        errmsg!(db, "view %s may not be altered", &zTabName);
        return;
    }

    // Invoke the authorization callback.
    if crate::auth::sqlite3AuthCheck(db, SQLITE_ALTER_TABLE, zDb, Some(&zTabName), None) != 0 {
        return;
    }

    if sqlite3ViewGetColumnNames(db, &pTab) != 0 {
        return;
    }
    // FIXME(vtab): when virtual tables are ported, a virtual table whose
    // module has an xRename() method must be renamed with OP_VRename below
    // (C: pVTab = sqlite3GetVTable(db, pTab); if xRename==0, pVTab = 0).
    let pVTab: Option<()> = None;

    // Begin a transaction for database iDb. Then modify the schema cookie
    // (since the ALTER TABLE modifies the schema). Call sqlite3MayAbort(),
    // as the scalar functions (e.g. sqlite_rename_table()) invoked by the
    // nested SQL may raise an exception.
    if !sqlite3GetVdbe(db) {
        return;
    }
    sqlite3MayAbort(db);

    // figure out how many UTF-8 characters are in zName
    let nTabName = crate::utf::sqlite3Utf8CharLen(Some(&zTabName), -1);

    // Rewrite all CREATE TABLE, INDEX, TRIGGER or VIEW statements in
    // the schema to use the new table name.
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, %d) WHERE (type!='index' OR tbl_name=%Q COLLATE nocase)AND   name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        zDb,
        &zTabName,
        &zName,
        (iDb == 1) as i32,
        &zTabName
    );

    // Update the tbl_name and name columns of the sqlite_schema table
    // as required.
    nested!(
        db,
        format!(
            "UPDATE %Q.{} SET tbl_name = %Q, name = CASE WHEN type='table' THEN %Q WHEN name LIKE 'sqliteX_autoindex%%' ESCAPE 'X'      AND type='index' THEN 'sqlite_autoindex_' || %Q || substr(name,%d+18) ELSE name END WHERE tbl_name=%Q COLLATE nocase AND (type='table' OR type='index' OR type='trigger');",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        &zName,
        &zName,
        &zName,
        nTabName,
        &zTabName
    );

    // If the sqlite_sequence table exists in this database, then update
    // it with the new table name.
    if sqlite3FindTable(db, Some(b"sqlite_sequence"), zDb).is_some() {
        nested!(
            db,
            "UPDATE \"%w\".sqlite_sequence set name = %Q WHERE name = %Q",
            zDb,
            &zName,
            &zTabName
        );
    }

    // If the table being renamed is not itself part of the temp database,
    // edit view and trigger definitions within the temp database
    // as required.
    if iDb != 1 {
        nested!(
            db,
            "UPDATE sqlite_temp_schema SET sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, 1), tbl_name = CASE WHEN tbl_name=%Q COLLATE nocase AND   sqlite_rename_test(%Q, sql, type, name, 1, 'after rename', 0) THEN %Q ELSE tbl_name END WHERE type IN ('view', 'trigger')",
            zDb,
            &zTabName,
            &zName,
            &zTabName,
            zDb,
            &zName
        );
    }

    // If this is a virtual table, invoke the xRename() function if
    // one is defined. The xRename() callback will modify the names
    // of any resources used by the v-table implementation (including other
    // SQLite tables) that are identified by the name of the virtual table.
    if pVTab.is_some() {
        db.pp().nMem += 1;
        let i = db.ppr().nMem;
        sqlite3VdbeLoadString(db.v(), i, &zName);
        sqlite3VdbeAddOp4(db.v(), OP_VRename, i, 0, 0, P4::None, P4_VTAB as i32);
    }

    renameReloadSchema(db, iDb, INITFLAG_AlterRename as u16);
    renameTestSchema(db, zDb, (iDb == 1) as i32, "after rename", 0);
}

/// Write code that will raise an error if the table described by
/// zDb and zTab is not empty.
fn sqlite3ErrorIfNotEmpty(db: &mut Sqlite3, zDb: Option<&[u8]>, zTab: &[u8], zErr: &str) {
    nested!(
        db,
        "SELECT raise(ABORT,%Q) FROM \"%w\".\"%w\"",
        zErr,
        zDb,
        zTab
    );
}

/// zCol is a column name used in an ALTER TABLE DROP, ADD or RENAME COLUMN
/// operation. zOp identifies the specific operation - "drop", "add",
/// "rename to" or "rename from". pTab is the table being altered.
///
/// If pTab has a rowid and zCol is a rowid alias, then SQLITE_ERROR is
/// returned and an error message left in pParse. Or, if zCol is not an
/// alias for "rowid" or pTab is not an intkey table, then SQLITE_OK is
/// returned.
fn isRowidAlias(db: &mut Sqlite3, pTab: &TableRef, zCol: &[u8], zOp: &str) -> i32 {
    let hasRowid = pTab.borrow().HasRowid();
    if hasRowid && sqlite3IsRowid(Some(zCol)) != 0 {
        errmsg!(db, "cannot %s rowid alias: %s", zOp, zCol);
        return SQLITE_ERROR;
    }
    SQLITE_OK
}

/// C: `void sqlite3AlterFinishAddColumn(Parse *pParse, Token *pColDef)`
///
/// This function is called after an "ALTER TABLE ... ADD" statement
/// has been parsed. Argument pColDef contains the text of the new
/// column definition.
///
/// The Table structure pParse->pNewTable was extended to include
/// the new column during parsing.
pub fn sqlite3AlterFinishAddColumn(db: &mut Sqlite3, pColDef: &Token) {
    if db.ppr().nErr != 0 {
        return;
    }
    debug_assert!(db.mallocFailed == 0);
    let pNew = db.ppr().pNewTable.clone().expect("pNewTable");

    let iDb = db.schema_index(pNew.borrow().pSchema);
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let zDb = zDb.as_deref();
    // Skip the "sqlite_altertab_" prefix on the name
    let zTab: ZStr = pNew.borrow().zName[16..].to_vec();
    let iColNew = (pNew.borrow().nCol - 1) as usize;
    let (colFlags, notNull, zCnName) = {
        let t = pNew.borrow();
        let c = &t.aCol[iColNew];
        (c.colFlags, c.notNull, c.zCnName.clone())
    };
    let mut pDflt = sqlite3ColumnExpr(db, &pNew, iColNew);
    let pTab = sqlite3FindTable(db, Some(&zTab), zDb).expect("altered table");
    let zTabName = pTab.borrow().zName.clone();

    // Invoke the authorization callback.
    if crate::auth::sqlite3AuthCheck(db, SQLITE_ALTER_TABLE, zDb, Some(&zTabName), None) != 0 {
        return;
    }

    // Check that the new column is not specified as PRIMARY KEY or UNIQUE,
    // or a rowid alias. If there is a NOT NULL constraint, then the default
    // value for the column must not be NULL.
    if (colFlags & COLFLAG_PRIMKEY) != 0 {
        errmsg!(db, "Cannot add a PRIMARY KEY column");
        return;
    }
    if !pNew.borrow().pIndex.is_empty() {
        errmsg!(db, "Cannot add a UNIQUE column");
        return;
    }
    if isRowidAlias(db, &pTab, &zCnName, "add") != 0 {
        return;
    }
    if (colFlags & COLFLAG_GENERATED) == 0 {
        // If the default value for the new column was specified with a
        // literal NULL, then set pDflt to 0. This simplifies checking
        // for an SQL NULL default below.
        debug_assert!(pDflt.is_null() || db.ex(pDflt).op == TK_SPAN);
        if pDflt.nn() {
            let l = db.ex(pDflt).pLeft;
            if db.ex(l).op == TK_NULL {
                pDflt = ExprId::NULL;
            }
        }
        debug_assert!(pNew.borrow().IsOrdinaryTable());
        if (db.flags & SQLITE_ForeignKeys) != 0
            && !pNew.borrow().u.tab.pFKey.is_empty()
            && pDflt.nn()
        {
            sqlite3ErrorIfNotEmpty(
                db,
                zDb,
                &zTab,
                "Cannot add a REFERENCES column with non-NULL default value",
            );
        }
        if notNull != 0 && pDflt.is_null() {
            sqlite3ErrorIfNotEmpty(
                db,
                zDb,
                &zTab,
                "Cannot add a NOT NULL column with default value NULL",
            );
        }

        // Ensure the default expression is something that
        // sqlite3ValueFromExpr() can handle (i.e. not CURRENT_TIME etc.)
        if pDflt.nn() {
            let mut pVal: Option<Box<Mem>> = None;
            let rc = crate::vdbemem::sqlite3ValueFromExpr(
                db,
                pDflt,
                SQLITE_UTF8,
                SQLITE_AFF_BLOB,
                &mut pVal,
            );
            debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
            if rc != SQLITE_OK {
                debug_assert!(db.mallocFailed == 1);
                return;
            }
            if pVal.is_none() {
                sqlite3ErrorIfNotEmpty(
                    db,
                    zDb,
                    &zTab,
                    "Cannot add a column with non-constant default",
                );
            }
            if let Some(mut v) = pVal {
                crate::vdbemem::sqlite3ValueFree(db, &mut v);
            }
        }
    } else if (colFlags & COLFLAG_STORED) != 0 {
        sqlite3ErrorIfNotEmpty(db, zDb, &zTab, "cannot add a STORED column");
    }

    // Modify the CREATE TABLE statement.
    let mut zCol: ZStr = pColDef.text().to_vec();
    {
        // C: zEnd = &zCol[pColDef->n-1]; while( zEnd>zCol && ... ) *zEnd-- = 0;
        while zCol.len() > 1 {
            let c = zCol[zCol.len() - 1];
            if c == b';' || crate::global::sqlite3Isspace(c) {
                zCol.pop();
            } else {
                break;
            }
        }
        let zCol = cstr(&zCol).to_vec();
        // substr() operations on characters, but addColOffset is in bytes.
        // So we have to use printf() to translate between these units:
        debug_assert!(pTab.borrow().IsOrdinaryTable());
        debug_assert!(pNew.borrow().IsOrdinaryTable());
        let addColOffset = pNew.borrow().u.tab.addColOffset;
        nested!(
            db,
            format!(
                "UPDATE \"%w\".{} SET sql = printf('%%.%ds, ',sql) || %Q || substr(sql,1+length(printf('%%.%ds',sql))) WHERE type = 'table' AND name = %Q",
                LEGACY_SCHEMA_TABLE
            ),
            zDb,
            addColOffset,
            &zCol,
            addColOffset,
            &zTab
        );
    }

    if sqlite3GetVdbe(db) {
        // Make sure the schema version is at least 3.  But do not upgrade
        // from less than 3 to 4, as that will corrupt any preexisting DESC
        // index.
        let r1 = sqlite3GetTempReg(db);
        sqlite3VdbeAddOp3(db.v(), OP_ReadCookie, iDb, r1, BTREE_FILE_FORMAT);
        sqlite3VdbeUsesBtree(db.v(), iDb);
        sqlite3VdbeAddOp2(db.v(), OP_AddImm, r1, -2);
        let a = sqlite3VdbeCurrentAddr(db.v());
        sqlite3VdbeAddOp2(db.v(), OP_IfPos, r1, a + 2);
        sqlite3VdbeAddOp3(db.v(), OP_SetCookie, iDb, BTREE_FILE_FORMAT, 3);
        sqlite3ReleaseTempReg(db, r1);

        // Reload the table definition
        renameReloadSchema(db, iDb, INITFLAG_AlterAdd as u16);

        // Verify that constraints are still satisfied
        if pNew.borrow().pCheck.is_some()
            || (notNull != 0 && (colFlags & COLFLAG_GENERATED) != 0)
            || (pTab.borrow().tabFlags & TF_Strict) != 0
        {
            db.pp().colNamesSet = 1;
            nested!(
                db,
                "SELECT CASE WHEN quick_check GLOB 'CHECK*' THEN raise(ABORT,'CHECK constraint failed') WHEN quick_check GLOB 'non-* value in*' THEN raise(ABORT,'type mismatch on DEFAULT') ELSE raise(ABORT,'NOT NULL constraint failed') END  FROM pragma_quick_check(%Q,%Q) WHERE quick_check GLOB 'CHECK*' OR quick_check GLOB 'NULL*' OR quick_check GLOB 'non-* value in*'",
                &zTab,
                zDb
            );
        }
    }
}

/// C: `void sqlite3AlterBeginAddColumn(Parse *pParse, SrcList *pSrc)`
///
/// This function is called by the parser after the table-name in
/// an "ALTER TABLE <table-name> ADD" statement is parsed. Argument
/// pSrc is the full-name of the table being altered.
///
/// This routine makes a (partial) copy of the Table structure
/// for the table being altered and sets Parse.pNewTable to point
/// to it. Routines called by the parser as the column definition
/// is parsed (i.e. sqlite3AddColumn()) add the new Column data to
/// the copy. The copy of the Table structure is deleted by tokenize.c
/// after parsing is finished.
///
/// Routine sqlite3AlterFinishAddColumn() will be called to complete
/// coding the "ALTER TABLE ... ADD" statement.
pub fn sqlite3AlterBeginAddColumn(db: &mut Sqlite3, pSrc: SrcListId) {
    alter_begin_add_column(db, pSrc);
    // exit_begin_add_column:
    sqlite3SrcListDelete(db, pSrc);
}

fn alter_begin_add_column(db: &mut Sqlite3, pSrc: SrcListId) {
    // Look up the table being altered.
    debug_assert!(db.ppr().pNewTable.is_none());
    if db.mallocFailed != 0 {
        return;
    }
    let pTab = match sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0)) {
        Some(t) => t,
        None => return,
    };

    if pTab.borrow().IsVirtual() {
        errmsg!(db, "virtual tables may not be altered");
        return;
    }

    // Make sure this is not an attempt to ALTER a view.
    if pTab.borrow().IsView() {
        errmsg!(db, "Cannot add a column to a view");
        return;
    }
    if SQLITE_OK != isAlterableTable(db, &pTab) {
        return;
    }

    sqlite3MayAbort(db);
    debug_assert!(pTab.borrow().IsOrdinaryTable());
    debug_assert!(pTab.borrow().u.tab.addColOffset > 0);
    let iDb = db.schema_index(pTab.borrow().pSchema);

    // Put a copy of the Table struct in Parse.pNewTable for the
    // sqlite3AddColumn() function and friends to modify.  But modify
    // the name by adding an "sqlite_altertab_" prefix.  By adding this
    // prefix, we insure that the name will not collide with an existing
    // table because user table are not allowed to have the "sqlite_"
    // prefix on their name.
    let (nCol, aCol, zTabName, dflt, addColOffset) = {
        let t = pTab.borrow();
        (
            t.nCol,
            t.aCol.clone(),
            t.zName.clone(),
            t.u.tab.pDfltList.clone(),
            t.u.tab.addColOffset,
        )
    };
    debug_assert!(nCol > 0);
    let zName = mprintf!(db, "sqlite_altertab_%s", &zTabName);
    let mut aCol = aCol;
    aCol.truncate(nCol as usize);
    for pCol in aCol.iter_mut() {
        pCol.hName = sqlite3StrIHash(Some(&pCol.zCnName));
    }
    let pOldDflt = db.tree(&dflt);
    let pDfltList = sqlite3ExprListDup(db, pOldDflt, 0);
    let mut pNew = Table::default();
    pNew.nTabRef = 1;
    pNew.nCol = nCol;
    pNew.aCol = aCol;
    pNew.zName = zName;
    pNew.u.tab.pDfltList = Tree::from_id(pDfltList);
    pNew.pSchema = db.aDb[iDb as usize].pSchema.id;
    pNew.u.tab.addColOffset = addColOffset;
    db.pp().pNewTable = Some(Rc::new(RefCell::new(pNew)));
}

/// Parameter pTab is the subject of an ALTER TABLE ... RENAME COLUMN
/// command. This function checks if the table is a view or virtual
/// table (columns of views or virtual tables may not be renamed). If so,
/// it loads an error message into pParse and returns non-zero.
///
/// Or, if pTab is not a view or virtual table, zero is returned.
fn isRealTable(db: &mut Sqlite3, pTab: &TableRef, iOp: usize) -> i32 {
    let (isView, isVirtual, zName) = {
        let t = pTab.borrow();
        (t.IsView(), t.IsVirtual(), t.zName.clone())
    };
    let mut zType: Option<&str> = None;
    if isView {
        zType = Some("view");
    }
    if isVirtual {
        zType = Some("virtual table");
    }
    if let Some(zType) = zType {
        const azMsg: [&str; 3] = [
            "rename columns of",
            "drop column from",
            "edit constraints of",
        ];
        debug_assert!(iOp < azMsg.len());
        errmsg!(db, "cannot %s %s \"%s\"", azMsg[iOp], zType, &zName);
        return 1;
    }
    0
}

/// C: `void sqlite3AlterRenameColumn( Parse *pParse, SrcList *pSrc, Token *pOld, Token *pNew )`
///
/// Handles the following parser reduction:
///
///  cmd ::= ALTER TABLE pSrc RENAME COLUMN pOld TO pNew
pub fn sqlite3AlterRenameColumn(db: &mut Sqlite3, pSrc: SrcListId, pOld: &Token, pNew: &Token) {
    alter_rename_column(db, pSrc, pOld, pNew);
    // exit_rename_column:
    sqlite3SrcListDelete(db, pSrc);
}

fn alter_rename_column(db: &mut Sqlite3, pSrc: SrcListId, pOld: &Token, pNew: &Token) {
    // Locate the table to be altered
    let pTab = match sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0)) {
        Some(t) => t,
        None => return,
    };

    // Cannot alter a system table
    if SQLITE_OK != isAlterableTable(db, &pTab) {
        return;
    }
    if SQLITE_OK != isRealTable(db, &pTab, 0) {
        return;
    }

    // Which schema holds the table to be altered
    let iSchema = db.schema_index(pTab.borrow().pSchema);
    debug_assert!(iSchema >= 0);
    let zDb = db.aDb[iSchema as usize].zDbSName.clone();
    let zDb = zDb.as_deref();
    let zTabName = pTab.borrow().zName.clone();

    // Invoke the authorization callback.
    if crate::auth::sqlite3AuthCheck(db, SQLITE_ALTER_TABLE, zDb, Some(&zTabName), None) != 0 {
        return;
    }

    // Make sure the old name really is a column name in the table to be
    // altered.  Set iCol to be the index of the column being renamed
    let zOld = match sqlite3NameFromToken(db, pOld) {
        Some(z) => z,
        None => return,
    };
    let iCol = sqlite3ColumnIndex(&pTab, Some(&zOld));
    if iCol < 0 {
        errmsg!(db, "no such column: \"%T\"", pOld);
        return;
    }

    // Ensure the schema contains no double-quoted strings
    renameTestSchema(db, zDb, (iSchema == 1) as i32, "", 0);
    renameFixQuotes(db, zDb, (iSchema == 1) as i32);

    // Do the rename operation using a recursive UPDATE statement that
    // uses the sqlite_rename_column() SQL function to compute the new
    // CREATE statement text for the sqlite_schema table.
    sqlite3MayAbort(db);
    let zNew = match sqlite3NameFromToken(db, pNew) {
        Some(z) => z,
        None => return,
    };
    if isRowidAlias(db, &pTab, &zOld, "rename from") != 0 {
        return;
    }
    if isRowidAlias(db, &pTab, &zNew, "rename to") != 0 {
        return;
    }
    debug_assert!(pNew.n > 0);
    let bQuote = crate::global::sqlite3Isquote(pNew.at(0)) as i32;
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, %d) WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X'  AND (type != 'index' OR tbl_name = %Q)",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        zDb,
        &zTabName,
        iCol,
        &zNew,
        bQuote,
        (iSchema == 1) as i32,
        &zTabName
    );

    nested!(
        db,
        format!(
            "UPDATE temp.{} SET sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, 1) WHERE type IN ('trigger', 'view')",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        &zTabName,
        iCol,
        &zNew,
        bQuote
    );

    // Drop and reload the database schema.
    renameReloadSchema(db, iSchema, INITFLAG_AlterRename as u16);
    renameTestSchema(db, zDb, (iSchema == 1) as i32, "after rename", 1);
}

// ---------------------------------------------------------------------------
// RenameToken machinery
// ---------------------------------------------------------------------------

/// The context of an ALTER TABLE RENAME COLUMN operation that gets passed
/// down into the Walker.
#[derive(Default)]
struct RenameCtx {
    /// List of tokens to overwrite (C pList; index 0 is the list head).
    pList: Vec<RenameToken>,
    /// Number of tokens in pList
    nList: i32,
    /// Index of column being renamed
    iCol: i32,
    /// Table being ALTERed
    pTab: Option<TableRef>,
}

/// C: `const void * sqlite3RenameTokenMap( Parse *pParse, const void *pPtr, const Token *pToken )`
///
/// Remember that the parser tree element pPtr was created using
/// the token pToken.
///
/// In other words, construct a new RenameToken object and add it
/// to the list of RenameToken objects currently being built up
/// in pParse->pRename.
///
/// Rust: the C function returns pPtr (for tail calls); callers keep their
/// own value instead.
pub fn sqlite3RenameTokenMap(db: &mut Sqlite3, pPtr: RenameKey, pToken: &Token) {
    if db.ppr().eParseMode != PARSE_MODE_UNMAP {
        db.pp().pRename.insert(
            0,
            RenameToken {
                p: pPtr,
                t: pToken.clone(),
            },
        );
    }
}

/// C: `void sqlite3RenameTokenRemap(Parse *pParse, const void *pTo, const void *pFrom)`
/// Rust: the C `void*` keys are `RenameKey`s; `pTo==None` is C NULL.
///
/// It is assumed that there is already a RenameToken object associated
/// with parse tree element pFrom. This function remaps the associated
/// token to parse tree element pTo.
pub fn sqlite3RenameTokenRemap(db: &mut Sqlite3, pTo: Option<RenameKey>, pFrom: RenameKey) {
    for p in db.pp().pRename.iter_mut() {
        if p.p == pFrom {
            p.p = pTo.unwrap_or(RENAME_KEY_NULL);
            break;
        }
    }
}

/// Walker callback used by sqlite3RenameExprUnmap().
fn renameUnmapExprCb(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    sqlite3RenameTokenRemap(w.db, None, RenameKey::Expr(pExpr));
    if w.db.ex(pExpr).UseYTab() {
        sqlite3RenameTokenRemap(w.db, None, RenameKey::ExprTab(pExpr));
    }
    WRC_Continue
}

/// Iterate through the Select objects that are part of WITH clauses
/// attached to select statement pSelect.
fn renameWalkWith(w: &mut Walker<'_>, pSelect: SelectId) {
    let pWith = w.db.sel(pSelect).pWith;
    if pWith.nn() {
        let mut pCopy = WithId::NULL;
        debug_assert!(!w.db.arena.with(pWith).a.is_empty());
        let s0 = w.db.arena.with(pWith).a[0].pSelect;
        if (w.db.sel(s0).selFlags & SF_Expanded) == 0 {
            // Push a copy of the With object onto the with-stack. We use a
            // copy here as the original will be expanded and resolved
            // (flags SF_Expanded and SF_Resolved) below. And the parser code
            // that uses the with-stack fails if the Select objects on it
            // have already been expanded and resolved.
            pCopy = sqlite3WithDup(w.db, pWith);
            pCopy = sqlite3WithPush(w.db, pCopy, 1);
        }
        let nCte = w.db.arena.with(pWith).a.len();
        for i in 0..nCte {
            let (p, pCols) = {
                let c = &w.db.arena.with(pWith).a[i];
                (c.pSelect, c.pCols)
            };
            let sNC = w.db.arena.new_nc(NameContext::default());
            if pCopy.nn() {
                sqlite3SelectPrep(w.db, p, sNC);
            }
            if w.db.mallocFailed != 0 {
                return;
            }
            sqlite3WalkSelect(w, p);
            sqlite3RenameExprlistUnmap(w.db, pCols);
        }
        if pCopy.nn() && w.db.ppr().pWith == pCopy {
            let outer = w.db.arena.with(pCopy).pOuter;
            w.db.pp().pWith = outer;
        }
    }
}

/// Unmap all tokens in the IdList object passed as the second argument.
fn unmapColumnIdlistNames(db: &mut Sqlite3, pIdList: IdListId) {
    debug_assert!(pIdList.nn());
    let n = db.idl(pIdList).nId();
    for ii in 0..n {
        sqlite3RenameTokenRemap(db, None, rename_key_idlist_item(pIdList, ii));
    }
}

/// Walker callback used by sqlite3RenameExprUnmap().
fn renameUnmapSelectCb(w: &mut Walker<'_>, p: SelectId) -> i32 {
    if w.db.ppr().nErr != 0 {
        return WRC_Abort;
    }
    let (selFlags, pList, pSrc) = {
        let s = w.db.sel(p);
        (s.selFlags, s.pEList, s.pSrc)
    };
    if (selFlags & (SF_View | SF_CopyCte)) != 0 {
        return WRC_Prune;
    }
    if pList.nn() {
        let n = w.db.el(pList).nExpr();
        for i in 0..n {
            let ok = {
                let it = &w.db.el(pList).a[i as usize];
                it.zEName.is_some() && it.fg.eEName == ENAME_NAME
            };
            if ok {
                sqlite3RenameTokenRemap(w.db, None, rename_key_elist_item(pList, i));
            }
        }
    }
    if pSrc.nn() {
        // Every Select as a SrcList, even if it is empty
        let n = w.db.src(pSrc).nSrc();
        for i in 0..n {
            let it = SrcItemId(pSrc, i as u32);
            sqlite3RenameTokenRemap(w.db, None, rename_key_srcitem_name(it));
            let (isUsing, pOn, pUsing) = {
                let x = w.db.item(it);
                (x.fg.isUsing, x.u3.pOn, x.u3.pUsing)
            };
            if isUsing == 0 {
                sqlite3WalkExpr(w, pOn);
            } else {
                unmapColumnIdlistNames(w.db, pUsing);
            }
        }
    }

    renameWalkWith(w, p);
    WRC_Continue
}

/// C: `void sqlite3RenameExprUnmap(Parse *pParse, Expr *pExpr)`
///
/// Remove all nodes that are part of expression pExpr from the rename list.
pub fn sqlite3RenameExprUnmap(db: &mut Sqlite3, pExpr: ExprId) {
    let eMode = db.ppr().eParseMode;
    db.pp().eParseMode = PARSE_MODE_UNMAP;
    {
        let mut sWalker = Walker::new(db);
        sWalker.xExprCallback = Some(renameUnmapExprCb);
        sWalker.xSelectCallback = Some(renameUnmapSelectCb);
        sqlite3WalkExpr(&mut sWalker, pExpr);
    }
    db.pp().eParseMode = eMode;
}

/// C: `void sqlite3RenameExprlistUnmap(Parse *pParse, ExprList *pEList)`
///
/// Remove all nodes that are part of expression-list pEList from the
/// rename list.
pub fn sqlite3RenameExprlistUnmap(db: &mut Sqlite3, pEList: ExprListId) {
    if pEList.nn() {
        {
            let mut sWalker = Walker::new(db);
            sWalker.xExprCallback = Some(renameUnmapExprCb);
            sqlite3WalkExprList(&mut sWalker, pEList);
        }
        let n = db.el(pEList).nExpr();
        for i in 0..n {
            if db.el(pEList).a[i as usize].fg.eEName == ENAME_NAME {
                sqlite3RenameTokenRemap(db, None, rename_key_elist_item(pEList, i));
            }
        }
    }
}

/// Search the Parse object passed as the first argument for a RenameToken
/// object associated with parse tree element pPtr. If found, return a
/// copy of it. Otherwise, return NULL.
///
/// If the second argument passed to this function is not NULL and a
/// matching RenameToken object is found, remove it from the Parse object
/// and add it to the list maintained by the RenameCtx object.
fn renameTokenFind(
    db: &mut Sqlite3,
    pCtx: Option<&mut RenameCtx>,
    pPtr: RenameKey,
) -> Option<RenameToken> {
    if pPtr == RENAME_KEY_NULL {
        return None;
    }
    let pos = db.ppr().pRename.iter().position(|t| t.p == pPtr)?;
    match pCtx {
        Some(c) => {
            let pToken = db.pp().pRename.remove(pos);
            c.pList.insert(0, pToken.clone());
            c.nList += 1;
            Some(pToken)
        }
        None => Some(db.ppr().pRename[pos].clone()),
    }
}

/// The RenameCtx of a walker (C pWalker->u.pRename).
fn wctx<'b>(w: &'b mut WalkerU) -> &'b mut RenameCtx {
    w.any
        .as_mut()
        .and_then(|a| a.downcast_mut::<RenameCtx>())
        .expect("RenameCtx")
}

/// Run `f` with a walker whose u.pRename is `ctx`.
fn with_rename_walker(
    db: &mut Sqlite3,
    ctx: &mut RenameCtx,
    xExpr: XExprCallback,
    xSelect: XSelectCallback,
    f: impl FnOnce(&mut Walker<'_>),
) {
    let c = std::mem::take(ctx);
    let back = {
        let mut sWalker = Walker::new(db);
        sWalker.xExprCallback = Some(xExpr);
        sWalker.xSelectCallback = Some(xSelect);
        sWalker.u.any = Some(Box::new(c));
        f(&mut sWalker);
        sWalker.u.any.take()
    };
    *ctx = *back
        .expect("RenameCtx")
        .downcast::<RenameCtx>()
        .ok()
        .expect("RenameCtx");
}

/// This is a Walker select callback. It does nothing. It is only required
/// because without a dummy callback, sqlite3WalkExpr() and similar do not
/// descend into sub-select statements.
fn renameColumnSelectCb(w: &mut Walker<'_>, p: SelectId) -> i32 {
    if (w.db.sel(p).selFlags & (SF_View | SF_CopyCte)) != 0 {
        return WRC_Prune;
    }
    renameWalkWith(w, p);
    WRC_Continue
}

/// This is a Walker expression callback.
///
/// For every TK_COLUMN node in the expression tree, search to see
/// if the column being references is the column being renamed by an
/// ALTER TABLE statement.  If it is, then attach its associated
/// RenameToken object to the list of RenameToken objects being
/// constructed in RenameCtx object at pWalker->u.pRename.
fn renameColumnExprCb(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (op, iColumn, useYTab, pTab) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iColumn as i32, e.UseYTab(), e.y.pTab.clone())
    };
    let pTriggerTab = w.db.ppr().pTriggerTab.clone();
    let p = wctx(&mut w.u);
    if op == TK_TRIGGER && iColumn == p.iCol && tab_eq(&pTriggerTab, &p.pTab) {
        renameTokenFind(w.db, Some(p), RenameKey::Expr(pExpr));
    } else if op == TK_COLUMN && iColumn == p.iCol && useYTab && tab_eq(&p.pTab, &pTab) {
        renameTokenFind(w.db, Some(p), RenameKey::Expr(pExpr));
    }
    WRC_Continue
}

/// The RenameCtx contains a list of tokens that reference a column that
/// is being renamed by an ALTER TABLE statement.  Return the "last"
/// RenameToken in the RenameCtx and remove that RenameToken from the
/// RenameContext.  "Last" means the last RenameToken encountered when
/// the input SQL is parsed from left to right.  Repeated calls to this
/// routine return all column name tokens in the order that they are
/// encountered in the SQL statement.
fn renameColumnTokenNext(pCtx: &mut RenameCtx) -> RenameToken {
    let mut iBest = 0;
    for i in 1..pCtx.pList.len() {
        if pCtx.pList[i].t.off > pCtx.pList[iBest].t.off {
            iBest = i;
        }
    }
    pCtx.pList.remove(iBest)
}

/// Set the error message of the context passed as the first argument to
/// the result of formatting zFmt using printf() style formatting.
fn errorMPrintf(pCtx: &mut Context, zFmt: &str, args: &[PfArg]) {
    let zErr = crate::printf::sqlite3VMPrintf(pCtx.db, zFmt.as_bytes(), args);
    sqlite3_result_error(pCtx, Some(&zErr), -1);
}

/// An error occurred while parsing or otherwise processing a database
/// object (either pParse->pNewTable, pNewIndex or pNewTrigger) as part of
/// an ALTER TABLE RENAME COLUMN program. The error message emitted by the
/// sub-routine is currently stored in pParse->zErrMsg. This function
/// adds context to the error message and then stores it in pCtx.
///
/// Rust: pType/pObject are the texts of the two values; zErrMsg is the
/// error message of the rename parse.
fn renameColumnParseError(
    pCtx: &mut Context,
    zWhen: &[u8],
    zT: Option<ZStr>,
    zN: Option<ZStr>,
    zErrMsg: Option<ZStr>,
) {
    let zErr = mprintf!(
        pCtx.db,
        "error in %s %s%s%s: %s",
        zT.as_deref(),
        zN.as_deref(),
        if !zWhen.is_empty() { " " } else { "" },
        zWhen,
        zErrMsg.as_deref()
    );
    sqlite3_result_error(pCtx, Some(&zErr), -1);
}

/// For each name in the the expression-list pEList (i.e. each
/// pEList->a[i].zName) that matches the string in zOld, extract the
/// corresponding rename-token from Parse object pParse and add it
/// to the RenameCtx pCtx.
fn renameColumnElistNames(db: &mut Sqlite3, pCtx: &mut RenameCtx, pEList: ExprListId, zOld: &[u8]) {
    if pEList.nn() {
        let n = db.el(pEList).nExpr();
        for i in 0..n {
            let ok = {
                let it = &db.el(pEList).a[i as usize];
                it.fg.eEName == ENAME_NAME
                    && it.zEName.is_some()
                    && sqlite3_stricmp(it.zEName.as_deref(), Some(zOld)) == 0
            };
            if ok {
                renameTokenFind(db, Some(pCtx), rename_key_elist_item(pEList, i));
            }
        }
    }
}

/// For each name in the the id-list pIdList (i.e. each pIdList->a[i].zName)
/// that matches the string in zOld, extract the corresponding rename-token
/// from Parse object pParse and add it to the RenameCtx pCtx.
fn renameColumnIdlistNames(db: &mut Sqlite3, pCtx: &mut RenameCtx, pIdList: IdListId, zOld: &[u8]) {
    if pIdList.nn() {
        let n = db.idl(pIdList).nId();
        for i in 0..n {
            let ok = {
                let it = &db.idl(pIdList).a[i as usize];
                sqlite3_stricmp(it.zName.as_deref(), Some(zOld)) == 0
            };
            if ok {
                renameTokenFind(db, Some(pCtx), rename_key_idlist_item(pIdList, i));
            }
        }
    }
}

/// Parse the SQL statement zSql using a new Parse object (pushed onto
/// db.parses; it must be released with renameParseCleanup()).  The
/// Parse object is initialized by this function before it is used.
fn renameParseSql(db: &mut Sqlite3, zDb: Option<&[u8]>, zSql: Option<&[u8]>, bTemp: bool) -> i32 {
    crate::prepare::sqlite3ParseObjectInit(db);
    let zSql = match zSql {
        None => return SQLITE_NOMEM,
        Some(z) => cstr(z),
    };
    if sqlite3_strnicmp(Some(zSql), Some(b"CREATE "), 7) != 0 {
        return crate::mainapi::sqlite3CorruptError(line!() as i32);
    }
    if bTemp {
        db.init.iDb = 1;
    } else {
        let iDb = sqlite3FindDbName(db, zDb);
        debug_assert!(iDb >= 0 && iDb <= 0xff);
        db.init.iDb = iDb as u8;
    }
    {
        let p = db.pp();
        p.eParseMode = PARSE_MODE_RENAME;
        p.nQueryLoop = 1;
    }
    let flags = db.flags;
    db.flags |= SQLITE_Comments;
    let z: Rc<[u8]> = Rc::from(zSql);
    let mut rc = crate::tokenize::sqlite3RunParser(db, &z, 0);
    db.flags = flags;
    if db.mallocFailed != 0 {
        rc = SQLITE_NOMEM;
    }
    if rc == SQLITE_OK {
        let p = db.ppr();
        if p.pNewTable.is_none() && p.pNewIndex.is_none() && p.pNewTrigger.is_none() {
            rc = crate::mainapi::sqlite3CorruptError(line!() as i32);
        }
    }

    db.init.iDb = 0;
    rc
}

/// This function edits SQL statement zSql, replacing each token identified
/// by the linked list pRename with the text of zNew. If argument bQuote is
/// true, then zNew is always quoted first.
///
/// Rust: returns the edited SQL (the C function loads it into the context
/// as the result); errors cannot happen (no OOM).
fn renameEditSql(
    db: &mut Sqlite3,
    pRename: &mut RenameCtx,
    zSql: &[u8],
    zNew: Option<&[u8]>,
    bQuote: i32,
) -> ZStr {
    let zSql = cstr(zSql);
    let nSql = zSql.len();
    let mut zQuot: ZStr = Vec::new();
    let mut nQuot = 0usize;

    if let Some(zNew) = zNew {
        // Set zQuot to point to a buffer containing a quoted copy of the
        // identifier zNew. If the corresponding identifier in the original
        // ALTER TABLE statement was quoted (bQuote==1), then set zNew to
        // point to zQuot so that all substitutions are made using the
        // quoted version of the new column name.
        zQuot = mprintf!(db, "\"%w\" ", zNew);
        nQuot = zQuot.len() - 1;
    }

    // At this point pRename->pList contains a list of RenameToken objects
    // corresponding to all tokens in the input SQL that must be replaced
    // with the new column name, or with single-quoted versions of
    // themselves.  All that remains is to construct and return the edited
    // SQL string.
    let mut zOut: ZStr = zSql.to_vec();
    while !pRename.pList.is_empty() {
        let pBest = renameColumnTokenNext(pRename);
        let n = pBest.t.n as usize;
        let zReplace: ZStr = if let Some(zNew) = zNew {
            if bQuote == 0 && crate::tokenize::sqlite3IsIdChar(pBest.t.at(0)) != 0 {
                zNew.to_vec()
            } else {
                let mut nReplace = nQuot;
                if pBest.t.at(n as isize) == b'"' {
                    nReplace += 1;
                }
                zQuot[..nReplace].to_vec()
            }
        } else {
            // Dequote the double-quoted token. Then requote it again, this
            // time using single quotes. If the character immediately
            // following the original token within the input SQL was a
            // single quote ('), then add another space after the new,
            // single-quoted version of the token. This is so that
            // (SELECT "string"'alias') maps to (SELECT 'string' 'alias'),
            // and not (SELECT 'string''alias').
            let mut zBuf1 = pBest.t.text().to_vec();
            dequote(&mut zBuf1);
            let zBuf1 = cstr(&zBuf1).to_vec();
            crate::printf::sqlite3_snprintf(
                (nSql * 2) as i32,
                b"%Q%s",
                &[
                    PfArg::from(&zBuf1),
                    PfArg::from(if pBest.t.at(n as isize) == b'\'' {
                        " "
                    } else {
                        ""
                    }),
                ],
            )
        };

        let iOff = pBest.t.off as usize;
        let end = (iOff + n).min(zOut.len());
        zOut.splice(iOff..end, zReplace);
    }
    zOut
}

/// Set all pEList->a[].fg.eEName fields in the expression-list to val.
fn renameSetENames(db: &mut Sqlite3, pEList: ExprListId, val: u8) {
    debug_assert!(val == ENAME_NAME || val == ENAME_TAB || val == ENAME_SPAN);
    if pEList.nn() {
        for it in db.elm(pEList).a.iter_mut() {
            it.fg.eEName = val & 0x3;
        }
    }
}

/// Resolve all symbols in the trigger at pParse->pNewTrigger, assuming
/// it was read from the schema of database zDb. Return SQLITE_OK if
/// successful. Otherwise, return an SQLite error code and leave an error
/// message in the Parse object.
fn renameResolveTrigger(db: &mut Sqlite3) -> i32 {
    let pNew = db.ppr().pNewTrigger.clone().expect("pNewTrigger");
    let mut rc = SQLITE_OK;

    let sNC = db.arena.new_nc(NameContext::default());
    let (table, pTabSchema, op, whenT, stepsT) = {
        let t = pNew.borrow();
        (
            t.table.clone(),
            t.pTabSchema,
            t.op,
            t.pWhen.clone(),
            t.step_list.clone(),
        )
    };
    let iTabDb = db.schema_index(pTabSchema);
    let zDbName = db.aDb[iTabDb as usize].zDbSName.clone();
    let pTriggerTab = sqlite3FindTable(db, Some(&table), zDbName.as_deref());
    db.pp().pTriggerTab = pTriggerTab.clone();
    db.pp().eTriggerOp = op;
    // ALWAYS() because if the table of the trigger does not exist, the
    // error would have been hit before this point
    if let Some(t) = pTriggerTab.as_ref() {
        rc = (sqlite3ViewGetColumnNames(db, t) != 0) as i32;
    }

    // Resolve symbols in WHEN clause
    let pWhen = db.tree(&whenT);
    if rc == SQLITE_OK && pWhen.nn() {
        rc = sqlite3ResolveExprNames(db, sNC, pWhen);
    }

    let mut pStep = db.tree(&stepsT);
    while rc == SQLITE_OK && pStep.nn() {
        let (pSelect, pStepSrc, pExprList, pWhere, pUpsert, pNext) = {
            let s = db.arena.tstep(pStep);
            (s.pSelect, s.pSrc, s.pExprList, s.pWhere, s.pUpsert, s.pNext)
        };
        if pSelect.nn() {
            sqlite3SelectPrep(db, pSelect, sNC);
            if db.ppr().nErr != 0 {
                rc = db.ppr().rc;
            }
        }
        if rc == SQLITE_OK && pStepSrc.nn() {
            let pSrc = sqlite3SrcListDup(db, pStepSrc, 0);
            if pSrc.nn() {
                let pSel = sqlite3SelectNew(
                    db,
                    pExprList,
                    pSrc,
                    ExprId::NULL,
                    ExprListId::NULL,
                    ExprId::NULL,
                    ExprListId::NULL,
                    0,
                    ExprId::NULL,
                );
                if pSel.is_null() {
                    db.arena.tstepm(pStep).pExprList = ExprListId::NULL;
                    rc = SQLITE_NOMEM;
                } else {
                    // pStep->pExprList contains an expression-list used for
                    // an UPDATE statement. So the a[].zEName values are the
                    // RHS of the "<col> = <expr>" clauses of the UPDATE
                    // statement. So, before running SelectPrep(), change all
                    // the eEName values in pStep->pExprList to ENAME_SPAN
                    // (from their current value of ENAME_NAME). This is to
                    // prevent any ids in ON() clauses that are part of pSrc
                    // from being incorrectly resolved against the a[].zEName
                    // values as if they were column aliases.
                    renameSetENames(db, pExprList, ENAME_SPAN);
                    sqlite3SelectPrep(db, pSel, NcId::NULL);
                    renameSetENames(db, pExprList, ENAME_NAME);
                    rc = if db.ppr().nErr != 0 {
                        SQLITE_ERROR
                    } else {
                        SQLITE_OK
                    };
                    debug_assert!(pExprList.is_null() || pExprList == db.sel(pSel).pEList);
                    debug_assert!(pSrc == db.sel(pSel).pSrc);
                    if pExprList.nn() {
                        db.selm(pSel).pEList = ExprListId::NULL;
                    }
                    db.selm(pSel).pSrc = SrcListId::NULL;
                    sqlite3SelectDelete(db, pSel);
                }
                {
                    let n = db.src(pStepSrc).nSrc();
                    let mut i = 0;
                    while i < n && rc == SQLITE_OK {
                        let sub = {
                            let p = db.item(SrcItemId(pStepSrc, i as u32));
                            if p.fg.isSubquery != 0 {
                                p.u4.pSubq.as_ref().map(|q| q.pSelect)
                            } else {
                                None
                            }
                        };
                        if let Some(s) = sub {
                            sqlite3SelectPrep(db, s, NcId::NULL);
                        }
                        i += 1;
                    }
                }

                if db.mallocFailed != 0 {
                    rc = SQLITE_NOMEM;
                }
                db.arena.ncm(sNC).pSrcList = pSrc;
                if rc == SQLITE_OK && pWhere.nn() {
                    rc = sqlite3ResolveExprNames(db, sNC, pWhere);
                }
                if rc == SQLITE_OK {
                    rc = sqlite3ResolveExprListNames(db, sNC, pExprList);
                }
                debug_assert!(pUpsert.is_null() || (pWhere.is_null() && pExprList.is_null()));
                if pUpsert.nn() && rc == SQLITE_OK {
                    db.arena.upsm(pUpsert).pUpsertSrc = pSrc;
                    {
                        let nc = db.arena.ncm(sNC);
                        nc.uNC.pUpsert = pUpsert;
                        nc.ncFlags = NC_UUpsert;
                    }
                    let (tgt, set, wh, twh) = {
                        let u = db.arena.ups(pUpsert);
                        (
                            u.pUpsertTarget,
                            u.pUpsertSet,
                            u.pUpsertWhere,
                            u.pUpsertTargetWhere,
                        )
                    };
                    rc = sqlite3ResolveExprListNames(db, sNC, tgt);
                    if rc == SQLITE_OK {
                        rc = sqlite3ResolveExprListNames(db, sNC, set);
                    }
                    if rc == SQLITE_OK {
                        rc = sqlite3ResolveExprNames(db, sNC, wh);
                    }
                    if rc == SQLITE_OK {
                        rc = sqlite3ResolveExprNames(db, sNC, twh);
                    }
                    db.arena.ncm(sNC).ncFlags = 0;
                }
                db.arena.ncm(sNC).pSrcList = SrcListId::NULL;
                sqlite3SrcListDelete(db, pSrc);
            } else {
                rc = SQLITE_NOMEM;
            }
        }
        pStep = pNext;
    }
    rc
}

/// Invoke sqlite3WalkExpr() or sqlite3WalkSelect() on all Select or Expr
/// objects that are part of the trigger passed as the second argument.
fn renameWalkTrigger(w: &mut Walker<'_>, pTrigger: &TriggerRef) {
    let (whenT, stepsT) = {
        let t = pTrigger.borrow();
        (t.pWhen.clone(), t.step_list.clone())
    };

    // Find tokens to edit in WHEN clause
    let pWhen = w.db.tree(&whenT);
    sqlite3WalkExpr(w, pWhen);

    // Find tokens to edit in trigger steps
    let mut pStep = w.db.tree(&stepsT);
    while pStep.nn() {
        let (pSelect, pWhere, pExprList, pUpsert, pSrc, pNext) = {
            let s = w.db.arena.tstep(pStep);
            (s.pSelect, s.pWhere, s.pExprList, s.pUpsert, s.pSrc, s.pNext)
        };
        sqlite3WalkSelect(w, pSelect);
        sqlite3WalkExpr(w, pWhere);
        sqlite3WalkExprList(w, pExprList);
        if pUpsert.nn() {
            let (tgt, set, wh, twh) = {
                let u = w.db.arena.ups(pUpsert);
                (
                    u.pUpsertTarget,
                    u.pUpsertSet,
                    u.pUpsertWhere,
                    u.pUpsertTargetWhere,
                )
            };
            sqlite3WalkExprList(w, tgt);
            sqlite3WalkExprList(w, set);
            sqlite3WalkExpr(w, wh);
            sqlite3WalkExpr(w, twh);
        }
        if pSrc.nn() {
            let n = w.db.src(pSrc).nSrc();
            for i in 0..n {
                let sub = {
                    let p = w.db.item(SrcItemId(pSrc, i as u32));
                    if p.fg.isSubquery != 0 {
                        p.u4.pSubq.as_ref().map(|q| q.pSelect)
                    } else {
                        None
                    }
                };
                if let Some(s) = sub {
                    sqlite3WalkSelect(w, s);
                }
            }
        }
        pStep = pNext;
    }
}

/// Free the contents of Parse object (*pParse) and pop it from db.parses
/// (it was pushed by renameParseSql()).
fn renameParseCleanup(db: &mut Sqlite3) {
    if let Some(mut v) = db.pp().pVdbe.take() {
        sqlite3VdbeFinalize(db, &mut v);
    }
    if let Some(t) = db.pp().pNewTable.take() {
        sqlite3DeleteTable(db, &t);
    }
    // C: a list linked through pNext; the port holds a single index.
    if let Some(pIdx) = db.pp().pNewIndex.take() {
        sqlite3FreeIndex(db, &pIdx);
    }
    if let Some(t) = db.pp().pNewTrigger.take() {
        crate::trigger::sqlite3DeleteTrigger(db, &t);
    }
    db.pp().zErrMsg = None;
    db.pp().pRename.clear();
    crate::prepare::sqlite3ParseObjectReset(db);
}

/// Walk the expression trees of an ExprList stored in a Tree.
fn walk_elist_tree(w: &mut Walker<'_>, t: &Tree<ExprListId>) {
    let l = w.db.tree(t);
    sqlite3WalkExprList(w, l);
}

/// Restore the authorizer saved by an internal function.
fn restore_auth(db: &mut Sqlite3, xAuth: Option<XAuth>) {
    db.xAuth = xAuth;
}

/// SQL function:
///
///     sqlite_rename_column(SQL,TYPE,OBJ,DB,TABLE,COL,NEWNAME,QUOTE,TEMP)
///
///   0. zSql:     SQL statement to rewrite
///   1. type:     Type of object ("table", "view" etc.)
///   2. object:   Name of object
///   3. Database: Database name (e.g. "main")
///   4. Table:    Table name
///   5. iCol:     Index of column to rename
///   6. zNew:     New column name
///   7. bQuote:   Non-zero if the new column name should be quoted.
///   8. bTemp:    True if zSql comes from temp schema
///
/// Do a column rename operation on the CREATE statement given in zSql.
/// The iCol-th column (left-most is 0) of table zTable is renamed from zCol
/// into zNew.  The name should be quoted if bQuote is true.
///
/// This function is used internally by the ALTER TABLE RENAME COLUMN
/// command.  It is only accessible to SQL created using
/// sqlite3NestedParse().  It is not reachable from ordinary SQL passed into
/// sqlite3_prepare() unless the SQLITE_TESTCTRL_INTERNAL_FUNCTIONS test
/// setting is enabled.
fn renameColumnFunc(context: &mut Context, argv: &mut [Mem]) {
    let zSql = vtext(&mut argv[0]);
    let zDb = vtext(&mut argv[3]);
    let zTable = vtext(&mut argv[4]);
    let iCol = sqlite3_value_int(&mut argv[5]);
    let zNew = vtext(&mut argv[6]);
    let bQuote = sqlite3_value_int(&mut argv[7]);
    let bTemp = sqlite3_value_int(&mut argv[8]);

    let (zSql, zTable, zNew) = match (zSql, zTable, zNew) {
        (Some(a), Some(b), Some(c)) => (a, b, c),
        _ => return,
    };
    if iCol < 0 {
        return;
    }
    let pTab = match sqlite3FindTable(context.db, Some(&zTable), zDb.as_deref()) {
        Some(t) if iCol < t.borrow().nCol as i32 => t,
        _ => return,
    };
    let (zOld, iPKey) = {
        let t = pTab.borrow();
        (t.aCol[iCol as usize].zCnName.clone(), t.iPKey as i32)
    };
    let mut sCtx = RenameCtx::default();
    sCtx.iCol = if iCol == iPKey { -1 } else { iCol };

    let xAuth = context.db.xAuth.take();
    let mut rc = renameParseSql(context.db, zDb.as_deref(), Some(&zSql), bTemp != 0);

    sCtx.pTab = Some(pTab.clone());
    let mut zOut: Option<ZStr> = None;
    if rc == SQLITE_OK {
        rc = rename_column_body(context.db, &mut sCtx, &pTab, &zTable, iCol, &zOld);
        if rc == SQLITE_OK {
            zOut = Some(renameEditSql(
                context.db,
                &mut sCtx,
                &zSql,
                Some(&zNew),
                bQuote,
            ));
        }
    }
    if let Some(z) = zOut {
        sqlite3_result_text(context, Some(&z), z.len() as i32);
    }

    // renameColumnFunc_done:
    if rc != SQLITE_OK {
        if rc == SQLITE_ERROR && sqlite3WritableSchema(context.db) != 0 {
            sqlite3_result_value(context, &mut argv[0]);
        } else if context.db.ppr().zErrMsg.is_some() {
            let zErr = context.db.ppr().zErrMsg.clone();
            let zT = vtext(&mut argv[1]);
            let zN = vtext(&mut argv[2]);
            renameColumnParseError(context, b"", zT, zN, zErr);
        } else {
            sqlite3_result_error_code(context, rc);
        }
    }

    renameParseCleanup(context.db);
    restore_auth(context.db, xAuth);
}

/// The part of renameColumnFunc() that finds the tokens to replace.
fn rename_column_body(
    db: &mut Sqlite3,
    sCtx: &mut RenameCtx,
    pTab: &TableRef,
    zTable: &[u8],
    iCol: i32,
    zOld: &[u8],
) -> i32 {
    let mut rc = SQLITE_OK;
    let pNewTable = db.ppr().pNewTable.clone();
    let pNewIndex = db.ppr().pNewIndex.clone();
    if let Some(nt) = pNewTable {
        if nt.borrow().IsView() {
            let selT = nt.borrow().u.view.pSelect.clone();
            let pSelect = db.tree(&selT);
            db.selm(pSelect).selFlags &= !SF_View;
            db.pp().rc = SQLITE_OK;
            sqlite3SelectPrep(db, pSelect, NcId::NULL);
            rc = if db.mallocFailed != 0 {
                SQLITE_NOMEM
            } else {
                db.ppr().rc
            };
            if rc == SQLITE_OK {
                with_rename_walker(db, sCtx, renameColumnExprCb, renameColumnSelectCb, |w| {
                    sqlite3WalkSelect(w, pSelect);
                });
            }
            if rc != SQLITE_OK {
                return rc;
            }
        } else if nt.borrow().IsOrdinaryTable() {
            // A regular table
            let zNtName = nt.borrow().zName.clone();
            let bFKOnly = sqlite3_stricmp(Some(zTable), Some(&zNtName));
            sCtx.pTab = Some(nt.clone());
            if bFKOnly == 0 {
                let nCol = nt.borrow().nCol as i32;
                if iCol < nCol {
                    renameTokenFind(db, Some(sCtx), rename_key_column(&nt, iCol));
                }
                if sCtx.iCol < 0 {
                    renameTokenFind(db, Some(sCtx), rename_key_ipkey(&nt));
                }
                let checkT = nt.borrow().pCheck.clone();
                let idxExprs: Vec<Tree<ExprListId>> = nt
                    .borrow()
                    .pIndex
                    .iter()
                    .map(|i| i.borrow().aColExpr.clone())
                    .collect();
                let newIdxExprs: Vec<Tree<ExprListId>> = pNewIndex
                    .iter()
                    .map(|i| i.borrow().aColExpr.clone())
                    .collect();
                let mut colExprs = Vec::new();
                for i in 0..nCol {
                    colExprs.push(sqlite3ColumnExpr(db, &nt, i as usize));
                }
                with_rename_walker(db, sCtx, renameColumnExprCb, renameColumnSelectCb, |w| {
                    walk_elist_tree(w, &checkT);
                    for t in idxExprs.iter() {
                        walk_elist_tree(w, t);
                    }
                    for t in newIdxExprs.iter() {
                        walk_elist_tree(w, t);
                    }
                    for e in colExprs.iter() {
                        sqlite3WalkExpr(w, *e);
                    }
                });
            }

            debug_assert!(nt.borrow().IsOrdinaryTable());
            let fkeys: Vec<FKeyRef> = nt.borrow().u.tab.pFKey.clone();
            for pFKey in fkeys.iter() {
                let (nFkCol, zTo) = {
                    let f = pFKey.borrow();
                    (f.nCol, f.zTo.clone())
                };
                for i in 0..nFkCol {
                    let (iFrom, zCol) = {
                        let f = pFKey.borrow();
                        (f.aCol[i as usize].iFrom, f.aCol[i as usize].zCol.clone())
                    };
                    if bFKOnly == 0 && iFrom == iCol {
                        renameTokenFind(db, Some(sCtx), rename_key_fkey_from_col(pFKey, i));
                    }
                    if sqlite3_stricmp(Some(&zTo), Some(zTable)) == 0
                        && sqlite3_stricmp(zCol.as_deref(), Some(zOld)) == 0
                    {
                        renameTokenFind(db, Some(sCtx), rename_key_fkey_to_col(pFKey, i));
                    }
                }
            }
        }
    } else if let Some(pIdx) = pNewIndex {
        let (colT, whereT) = {
            let x = pIdx.borrow();
            (x.aColExpr.clone(), x.pPartIdxWhere.clone())
        };
        with_rename_walker(db, sCtx, renameColumnExprCb, renameColumnSelectCb, |w| {
            walk_elist_tree(w, &colT);
            let e = w.db.tree(&whereT);
            sqlite3WalkExpr(w, e);
        });
    } else {
        // A trigger
        rc = renameResolveTrigger(db);
        if rc != SQLITE_OK {
            return rc;
        }
        let pTrig = db.ppr().pNewTrigger.clone().expect("pNewTrigger");

        let stepsT = pTrig.borrow().step_list.clone();
        let mut pStep = db.tree(&stepsT);
        while pStep.nn() {
            let (pSrc, pUpsert, pIdList, pExprList, pNext) = {
                let s = db.arena.tstep(pStep);
                (s.pSrc, s.pUpsert, s.pIdList, s.pExprList, s.pNext)
            };
            if pSrc.nn() {
                let pTarget = sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0));
                if tab_eq(&pTarget, &Some(pTab.clone())) {
                    if pUpsert.nn() {
                        let pUpsertSet = db.arena.ups(pUpsert).pUpsertSet;
                        renameColumnElistNames(db, sCtx, pUpsertSet, zOld);
                    }
                    renameColumnIdlistNames(db, sCtx, pIdList, zOld);
                    renameColumnElistNames(db, sCtx, pExprList, zOld);
                }
            }
            pStep = pNext;
        }

        // Find tokens to edit in UPDATE OF clause
        let pTriggerTab = db.ppr().pTriggerTab.clone();
        if tab_eq(&pTriggerTab, &Some(pTab.clone())) {
            let colsT = pTrig.borrow().pColumns.clone();
            let pColumns = db.tree(&colsT);
            renameColumnIdlistNames(db, sCtx, pColumns, zOld);
        }

        // Find tokens to edit in various expressions and selects
        with_rename_walker(db, sCtx, renameColumnExprCb, renameColumnSelectCb, |w| {
            renameWalkTrigger(w, &pTrig);
        });
    }
    debug_assert!(rc == SQLITE_OK);
    rc
}

/// Walker expression callback used by "RENAME TABLE".
fn renameTableExprCb(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (op, useYTab, pTab) = {
        let e = w.db.ex(pExpr);
        (e.op, e.UseYTab(), e.y.pTab.clone())
    };
    let p = wctx(&mut w.u);
    if op == TK_COLUMN && useYTab && tab_eq(&p.pTab, &pTab) {
        renameTokenFind(w.db, Some(p), RenameKey::ExprTab(pExpr));
    }
    WRC_Continue
}

/// Walker select callback used by "RENAME TABLE".
fn renameTableSelectCb(w: &mut Walker<'_>, pSelect: SelectId) -> i32 {
    let (selFlags, pSrc) = {
        let s = w.db.sel(pSelect);
        (s.selFlags, s.pSrc)
    };
    if (selFlags & (SF_View | SF_CopyCte)) != 0 {
        return WRC_Prune;
    }
    if pSrc.is_null() {
        debug_assert!(w.db.mallocFailed != 0);
        return WRC_Abort;
    }
    let n = w.db.src(pSrc).nSrc();
    for i in 0..n {
        let it = SrcItemId(pSrc, i as u32);
        let (pSTab, hasName) = {
            let x = w.db.item(it);
            (x.pSTab.clone(), x.zName.is_some())
        };
        let p = wctx(&mut w.u);
        if tab_eq(&pSTab, &p.pTab) && hasName {
            renameTokenFind(w.db, Some(p), rename_key_srcitem_name(it));
        }
    }
    renameWalkWith(w, pSelect);

    WRC_Continue
}

/// This C function implements an SQL user function that is used by SQL
/// code generated by the ALTER TABLE ... RENAME command to modify the
/// definition of any foreign key constraints that use the table being
/// renamed as the parent table. It is passed three arguments:
///
///   0: The database containing the table being renamed.
///   1. type:     Type of object ("table", "view" etc.)
///   2. object:   Name of object
///   3: The complete text of the schema statement being modified,
///   4: The old name of the table being renamed, and
///   5: The new name of the table being renamed.
///   6: True if the schema statement comes from the temp db.
///
/// It returns the new schema statement. For example:
///
/// sqlite_rename_table('main', 'CREATE TABLE t1(a REFERENCES t2)','t2','t3',0)
///       -> 'CREATE TABLE t1(a REFERENCES t3)'
fn renameTableFunc(context: &mut Context, argv: &mut [Mem]) {
    let zDb = vtext(&mut argv[0]);
    let zInput = vtext(&mut argv[3]);
    let zOld = vtext(&mut argv[4]);
    let zNew = vtext(&mut argv[5]);
    let bTemp = sqlite3_value_int(&mut argv[6]);

    if let (Some(zInput), Some(zOld), Some(zNew)) = (zInput, zOld, zNew) {
        let bQuote = 1;
        let xAuth = context.db.xAuth.take();

        let mut sCtx = RenameCtx::default();
        sCtx.pTab = sqlite3FindTable(context.db, Some(&zOld), zDb.as_deref());

        let mut rc = renameParseSql(context.db, zDb.as_deref(), Some(&zInput), bTemp != 0);

        if rc == SQLITE_OK {
            rc = rename_table_body(context.db, &mut sCtx, &zOld);
        }

        if rc == SQLITE_OK {
            let z = renameEditSql(context.db, &mut sCtx, &zInput, Some(&zNew), bQuote);
            sqlite3_result_text(context, Some(&z), z.len() as i32);
        }
        if rc != SQLITE_OK {
            if rc == SQLITE_ERROR && sqlite3WritableSchema(context.db) != 0 {
                sqlite3_result_value(context, &mut argv[3]);
            } else if context.db.ppr().zErrMsg.is_some() {
                let zErr = context.db.ppr().zErrMsg.clone();
                let zT = vtext(&mut argv[1]);
                let zN = vtext(&mut argv[2]);
                renameColumnParseError(context, b"", zT, zN, zErr);
            } else {
                sqlite3_result_error_code(context, rc);
            }
        }

        renameParseCleanup(context.db);
        restore_auth(context.db, xAuth);
    }
}

/// The part of renameTableFunc() that finds the tokens to replace.
fn rename_table_body(db: &mut Sqlite3, sCtx: &mut RenameCtx, zOld: &[u8]) -> i32 {
    let mut rc = SQLITE_OK;
    let isLegacy = (db.flags & SQLITE_LegacyAlter) != 0;
    let pNewTable = db.ppr().pNewTable.clone();
    let pNewIndex = db.ppr().pNewIndex.clone();
    if let Some(pTab) = pNewTable {
        if pTab.borrow().IsView() {
            if !isLegacy {
                let selT = pTab.borrow().u.view.pSelect.clone();
                let pSelect = db.tree(&selT);
                let sNC = db.arena.new_nc(NameContext::default());

                debug_assert!((db.sel(pSelect).selFlags & SF_View) != 0);
                db.selm(pSelect).selFlags &= !SF_View;
                sqlite3SelectPrep(db, pSelect, sNC);
                if db.ppr().nErr != 0 {
                    rc = db.ppr().rc;
                } else {
                    with_rename_walker(db, sCtx, renameTableExprCb, renameTableSelectCb, |w| {
                        sqlite3WalkSelect(w, pSelect);
                    });
                }
            }
        } else {
            // Modify any FK definitions to point to the new table.
            if (!isLegacy || (db.flags & SQLITE_ForeignKeys) != 0) && !pTab.borrow().IsVirtual() {
                debug_assert!(pTab.borrow().IsOrdinaryTable());
                let fkeys: Vec<FKeyRef> = pTab.borrow().u.tab.pFKey.clone();
                for pFKey in fkeys.iter() {
                    let zTo = pFKey.borrow().zTo.clone();
                    if sqlite3_stricmp(Some(&zTo), Some(zOld)) == 0 {
                        renameTokenFind(db, Some(sCtx), rename_key_fkey_to(pFKey));
                    }
                }
            }

            // If this is the table being altered, fix any table refs in
            // CHECK expressions. Also update the name that appears right
            // after the "CREATE [VIRTUAL] TABLE" bit.
            let zTabName = pTab.borrow().zName.clone();
            if sqlite3_stricmp(Some(zOld), Some(&zTabName)) == 0 {
                sCtx.pTab = Some(pTab.clone());
                if !isLegacy {
                    let checkT = pTab.borrow().pCheck.clone();
                    with_rename_walker(db, sCtx, renameTableExprCb, renameTableSelectCb, |w| {
                        walk_elist_tree(w, &checkT);
                    });
                }
                renameTokenFind(db, Some(sCtx), rename_key_table_name(&pTab));
            }
        }
    } else if let Some(pIdx) = pNewIndex {
        renameTokenFind(db, Some(sCtx), rename_key_index_name(&pIdx));
        if !isLegacy {
            let whereT = pIdx.borrow().pPartIdxWhere.clone();
            with_rename_walker(db, sCtx, renameTableExprCb, renameTableSelectCb, |w| {
                let e = w.db.tree(&whereT);
                sqlite3WalkExpr(w, e);
            });
        }
    } else {
        let pTrigger = db.ppr().pNewTrigger.clone().expect("pNewTrigger");
        let (table, pTabSchema) = {
            let t = pTrigger.borrow();
            (t.table.clone(), t.pTabSchema)
        };
        let ctxSchema = sCtx.pTab.as_ref().map(|t| t.borrow().pSchema);
        if sqlite3_stricmp(Some(&table), Some(zOld)) == 0 && ctxSchema == Some(pTabSchema) {
            renameTokenFind(db, Some(sCtx), rename_key_trigger_table(&pTrigger));
        }

        if !isLegacy {
            rc = renameResolveTrigger(db);
            if rc == SQLITE_OK {
                with_rename_walker(db, sCtx, renameTableExprCb, renameTableSelectCb, |w| {
                    renameWalkTrigger(w, &pTrigger);
                });
                let stepsT = pTrigger.borrow().step_list.clone();
                let mut pStep = db.tree(&stepsT);
                while pStep.nn() {
                    let (pSrc, pNext) = {
                        let s = db.arena.tstep(pStep);
                        (s.pSrc, s.pNext)
                    };
                    if pSrc.nn() {
                        let n = db.src(pSrc).nSrc();
                        for i in 0..n {
                            let it = SrcItemId(pSrc, i as u32);
                            let zName = db.item(it).zName.clone();
                            if zName.is_some() && sqlite3_stricmp(zName.as_deref(), Some(zOld)) == 0
                            {
                                renameTokenFind(db, Some(sCtx), rename_key_srcitem_name(it));
                            }
                        }
                    }
                    pStep = pNext;
                }
            }
        }
    }
    rc
}

fn renameQuotefixExprCb(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (op, flags) = {
        let e = w.db.ex(pExpr);
        (e.op, e.flags)
    };
    if op == TK_STRING && (flags & EP_DblQuoted) != 0 {
        let p = wctx(&mut w.u);
        renameTokenFind(w.db, Some(p), RenameKey::Expr(pExpr));
    }
    WRC_Continue
}

/// SQL function: sqlite_rename_quotefix(DB,SQL)
///
/// Rewrite the DDL statement "SQL" so that any string literals that use
/// double-quotes use single quotes instead.
///
/// Two arguments must be passed:
///
///   0: Database name ("main", "temp" etc.).
///   1: SQL statement to edit.
///
/// The returned value is the modified SQL statement. For example, given
/// the database schema:
///
///   CREATE TABLE t1(a, b, c);
///
///   SELECT sqlite_rename_quotefix('main',
///       'CREATE VIEW v1 AS SELECT "a", "string" FROM t1'
///   );
///
/// returns the string:
///
///   CREATE VIEW v1 AS SELECT "a", 'string' FROM t1
///
/// If there is a error in the input SQL, then raise an error, except
/// if PRAGMA writable_schema=ON, then just return the input string
/// unmodified following an error.
fn renameQuotefixFunc(context: &mut Context, argv: &mut [Mem]) {
    let zDb = vtext(&mut argv[0]);
    let zInput = vtext(&mut argv[1]);

    let xAuth = context.db.xAuth.take();

    if let (Some(zDb), Some(zInput)) = (zDb, zInput) {
        let mut rc = renameParseSql(context.db, Some(&zDb), Some(&zInput), false);

        if rc == SQLITE_OK {
            // Walker to find tokens that need to be replaced.
            let mut sCtx = RenameCtx::default();
            let db = &mut *context.db;
            let pNewTable = db.ppr().pNewTable.clone();
            let pNewIndex = db.ppr().pNewIndex.clone();

            if let Some(nt) = pNewTable {
                if nt.borrow().IsView() {
                    let selT = nt.borrow().u.view.pSelect.clone();
                    let pSelect = db.tree(&selT);
                    db.selm(pSelect).selFlags &= !SF_View;
                    db.pp().rc = SQLITE_OK;
                    sqlite3SelectPrep(db, pSelect, NcId::NULL);
                    rc = if db.mallocFailed != 0 {
                        SQLITE_NOMEM
                    } else {
                        db.ppr().rc
                    };
                    if rc == SQLITE_OK {
                        with_rename_walker(
                            db,
                            &mut sCtx,
                            renameQuotefixExprCb,
                            renameColumnSelectCb,
                            |w| {
                                sqlite3WalkSelect(w, pSelect);
                            },
                        );
                    }
                } else {
                    let checkT = nt.borrow().pCheck.clone();
                    let nCol = nt.borrow().nCol as usize;
                    let mut colExprs = Vec::new();
                    for i in 0..nCol {
                        colExprs.push(sqlite3ColumnExpr(db, &nt, i));
                    }
                    with_rename_walker(
                        db,
                        &mut sCtx,
                        renameQuotefixExprCb,
                        renameColumnSelectCb,
                        |w| {
                            walk_elist_tree(w, &checkT);
                            for e in colExprs.iter() {
                                sqlite3WalkExpr(w, *e);
                            }
                        },
                    );
                }
            } else if let Some(pIdx) = pNewIndex {
                let (colT, whereT) = {
                    let x = pIdx.borrow();
                    (x.aColExpr.clone(), x.pPartIdxWhere.clone())
                };
                with_rename_walker(
                    db,
                    &mut sCtx,
                    renameQuotefixExprCb,
                    renameColumnSelectCb,
                    |w| {
                        walk_elist_tree(w, &colT);
                        let e = w.db.tree(&whereT);
                        sqlite3WalkExpr(w, e);
                    },
                );
            } else {
                rc = renameResolveTrigger(db);
                if rc == SQLITE_OK {
                    let pTrig = db.ppr().pNewTrigger.clone().expect("pNewTrigger");
                    with_rename_walker(
                        db,
                        &mut sCtx,
                        renameQuotefixExprCb,
                        renameColumnSelectCb,
                        |w| {
                            renameWalkTrigger(w, &pTrig);
                        },
                    );
                }
            }

            if rc == SQLITE_OK {
                let z = renameEditSql(context.db, &mut sCtx, &zInput, None, 0);
                sqlite3_result_text(context, Some(&z), z.len() as i32);
            }
        }
        if rc != SQLITE_OK {
            if sqlite3WritableSchema(context.db) != 0 && rc == SQLITE_ERROR {
                sqlite3_result_value(context, &mut argv[1]);
            } else {
                sqlite3_result_error_code(context, rc);
            }
        }
        renameParseCleanup(context.db);
    }

    restore_auth(context.db, xAuth);
}

/// Function:  sqlite_rename_test(DB,SQL,TYPE,NAME,ISTEMP,WHEN,DQS)
///
/// An SQL user function that checks that there are no parse or symbol
/// resolution problems in a CREATE TRIGGER|TABLE|VIEW|INDEX statement.
/// After an ALTER TABLE .. RENAME operation is performed and the schema
/// reloaded, this function is called on each SQL statement in the schema
/// to ensure that it is still usable.
///
///   0: Database name ("main", "temp" etc.).
///   1: SQL statement.
///   2: Object type ("view", "table", "trigger" or "index").
///   3: Object name.
///   4: True if object is from temp schema.
///   5: "when" part of error message.
///   6: True to disable the DQS quirk when parsing SQL.
///
/// The return value is computed as follows:
///
///   A. If an error is seen and not in PRAGMA writable_schema=ON mode,
///      then raise the error.
///   B. Else if a trigger is created and the the table that the trigger is
///      attached to is in database zDb, then return 1.
///   C. Otherwise return NULL.
fn renameTableTest(context: &mut Context, argv: &mut [Mem]) {
    let zDb = vtext(&mut argv[0]);
    let zInput = vtext(&mut argv[1]);
    let bTemp = sqlite3_value_int(&mut argv[4]);
    let isLegacy = (context.db.flags & SQLITE_LegacyAlter) != 0;
    let zWhen = vtext(&mut argv[5]);
    let bNoDQS = sqlite3_value_int(&mut argv[6]);

    let xAuth = context.db.xAuth.take();

    if let (Some(zDb), Some(zInput)) = (zDb, zInput) {
        let flags = context.db.flags;
        if bNoDQS != 0 {
            context.db.flags &= !(SQLITE_DqsDML | SQLITE_DqsDDL);
        }
        let mut rc = renameParseSql(context.db, Some(&zDb), Some(&zInput), bTemp != 0);
        context.db.flags = flags;
        if rc == SQLITE_OK {
            let pNewTable = context.db.ppr().pNewTable.clone();
            let pNewTrigger = context.db.ppr().pNewTrigger.clone();
            let isView = pNewTable
                .as_ref()
                .map(|t| t.borrow().IsView())
                .unwrap_or(false);
            if !isLegacy && isView {
                let db = &mut *context.db;
                let nt = pNewTable.unwrap();
                let selT = nt.borrow().u.view.pSelect.clone();
                let pSelect = db.tree(&selT);
                let sNC = db.arena.new_nc(NameContext::default());
                sqlite3SelectPrep(db, pSelect, sNC);
                if db.ppr().nErr != 0 {
                    rc = db.ppr().rc;
                }
            } else if let Some(trig) = pNewTrigger {
                if !isLegacy {
                    rc = renameResolveTrigger(context.db);
                }
                if rc == SQLITE_OK {
                    let pTabSchema = trig.borrow().pTabSchema;
                    let i1 = context.db.schema_index(pTabSchema);
                    let i2 = sqlite3FindDbName(context.db, Some(&zDb));
                    if i1 == i2 {
                        // Handle output case B
                        sqlite3_result_int(context, 1);
                    }
                }
            }
        }

        if rc != SQLITE_OK && zWhen.is_some() && sqlite3WritableSchema(context.db) == 0 {
            // Output case A
            let zErr = context.db.ppr().zErrMsg.clone();
            let zT = vtext(&mut argv[2]);
            let zN = vtext(&mut argv[3]);
            renameColumnParseError(context, zWhen.as_deref().unwrap_or(b""), zT, zN, zErr);
        }
        renameParseCleanup(context.db);
    }

    restore_auth(context.db, xAuth);
}

/// Return the number of bytes until the end of the next non-whitespace and
/// non-comment token.  For the purpose of this function, a "(" token
/// includes all of the bytes through and including the matching ")", or
/// until the first illegal token, whichever comes first.
///
/// Write the token type into *piToken.
///
/// The value returned is the number of bytes in the token itself plus
/// the number of bytes of leading whitespace and comments skipped plus
/// all bytes through the next matching ")" if the token is TK_LP.
fn getConstraintToken(z: &[u8], piToken: &mut i32) -> usize {
    let mut iOff = 0usize;
    let mut t = 0i32;
    loop {
        iOff += get_token(z, iOff, &mut t);
        if !(t == TK_SPACE as i32 || t == TK_COMMENT as i32) {
            break;
        }
    }

    *piToken = t;

    if t == TK_LP as i32 {
        let mut nNest = 1;
        while nNest > 0 {
            iOff += get_token(z, iOff, &mut t);
            if t == TK_LP as i32 {
                nNest += 1;
            } else if t == TK_RP as i32 {
                t = TK_LP as i32;
                nNest -= 1;
            } else if t == TK_ILLEGAL as i32 {
                break;
            }
        }
    }

    *piToken = t;
    iOff
}

/// sqlite3GetToken()-style helper on a sub-slice.
#[inline]
fn sub(z: &[u8], off: usize) -> &[u8] {
    if off < z.len() {
        &z[off..]
    } else {
        &z[z.len()..]
    }
}

/// The implementation of internal UDF sqlite_drop_column().
///
/// Arguments:
///
///  argv[0]: An integer - the index of the schema containing the table
///  argv[1]: CREATE TABLE statement to modify.
///  argv[2]: An integer - the index of the column to remove.
///
/// The value returned is a string containing the CREATE TABLE statement
/// with column argv[2] removed.
fn dropColumnFunc(context: &mut Context, argv: &mut [Mem]) {
    let iSchema = sqlite3_value_int(&mut argv[0]);
    let zSql = vtext(&mut argv[1]);
    let iCol = sqlite3_value_int(&mut argv[2]);
    let zDb = context.db.aDb[iSchema as usize].zDbSName.clone();

    let xAuth = context.db.xAuth.take();

    let mut rc = renameParseSql(context.db, zDb.as_deref(), zSql.as_deref(), iSchema == 1);
    if rc == SQLITE_OK {
        let zSql = cstr(zSql.as_deref().unwrap_or(b"")).to_vec();
        let db = &mut *context.db;
        let pTab = db.ppr().pNewTable.clone();
        let bad = match pTab.as_ref() {
            None => true,
            Some(t) => {
                let n = t.borrow().nCol as i32;
                n == 1 || iCol >= n
            }
        };
        if bad {
            // This can happen if the sqlite_schema table is corrupt
            rc = crate::mainapi::sqlite3CorruptError(line!() as i32);
        } else {
            let pTab = pTab.unwrap();
            let nCol = pTab.borrow().nCol as i32;
            let start: Option<usize>;
            let zEnd: usize;
            if iCol < nCol - 1 {
                let pCol = renameTokenFind(db, None, rename_key_column(&pTab, iCol));
                let pEnd = renameTokenFind(db, None, rename_key_column(&pTab, iCol + 1));
                start = pCol.map(|t| t.t.off as usize);
                zEnd = pEnd.map(|t| t.t.off as usize).unwrap_or(zSql.len());
            } else {
                debug_assert!(pTab.borrow().IsOrdinaryTable());
                debug_assert!(iCol != 0);
                // Point pCol->t.z at the "," immediately preceding the
                // definition of the column being dropped. To do this, start
                // at the name of the previous column, and tokenize until the
                // next ",".
                let pCol = renameTokenFind(db, None, rename_key_column(&pTab, iCol - 1));
                start = pCol.map(|t| {
                    let mut z = t.t.off as usize;
                    let mut eTok = 0i32;
                    loop {
                        z += getConstraintToken(sub(&zSql, z), &mut eTok);
                        if eTok == TK_COMMA as i32 || z >= zSql.len() {
                            break;
                        }
                    }
                    z - 1
                });
                zEnd = pTab.borrow().u.tab.addColOffset as usize;
            }
            match start {
                Some(s) => {
                    let s = s.min(zSql.len());
                    let zNew = mprintf!(db, "%.*s%s", s as i32, &zSql, sub(&zSql, zEnd));
                    sqlite3_result_text(context, Some(&zNew), zNew.len() as i32);
                }
                None => {
                    rc = crate::mainapi::sqlite3CorruptError(line!() as i32);
                }
            }
        }
    }

    // drop_column_done:
    renameParseCleanup(context.db);
    restore_auth(context.db, xAuth);
    if rc != SQLITE_OK {
        sqlite3_result_error_code(context, rc);
    }
}

/// C: `void sqlite3AlterDropColumn(Parse *pParse, SrcList *pSrc, const Token *pName)`
///
/// This function is called by the parser upon parsing an
///
///     ALTER TABLE pSrc DROP COLUMN pName
///
/// statement. Argument pSrc contains the possibly qualified name of the
/// table being edited, and token pName the name of the column to drop.
pub fn sqlite3AlterDropColumn(db: &mut Sqlite3, pSrc: SrcListId, pName: &Token) {
    alter_drop_column(db, pSrc, pName);
    // exit_drop_column:
    sqlite3SrcListDelete(db, pSrc);
}

fn alter_drop_column(db: &mut Sqlite3, pSrc: SrcListId, pName: &Token) {
    // Look up the table being altered.
    debug_assert!(db.ppr().pNewTable.is_none());
    if db.mallocFailed != 0 {
        return;
    }
    let pTab = match sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0)) {
        Some(t) => t,
        None => return,
    };

    // Make sure this is not an attempt to ALTER a view, virtual table or
    // system table.
    if SQLITE_OK != isAlterableTable(db, &pTab) {
        return;
    }
    if SQLITE_OK != isRealTable(db, &pTab, 1) {
        return;
    }

    // Find the index of the column being dropped.
    let zCol = match sqlite3NameFromToken(db, pName) {
        Some(z) => z,
        None => {
            debug_assert!(db.mallocFailed != 0);
            return;
        }
    };
    let iCol = sqlite3ColumnIndex(&pTab, Some(&zCol));
    if iCol < 0 {
        errmsg!(db, "no such column: \"%T\"", pName);
        return;
    }
    if isRowidAlias(db, &pTab, &zCol, "drop") != 0 {
        return;
    }

    // Do not allow the user to drop a PRIMARY KEY column or a column
    // constrained by a UNIQUE constraint.
    let colFlags = pTab.borrow().aCol[iCol as usize].colFlags;
    if (colFlags & (COLFLAG_PRIMKEY | COLFLAG_UNIQUE)) != 0 {
        errmsg!(
            db,
            "cannot drop %s column: \"%s\"",
            if (colFlags & COLFLAG_PRIMKEY) != 0 {
                "PRIMARY KEY"
            } else {
                "UNIQUE"
            },
            &zCol
        );
        return;
    }

    // Do not allow the number of columns to go to zero
    if pTab.borrow().nCol <= 1 {
        errmsg!(
            db,
            "cannot drop column \"%s\": no other columns exist",
            &zCol
        );
        return;
    }

    // Edit the sqlite_schema table
    let iDb = db.schema_index(pTab.borrow().pSchema);
    debug_assert!(iDb >= 0);
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let zDb = zDb.as_deref();
    let zTabName = pTab.borrow().zName.clone();
    // Invoke the authorization callback.
    if crate::auth::sqlite3AuthCheck(db, SQLITE_ALTER_TABLE, zDb, Some(&zTabName), Some(&zCol)) != 0
    {
        return;
    }
    renameTestSchema(db, zDb, (iDb == 1) as i32, "", 0);
    renameFixQuotes(db, zDb, (iDb == 1) as i32);
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_drop_column(%d, sql, %d) WHERE (type=='table' AND tbl_name=%Q COLLATE nocase)",
            LEGACY_SCHEMA_TABLE
        ),
        zDb,
        iDb,
        iCol,
        &zTabName
    );

    // Drop and reload the database schema.
    renameReloadSchema(db, iDb, INITFLAG_AlterDrop as u16);
    renameTestSchema(db, zDb, (iDb == 1) as i32, "after drop column", 1);

    // Edit rows of table on disk
    if db.ppr().nErr == 0 && (pTab.borrow().aCol[iCol as usize].colFlags & COLFLAG_VIRTUAL) == 0 {
        let mut pPk: Option<IndexRef> = None;
        let mut nField: i32 = 0; // Number of non-virtual columns after drop
        sqlite3GetVdbe(db);
        let iCur = db.ppr().nTab;
        db.pp().nTab += 1;
        crate::insert::sqlite3OpenTable(db, iCur, iDb, &pTab, OP_OpenWrite as i32);
        let addr = sqlite3VdbeAddOp1(db.v(), OP_Rewind, iCur);
        db.pp().nMem += 1;
        let reg = db.ppr().nMem;
        let (hasRowid, nCol, iPKey) = {
            let t = pTab.borrow();
            (t.HasRowid(), t.nCol as i32, t.iPKey as i32)
        };
        if hasRowid {
            sqlite3VdbeAddOp2(db.v(), OP_Rowid, iCur, reg);
            db.pp().nMem += nCol;
        } else {
            let pk = crate::build::sqlite3PrimaryKeyIndex(&pTab).expect("pk");
            let (nColumn, nKeyCol) = {
                let x = pk.borrow();
                (x.nColumn as i32, x.nKeyCol as i32)
            };
            db.pp().nMem += nColumn;
            for i in 0..nKeyCol {
                sqlite3VdbeAddOp3(db.v(), OP_Column, iCur, i, reg + i + 1);
            }
            nField = nKeyCol;
            pPk = Some(pk);
        }
        db.pp().nMem += 1;
        let regRec = db.ppr().nMem;
        for i in 0..nCol {
            let cf = pTab.borrow().aCol[i as usize].colFlags;
            if i != iCol && (cf & COLFLAG_VIRTUAL) == 0 {
                let regOut;
                if let Some(pk) = pPk.as_ref() {
                    let iPos = sqlite3TableColumnToIndex(pk, i);
                    let iColPos = sqlite3TableColumnToIndex(pk, iCol);
                    if iPos < pk.borrow().nKeyCol as i32 {
                        continue;
                    }
                    regOut = reg + 1 + iPos - (iPos > iColPos) as i32;
                } else {
                    regOut = reg + 1 + nField;
                }
                if i == iPKey {
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regOut);
                } else {
                    let aff = pTab.borrow().aCol[i as usize].affinity;
                    if aff == SQLITE_AFF_REAL {
                        pTab.borrow_mut().aCol[i as usize].affinity = SQLITE_AFF_NUMERIC;
                    }
                    sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, i, regOut);
                    pTab.borrow_mut().aCol[i as usize].affinity = aff;
                }
                nField += 1;
            }
        }
        if nField == 0 {
            // dbsqlfuzz 5f09e7bcc78b4954d06bf9f2400d7715f48d1fef
            db.pp().nMem += 1;
            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, reg + 1);
            nField = 1;
        }
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, reg + 1, nField, regRec);
        if let Some(pk) = pPk.as_ref() {
            let nKeyCol = pk.borrow().nKeyCol as i32;
            sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, iCur, regRec, reg + 1, nKeyCol);
        } else {
            sqlite3VdbeAddOp3(db.v(), OP_Insert, iCur, regRec, reg);
        }
        sqlite3VdbeChangeP5(db.v(), OPFLAG_SAVEPOSITION);

        sqlite3VdbeAddOp2(db.v(), OP_Next, iCur, addr + 1);
        sqlite3VdbeJumpHere(db.v(), addr);
    }
}

/// Return the number of bytes of leading whitespace/comments in string z[].
fn getWhitespace(z: &[u8]) -> usize {
    let mut nRet = 0usize;
    loop {
        let mut t = 0i32;
        let n = get_token(z, nRet, &mut t);
        if t != TK_SPACE as i32 && t != TK_COMMENT as i32 {
            break;
        }
        nRet += n;
    }
    nRet
}

/// Argument z points into the body of a constraint - specifically the
/// second token of the constraint definition.  For a named constraint,
/// z points to the second token of the constraint definition. For an
/// unnamed NOT NULL constraint, z points to the first byte past the NOT
/// keyword.
///
/// Argument eTok may be the token value of the first token of the
/// constraint (e.g. TK_CHECK or TK_REFERENCES) or zero. If it is either
/// TK_REFERENCES or TK_FOREIGN, special parsing is enabled to find the end
/// of the foreign-key constraint definition.
///
/// Return the number of bytes until the end of the constraint.
fn getConstraint(z: &[u8], mut eTok: i32) -> usize {
    let mut iOff = 0usize;
    let mut t = 0i32;

    if eTok == TK_FOREIGN as i32 {
        // For a FOREIGN KEY constraint, use getConstraint() to parse
        // everything up to the REFERENCES keyword. Then
        // getConstraintToken() to consume the TK_REFERENCES token itself.
        // Then fall through to the special handling for TK_REFERENCES
        // below.
        iOff = getConstraint(z, 0);
        iOff += getConstraintToken(sub(z, iOff), &mut eTok);
    }

    if eTok == TK_REFERENCES as i32 {
        // REFERENCES is followed by a table name. Gobble this up here in
        // case the table name is a fallback token like TK_GENERATED.
        iOff += getConstraintToken(sub(z, iOff), &mut t);
    }

    // Now, the current constraint proceeds until the next occurence of one
    // of the following tokens:
    //
    //   CONSTRAINT, PRIMARY, NOT, UNIQUE, CHECK, DEFAULT,
    //   COLLATE, REFERENCES, FOREIGN, GENERATED, AS, RP, or COMMA
    //
    // Also exit the loop if ILLEGAL turns up.
    loop {
        let n = getConstraintToken(sub(z, iOff), &mut t);
        let stop = [
            TK_CONSTRAINT,
            TK_PRIMARY,
            TK_NOT,
            TK_UNIQUE,
            TK_CHECK,
            TK_DEFAULT,
            TK_COLLATE,
            TK_REFERENCES,
            TK_FOREIGN,
            TK_RP,
            TK_COMMA,
            TK_ILLEGAL,
            TK_AS,
            TK_GENERATED,
        ];
        if stop.iter().any(|&k| k as i32 == t) {
            break;
        }
        iOff += n;
    }

    iOff
}

/// Compare two constraint names.
///
/// Summary:   *pRes := zQuote != zCmp
///
/// Details:
/// Compare the (possibly quoted) constraint name zQuote[0..nQuote-1]
/// against zCmp[].  Write zero into *pRes if they are the same and
/// non-zero if they differ.  Normally return SQLITE_OK.
fn quotedCompare(
    _ctx: &mut Context,
    t: i32,
    zQuote: &[u8],
    nQuote: usize,
    zCmp: &[u8],
    pRes: &mut i32,
) -> i32 {
    if t == TK_ILLEGAL as i32 {
        *pRes = 1;
        return SQLITE_OK;
    }
    let mut zCopy: Vec<u8> = zQuote[..nQuote.min(zQuote.len())].to_vec();
    dequote(&mut zCopy);
    *pRes = sqlite3_stricmp(Some(cstr(&zCopy)), Some(zCmp));
    SQLITE_OK
}

/// zSql[] is a CREATE TABLE statement, supposedly.  Find the offset
/// into zSql[] of the first character past the first "(" and write
/// that offset into *piOff and return SQLITE_OK.  Or, if not found,
/// set the SQLITE_CORRUPT error code and return SQLITE_ERROR.
fn skipCreateTable(ctx: &mut Context, zSql: Option<&[u8]>, piOff: &mut usize) -> i32 {
    let mut iOff = 0usize;

    let zSql = match zSql {
        None => return SQLITE_ERROR,
        Some(z) => z,
    };

    // Jump past the "CREATE TABLE" bit.
    loop {
        let mut t = 0i32;
        iOff += get_token(zSql, iOff, &mut t);
        if t == TK_LP as i32 {
            break;
        }
        if t == TK_ILLEGAL as i32 {
            let rc = crate::mainapi::sqlite3CorruptError(line!() as i32);
            sqlite3_result_error_code(ctx, rc);
            return SQLITE_ERROR;
        }
    }

    *piOff = iOff;
    SQLITE_OK
}

/// Internal SQL function sqlite3_drop_constraint():  Given an input
/// CREATE TABLE statement, return a revised CREATE TABLE statement
/// with a constraint removed.  Two forms, depending on the datatype
/// of argv[2]:
///
///   sqlite_drop_constraint(SQL, INT)  -- Omit NOT NULL from the INT-th column
///   sqlite_drop_constraint(SQL, TEXT) -- OMIT constraint with name TEXT
///
/// In the first case, the left-most column is 0.
fn dropConstraintFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let zSql = match vtext(&mut argv[0]) {
        None => return,
        Some(z) => cstr(&z).to_vec(),
    };
    let zSql = &zSql[..];
    let mut zCons: Option<ZStr> = None;
    let mut iNotNull = -1;
    let mut iOff = 0usize;
    let mut iStart = 0usize;
    let mut iEnd: i64 = 0;
    let mut t = 0i32;

    // Jump past the "CREATE TABLE" bit.
    if skipCreateTable(ctx, Some(zSql), &mut iOff) != 0 {
        return;
    }

    if sqlite3_value_type(&mut argv[1]) == SQLITE_INTEGER {
        iNotNull = sqlite3_value_int(&mut argv[1]);
    } else {
        zCons = vtext(&mut argv[1]);
    }

    // Search for the named constraint within column definitions.
    let mut ii = 0;
    while iEnd == 0 {
        // Now parse the column or table constraint definition. Search
        // for the token CONSTRAINT if this is a DROP CONSTRAINT command,
        // or NOT in the right column if this is a DROP NOT NULL.
        loop {
            iStart = iOff;
            iOff += getConstraintToken(sub(zSql, iOff), &mut t);
            if t == TK_CONSTRAINT as i32 && (zCons.is_some() || iNotNull == ii) {
                // Check if this is the constraint we are searching for.
                let mut cmp = 1;

                // Skip past any whitespace.
                iOff += getWhitespace(sub(zSql, iOff));

                // Compare the next token - which may be quoted - with the
                // name of the constraint being dropped.
                let nTok = getConstraintToken(sub(zSql, iOff), &mut t);
                if let Some(zc) = zCons.as_deref() {
                    if quotedCompare(ctx, t, sub(zSql, iOff), nTok, zc, &mut cmp) != 0 {
                        return;
                    }
                }
                iOff += nTok;

                // The next token is usually the first token of the
                // constraint definition. This is enough to tell the type of
                // the constraint - TK_NOT means it is a NOT NULL, TK_CHECK a
                // CHECK constraint etc.
                //
                // There is also the chance that the next token is
                // TK_CONSTRAINT (or TK_DEFAULT or TK_COLLATE), for example if
                // a table has been created as follows:
                //
                //    CREATE TABLE t1(cols, CONSTRAINT one CONSTRAINT two NOT NULL);
                //
                // In this case, allow the "CONSTRAINT one" bit to be dropped
                // by this command if that is what is requested, or to advance
                // to the next iteration of the loop with &zSql[iOff] still
                // pointing to the CONSTRAINT keyword.
                let nTok = getConstraintToken(sub(zSql, iOff), &mut t);
                if t == TK_CONSTRAINT as i32
                    || t == TK_DEFAULT as i32
                    || t == TK_COLLATE as i32
                    || t == TK_COMMA as i32
                    || t == TK_RP as i32
                    || t == TK_GENERATED as i32
                    || t == TK_AS as i32
                {
                    t = TK_CHECK as i32;
                } else {
                    iOff += nTok;
                    iOff += getConstraint(sub(zSql, iOff), t);
                }

                if cmp == 0 || (iNotNull >= 0 && t == TK_NOT as i32) {
                    if t != TK_NOT as i32
                        && t != TK_CHECK as i32
                        && t != TK_REFERENCES as i32
                        && t != TK_FOREIGN as i32
                    {
                        errorMPrintf(
                            ctx,
                            "constraint may not be dropped: %s",
                            &[PfArg::from(zCons.as_deref())],
                        );
                        return;
                    }
                    iEnd = iOff as i64;
                    break;
                }
            } else if t == TK_NOT as i32 && iNotNull == ii {
                iEnd = (iOff + getConstraint(sub(zSql, iOff), 0)) as i64;
                break;
            } else if t == TK_RP as i32 || t == TK_ILLEGAL as i32 {
                iEnd = -1;
                break;
            } else if t == TK_COMMA as i32 {
                break;
            }
        }
        ii += 1;
    }

    // If the constraint has not been found it is an error.
    if iEnd <= 0 {
        if let Some(zc) = zCons.as_deref() {
            errorMPrintf(ctx, "no such constraint: %s", &[PfArg::from(zc)]);
        } else {
            // SQLite follows postgres in that a DROP NOT NULL on a column
            // that is not NOT NULL is not an error. So just return the
            // original SQL here.
            sqlite3_result_text(ctx, Some(zSql), zSql.len() as i32);
        }
    } else {
        // Figure out if an extra space should be inserted after the
        // constraint is removed. And if an additional comma preceding the
        // constraint should be removed.
        let mut iEnd = iEnd as usize;
        let mut zSpace = " ";
        iEnd += getWhitespace(sub(zSql, iEnd));
        get_token(zSql, iEnd, &mut t);
        if t == TK_RP as i32 || t == TK_COMMA as i32 {
            zSpace = "";
            if iStart > 0 && zSql[iStart - 1] == b',' {
                iStart -= 1;
            }
        }

        let zNew = mprintf!(
            ctx.db,
            "%.*s%s%s",
            iStart as i32,
            zSql,
            zSpace,
            sub(zSql, iEnd)
        );
        sqlite3_result_text(ctx, Some(&zNew), zNew.len() as i32);
    }
}

/// Internal SQL function:
///
///     sqlite_add_constraint(SQL, CONSTRAINT-TEXT, ICOL)
///
/// SQL is a CREATE TABLE statement.  Return a modified version of
/// SQL that adds CONSTRAINT-TEXT at the end of the ICOL-th column
/// definition.  (The left-most column defintion is 0.)
fn addConstraintFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let zSql = vtext(&mut argv[0]).map(|z| cstr(&z).to_vec());
    let zCons = vtext(&mut argv[1]);
    let iCol = sqlite3_value_int(&mut argv[2]);
    let mut iOff = 0usize;
    let mut t = 0i32;

    if skipCreateTable(ctx, zSql.as_deref(), &mut iOff) != 0 {
        return;
    }
    let zSql = zSql.unwrap();
    let zSql = &zSql[..];

    let mut ii = 0;
    while ii <= iCol || (iCol < 0 && t != TK_RP as i32) {
        iOff += getConstraintToken(sub(zSql, iOff), &mut t);
        loop {
            let nTok = getConstraintToken(sub(zSql, iOff), &mut t);
            if t == TK_COMMA as i32 || t == TK_RP as i32 {
                break;
            }
            if t == TK_ILLEGAL as i32 {
                let rc = crate::mainapi::sqlite3CorruptError(line!() as i32);
                sqlite3_result_error_code(ctx, rc);
                return;
            }
            iOff += nTok;
        }
        ii += 1;
    }

    iOff += getWhitespace(sub(zSql, iOff));

    let mut pNew = crate::printf::sqlite3_str_new(Some(&*ctx.db));
    crate::printf::sqlite3_str_append(&mut pNew, &zSql[..iOff.min(zSql.len())], iOff as i32);
    if iCol < 0 {
        crate::printf::sqlite3_str_append(&mut pNew, b",", 1);
    }
    crate::printf::sqlite3_str_appendf(
        &mut pNew,
        b" %s%s",
        &[PfArg::from(zCons.as_deref()), PfArg::from(sub(zSql, iOff))],
    );
    sqlite3_result_str(ctx, &mut pNew, SQLITE_FINISH);
}

/// Find a column named pCol in table pTab. If successful, set output
/// parameter *piCol to the index of the column in the table and return
/// SQLITE_OK. Otherwise, set *piCol to -1 and return an SQLite error
/// code.
fn alterFindCol(db: &mut Sqlite3, pTab: &TableRef, pCol: &Token, piCol: &mut i32) -> i32 {
    let zName = sqlite3NameFromToken(db, pCol);
    let mut rc = SQLITE_NOMEM;
    let mut iCol = -1;

    if let Some(zName) = zName.as_deref() {
        iCol = sqlite3ColumnIndex(pTab, Some(zName));
        if iCol < 0 {
            errmsg!(db, "no such column: %s", zName);
            rc = SQLITE_ERROR;
        } else {
            rc = SQLITE_OK;
        }
    }

    if rc == SQLITE_OK {
        let (pSchema, zTabName, zCol) = {
            let t = pTab.borrow();
            (
                t.pSchema,
                t.zName.clone(),
                t.aCol[iCol as usize].zCnName.clone(),
            )
        };
        let iDb = db.schema_index(pSchema);
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        // C: the result of the authorization check only clears a local
        // variable; the error (if any) is left in the Parse object.
        let _ = crate::auth::sqlite3AuthCheck(
            db,
            SQLITE_ALTER_TABLE,
            zDb.as_deref(),
            Some(&zTabName),
            Some(&zCol),
        );
    }

    *piCol = iCol;
    rc
}

/// Find the table named by the first entry in source list pSrc. If
/// successful, return a pointer to the Table structure and set output
/// variable (*pzDb) to point to the name of the database containin the
/// table (i.e. "main", "temp" or the name of an attached database).
///
/// If the table cannot be located, return NULL. The value of the two
/// output parameters is undefined in this case.
fn alterFindTable(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    piDb: &mut i32,
    pzDb: &mut Option<ZStr>,
    bAuth: bool,
) -> Option<TableRef> {
    let mut pTab = sqlite3LocateTableItem(db, 0, SrcItemId(pSrc, 0));
    if let Some(t) = pTab.clone() {
        let iDb = db.schema_index(t.borrow().pSchema);
        *pzDb = db.aDb[iDb as usize].zDbSName.clone();
        *piDb = iDb;

        if SQLITE_OK != isRealTable(db, &t, 2) || SQLITE_OK != isAlterableTable(db, &t) {
            pTab = None;
        }
    }
    if let Some(t) = pTab.clone() {
        if bAuth {
            let zTabName = t.borrow().zName.clone();
            let zDb = pzDb.clone();
            if crate::auth::sqlite3AuthCheck(
                db,
                SQLITE_ALTER_TABLE,
                zDb.as_deref(),
                Some(&zTabName),
                None,
            ) != 0
            {
                pTab = None;
            }
        }
    }
    sqlite3SrcListDelete(db, pSrc);
    pTab
}

/// C: `void sqlite3AlterDropConstraint( Parse *pParse, SrcList *pSrc, Token *pCons, Token *pCol )`
///
/// Generate bytecode for one of:
///
///  (1)   ALTER TABLE pSrc DROP CONSTRAINT pCons
///  (2)   ALTER TABLE pSrc ALTER pCol DROP NOT NULL
///
/// One of pCons and pCol must be NULL and the other non-null.
pub fn sqlite3AlterDropConstraint(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    pCons: Option<&Token>,
    pCol: Option<&Token>,
) {
    let mut iDb = 0;
    let mut zDb: Option<ZStr> = None;

    debug_assert!(pCol.is_none() != pCons.is_none());
    debug_assert!(db.src(pSrc).nSrc() == 1);
    let pTab = match alterFindTable(db, pSrc, &mut iDb, &mut zDb, pCons.is_some()) {
        Some(t) => t,
        None => return,
    };

    let zArg: ZStr;
    if let Some(pCons) = pCons {
        let z = sqlite3NameFromToken(db, pCons);
        zArg = mprintf!(db, "%Q", z.as_deref());
    } else {
        let mut iCol = 0;
        if alterFindCol(db, &pTab, pCol.unwrap(), &mut iCol) != 0 {
            return;
        }
        zArg = mprintf!(db, "%d", iCol);
    }

    // Edit the SQL for the named table.
    let zTabName = pTab.borrow().zName.clone();
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_drop_constraint(sql, %s) WHERE type='table' AND tbl_name=%Q COLLATE nocase",
            LEGACY_SCHEMA_TABLE
        ),
        zDb.as_deref(),
        &zArg,
        &zTabName
    );

    // Finally, reload the database schema.
    renameReloadSchema(db, iDb, INITFLAG_AlterDropCons as u16);
}

/// The implementation of SQL function sqlite_fail(MSG). This takes a
/// single argument, and returns it as an error message with the error code
/// set to SQLITE_CONSTRAINT.
fn failConstraintFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let zText = vtext(&mut argv[0]);
    let err = sqlite3_value_int(&mut argv[1]);
    sqlite3_result_error(ctx, zText.as_deref(), -1);
    sqlite3_result_error_code(ctx, err);
}

/// Buffer pCons, which is nCons bytes in size, contains the text of a
/// NOT NULL or CHECK constraint that will be inserted into a CREATE TABLE
/// statement. If successful, this function returns the size of the buffer
/// in bytes not including any trailing whitespace or "--" style comments.
///
/// C-style comments at the end are preserved.  "--" style comments are
/// removed because the comment terminator might be \000, and we are about
/// to insert the pCons[] text into the middle of a larger string, and that
/// will have the effect of removing the comment terminator and messing up
/// the syntax.
fn alterRtrimConstraint(db: &mut Sqlite3, pCons: &[u8]) -> usize {
    let zTmp = mprintf!(db, "%.*s", pCons.len() as i32, pCons);
    let mut iOff = 0usize;
    let mut iEnd = 0usize;

    loop {
        let mut t = 0i32;
        let nToken = get_token(&zTmp, iOff, &mut t);
        if t == TK_ILLEGAL as i32 {
            break;
        }
        if t != TK_SPACE as i32 && (t != TK_COMMENT as i32 || zTmp.get(iOff).copied() != Some(b'-'))
        {
            iEnd = iOff + nToken;
        }
        iOff += nToken;
    }

    iEnd
}

/// The SQL text from token pFirst up to (not including) the last token
/// of the current parse (C `pFirst->z .. pParse->sLastToken.z`).
fn constraint_text(db: &mut Sqlite3, pFirst: &Token) -> ZStr {
    let end = db.ppr().sLastToken.off as usize;
    let start = pFirst.off as usize;
    match &pFirst.z {
        Some(z) if end >= start => z[start..end.min(z.len())].to_vec(),
        _ => Vec::new(),
    }
}

/// C: `void sqlite3AlterSetNotNull( Parse *pParse, SrcList *pSrc, Token *pCol, Token *pFirst )`
///
/// Prepare a statement of the form:
///
///   ALTER TABLE pSrc ALTER pCol SET NOT NULL
pub fn sqlite3AlterSetNotNull(db: &mut Sqlite3, pSrc: SrcListId, pCol: &Token, pFirst: &Token) {
    let mut iCol = 0;
    let mut iDb = 0;
    let mut zDb: Option<ZStr> = None;

    // Look up the table being altered.
    debug_assert!(db.src(pSrc).nSrc() == 1);
    let pTab = match alterFindTable(db, pSrc, &mut iDb, &mut zDb, false) {
        Some(t) => t,
        None => return,
    };

    // Find the column being altered.
    if alterFindCol(db, &pTab, pCol, &mut iCol) != 0 {
        return;
    }

    // Find the length in bytes of the constraint definition
    let pCons = constraint_text(db, pFirst);
    let nCons = alterRtrimConstraint(db, &pCons);

    // Search for a constraint violation. Throw an exception if one is found.
    let zTabName = pTab.borrow().zName.clone();
    nested!(
        db,
        "SELECT sqlite_fail('constraint failed', %d) FROM %Q.%Q AS x WHERE x.%.*s IS NULL",
        SQLITE_CONSTRAINT,
        zDb.as_deref(),
        &zTabName,
        pCol.n as i32,
        pCol.text()
    );

    // Edit the SQL for the named table.
    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_add_constraint(sqlite_drop_constraint(sql, %d), %.*Q, %d) WHERE type='table' AND tbl_name=%Q COLLATE nocase",
            LEGACY_SCHEMA_TABLE
        ),
        zDb.as_deref(),
        iCol,
        nCons as i32,
        &pCons,
        iCol,
        &zTabName
    );

    // Finally, reload the database schema.
    renameReloadSchema(db, iDb, INITFLAG_AlterDropCons as u16);
}

/// Implementation of internal SQL function:
///
///     sqlite_find_constraint(SQL, CONSTRAINT-NAME)
///
/// This function returns true if the SQL passed as the first argument is
/// a CREATE TABLE that contains a constraint with the name
/// CONSTRAINT-NAME, or false otherwise.
fn findConstraintFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let zSql = vtext(&mut argv[0]);
    let zCons = vtext(&mut argv[1]);
    let mut iOff = 0usize;
    let mut t = 0i32;

    let (zSql, zCons) = match (zSql, zCons) {
        (Some(a), Some(b)) => (cstr(&a).to_vec(), b),
        _ => return,
    };
    let zSql = &zSql[..];
    while t != TK_LP as i32 && t != TK_ILLEGAL as i32 {
        iOff += get_token(zSql, iOff, &mut t);
    }

    loop {
        iOff += getConstraintToken(sub(zSql, iOff), &mut t);
        if t == TK_CONSTRAINT as i32 {
            let mut cmp = 0;
            iOff += getWhitespace(sub(zSql, iOff));
            let nTok = getConstraintToken(sub(zSql, iOff), &mut t);
            if quotedCompare(ctx, t, sub(zSql, iOff), nTok, &zCons, &mut cmp) != 0 {
                return;
            }
            if cmp == 0 {
                sqlite3_result_int(ctx, 1);
                return;
            }
        } else if t == TK_ILLEGAL as i32 {
            break;
        }
    }

    sqlite3_result_int(ctx, 0);
}

/// C: `void sqlite3AlterAddConstraint( Parse *pParse, SrcList *pSrc, Token *pFirst, Token *pName, const char *zExpr, int nExpr, Expr *pExpr )`
///
/// Generate bytecode to implement:
///
///    ALTER TABLE pSrc ADD [CONSTRAINT pName] CHECK(pExpr)
///
/// Any "ON CONFLICT" text that occurs after the "CHECK(...)", up
/// until pParse->sLastToken, is included as part of the new constraint.
pub fn sqlite3AlterAddConstraint(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    pFirst: &Token,
    pName: Option<&Token>,
    zExpr: Option<&[u8]>,
    nExpr: i32,
    pExpr: ExprId,
) {
    let mut iDb = 0;
    let mut zDb: Option<ZStr> = None;

    // Look up the table being altered.
    debug_assert!(db.src(pSrc).nSrc() == 1);
    let pTab = match alterFindTable(db, pSrc, &mut iDb, &mut zDb, true) {
        Some(t) => t,
        None => {
            sqlite3ExprDelete(db, pExpr);
            return;
        }
    };

    // Verify that the new CHECK constraint does not contain any
    // internal-use-only function.  Forum post 2026-05-10T01:11:28Z
    let rc = sqlite3ResolveSelfReference(db, Some(&pTab), NC_IsCheck, pExpr, ExprListId::NULL);
    sqlite3ExprDelete(db, pExpr);
    if rc != 0 {
        return;
    }

    // If this new constraint has a name, check that it is not a duplicate
    // of an existing constraint. It is an error if it is.
    let zTabName = pTab.borrow().zName.clone();
    if let Some(pName) = pName {
        let zName = sqlite3NameFromToken(db, pName);

        nested!(
            db,
            format!(
                "SELECT sqlite_fail('constraint %q already exists', %d) FROM \"%w\".{} WHERE type='table' AND tbl_name=%Q COLLATE nocase AND sqlite_find_constraint(sql, %Q)",
                LEGACY_SCHEMA_TABLE
            ),
            zName.as_deref(),
            SQLITE_ERROR,
            zDb.as_deref(),
            &zTabName,
            zName.as_deref()
        );
    }

    // Search for a constraint violation. Throw an exception if one is found.
    nested!(
        db,
        "SELECT sqlite_fail('constraint failed', %d) FROM %Q.%Q WHERE (%.*s) IS FALSE",
        SQLITE_CONSTRAINT,
        zDb.as_deref(),
        &zTabName,
        nExpr,
        zExpr
    );

    // Edit the SQL for the named table.
    let pCons = constraint_text(db, pFirst);
    let nCons = alterRtrimConstraint(db, &pCons);

    nested!(
        db,
        format!(
            "UPDATE \"%w\".{} SET sql = sqlite_add_constraint(sql, %.*Q, -1) WHERE type='table' AND tbl_name=%Q COLLATE nocase",
            LEGACY_SCHEMA_TABLE
        ),
        zDb.as_deref(),
        nCons as i32,
        &pCons,
        &zTabName
    );

    // Finally, reload the database schema.
    renameReloadSchema(db, iDb, INITFLAG_AlterDropCons as u16);
}

/// INTERNAL_FUNCTION(zName, nArg, xFunc)
fn internal_function(zName: &str, nArg: i16, xFunc: fn(&mut Context, &mut [Mem])) -> FuncDef {
    FuncDef {
        nArg,
        funcFlags: SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_INTERNAL
            | SQLITE_UTF8 as u32
            | SQLITE_FUNC_CONSTANT,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(xFunc)),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: zName.as_bytes().to_vec(),
        pDestructor: None,
    }
}

/// C: `void sqlite3AlterFunctions(void)`
///
/// Register built-in functions used to help implement ALTER TABLE.
///
/// Rust: returns the definitions; the caller (sqlite3RegisterBuiltinFunctions)
/// inserts them with `sqlite3InsertBuiltinFuncs(pHash, &sqlite3AlterFunctions())`.
pub fn sqlite3AlterFunctions() -> Vec<FuncDef> {
    vec![
        internal_function("sqlite_rename_column", 9, renameColumnFunc),
        internal_function("sqlite_rename_table", 7, renameTableFunc),
        internal_function("sqlite_rename_test", 7, renameTableTest),
        internal_function("sqlite_drop_column", 3, dropColumnFunc),
        internal_function("sqlite_rename_quotefix", 2, renameQuotefixFunc),
        internal_function("sqlite_drop_constraint", 2, dropConstraintFunc),
        internal_function("sqlite_fail", 2, failConstraintFunc),
        internal_function("sqlite_add_constraint", 3, addConstraintFunc),
        internal_function("sqlite_find_constraint", 2, findConstraintFunc),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dequote_works() {
        let mut z = b"\"ab\"\"c\"".to_vec();
        dequote(&mut z);
        assert_eq!(z, b"ab\"c");
        let mut z = b"[x y]".to_vec();
        dequote(&mut z);
        assert_eq!(z, b"x y");
        let mut z = b"plain".to_vec();
        dequote(&mut z);
        assert_eq!(z, b"plain");
    }

    #[test]
    fn token_next_order() {
        let src: Rc<[u8]> = Rc::from(&b"abc def ghi"[..]);
        let mk = |off| RenameToken {
            p: RENAME_KEY_NULL,
            t: Token {
                z: Some(src.clone()),
                off,
                n: 3,
            },
        };
        let mut c = RenameCtx::default();
        c.pList = vec![mk(4), mk(8), mk(0)];
        assert_eq!(renameColumnTokenNext(&mut c).t.off, 8);
        assert_eq!(renameColumnTokenNext(&mut c).t.off, 4);
        assert_eq!(renameColumnTokenNext(&mut c).t.off, 0);
    }
}
