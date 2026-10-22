//! Port of trigger.c: CREATE/DROP TRIGGER, trigger programs (coded as
//! sub-programs run by OP_Program) and RETURNING.
//!
//! Trigger lists: the C code links triggers through Trigger.pNext; here a
//! list of triggers is a `Vec<TriggerRef>` in the same order (an empty Vec
//! is the C NULL list).
//!
//! Trigger sub-programs: codeRowTrigger() pushes a new Parse onto
//! `db.parses` (with `iToplevel` pointing at the toplevel parse), codes the
//! trigger body into that parse's own Vdbe, moves the opcodes into a
//! SubProgram and pops the sub-parse.  The TriggerPrg/SubProgram is linked
//! into the toplevel parse/Vdbe *before* the body is coded so that recursive
//! references find it (hence SubProgramRef is a RefCell).

use crate::attach::sqlite3FixTriggerStep;
use crate::attach::{sqlite3DbIsNamed, sqlite3FixExpr, sqlite3FixInit, sqlite3FixSrcList};
use crate::auth::sqlite3AuthCheck;
use crate::build::*;
use crate::delete::{sqlite3DeleteFrom, sqlite3SrcListLookup};
use crate::expr::*;
use crate::global::sqlite3Isspace;
use crate::insert::sqlite3Insert;
use crate::int::*;
use crate::resolve::{sqlite3ResolveExprListNames, sqlite3ResolveExprNames};
use crate::select::*;
use crate::update::sqlite3Update;
use crate::upsert::{sqlite3UpsertDelete, sqlite3UpsertDup};
use crate::util::{sqlite3_strnicmp, sqlite3StrICmp};
use crate::vdbeaux::*;
use crate::walker::{sqlite3ExprWalkNoop, sqlite3SelectWalkNoop, sqlite3WalkExprList};
use std::cell::RefCell;
use std::rc::Rc;

/// sqlite3IsToplevel(pParse): true if the current parse is not a trigger
/// sub-parse (C `pParse->pToplevel==0`).
pub fn sqlite3IsToplevel(db: &Sqlite3) -> bool {
    db.ppr().iToplevel == db.parses.len() - 1
}

/// SCHEMA_TABLE(x)
fn SCHEMA_TABLE(iDb: i32) -> &'static [u8] {
    if OMIT_TEMPDB == 0 && iDb == 1 {
        b"sqlite_temp_master"
    } else {
        b"sqlite_master"
    }
}

/// The names of the UPDATE OF column list of a trigger (C pColumns), or
/// None for no list.  Reads owned trees in place (no arena copy).
fn trigger_columns(db: &Sqlite3, t: &Tree<IdListId>) -> Option<Vec<Option<ZStr>>> {
    match t {
        Tree::None => None,
        Tree::Live(id) => {
            if id.is_null() {
                None
            } else {
                Some(db.idl(*id).a.iter().map(|x| x.zName.clone()).collect())
            }
        }
        Tree::Owned(o) => Some(
            o.arena
                .idl(o.root)
                .a
                .iter()
                .map(|x| x.zName.clone())
                .collect(),
        ),
    }
}

/// C: `void sqlite3DeleteTriggerStep(sqlite3 *db, TriggerStep *pTriggerStep)`
/// Delete a linked list of TriggerStep structures.
pub fn sqlite3DeleteTriggerStep(db: &mut Sqlite3, pTriggerStep: TriggerStepId) {
    let mut p = pTriggerStep;
    while p.nn() {
        let s = db.arena.tstep(p).clone();
        p = s.pNext;
        sqlite3ExprDelete(db, s.pWhere);
        sqlite3ExprListDelete(db, s.pExprList);
        sqlite3SelectDelete(db, s.pSelect);
        sqlite3IdListDelete(db, s.pIdList);
        sqlite3UpsertDelete(db, s.pUpsert);
        sqlite3SrcListDelete(db, s.pSrc);
    }
}

/// C: `Trigger * sqlite3TriggerList(Parse *pParse, Table *pTab)`
///
/// Given table pTab, return a list of all the triggers attached to
/// the table.
///
/// All of the triggers on pTab that are in the same database as pTab
/// are already attached to pTab->pTrigger.  But there might be additional
/// triggers on pTab in the TEMP schema.  This routine prepends all
/// TEMP triggers on pTab to the beginning of the pTab->pTrigger list
/// and returns the combined list.
///
/// To state it another way:  This routine returns a list of all triggers
/// that fire off of pTab.  The list will include any TEMP triggers on
/// pTab as well as the triggers lised in pTab->pTrigger.
///
/// Rust: returns the list as a Vec (empty for C NULL).
pub fn sqlite3TriggerList(db: &mut Sqlite3, pTab: &TableRef) -> Vec<TriggerRef> {
    debug_assert!(db.ppr().disableTriggers == 0);
    let (tabSchema, tabName, mut pList) = {
        let t = pTab.borrow();
        (t.pSchema, t.zName.clone(), t.pTrigger.clone())
    };
    if db.aDb.len() < 2 {
        return pList;
    }
    let pTmpSchema = db.aDb[1].pSchema.id;
    let temps: Vec<TriggerRef> = db.aDb[1].pSchema.trigHash.values();
    for pTrig in temps.iter() {
        let (ts, tableNull, matchName, bRet, op) = {
            let t = pTrig.borrow();
            // The RETURNING trigger is the only one with a NULL table name.
            let tableNull = t.bReturning != 0 && t.table.is_empty();
            (
                t.pTabSchema,
                tableNull,
                sqlite3StrICmp(Some(&t.table), Some(&tabName)) == 0,
                t.bReturning,
                t.op,
            )
        };
        if ts == tabSchema && !tableNull && matchName && (ts != pTmpSchema || bRet != 0) {
            pList.insert(0, pTrig.clone());
        } else if op == TK_RETURNING {
            debug_assert!(db.ppr().bReturning != 0);
            {
                let mut t = pTrig.borrow_mut();
                t.table = tabName.clone();
                t.pTabSchema = tabSchema;
            }
            pList.insert(0, pTrig.clone());
        }
    }
    pList
}

/// C: `void sqlite3BeginTrigger( Parse *pParse, Token *pName1, Token *pName2, int tr_tm, int op, IdList *pColumns, SrcList *pTableName, Expr *pWhen, int isTemp, int noErr )`
///
/// This is called by the parser when it sees a CREATE TRIGGER statement
/// up to the point of the BEGIN before the trigger actions.  A Trigger
/// structure is generated based on the information available and stored
/// in pParse->pNewTrigger.  After the trigger actions have been parsed, the
/// sqlite3FinishTrigger() function is called to complete the trigger
/// construction process.
pub fn sqlite3BeginTrigger(
    db: &mut Sqlite3,
    pName1: &Token,
    pName2: &Token,
    tr_tm: i32,
    op: i32,
    pColumns: IdListId,
    pTableName: SrcListId,
    pWhen: ExprId,
    isTemp: i32,
    noErr: i32,
) {
    let mut tr_tm = tr_tm;
    let mut pColumns = pColumns;
    let mut pWhen = pWhen;
    let iDb: i32;
    let pName: Token;

    debug_assert!(op == TK_INSERT as i32 || op == TK_UPDATE as i32 || op == TK_DELETE as i32);
    debug_assert!(op > 0 && op < 0xff);
    'cleanup: {
        let mut orphan = false;
        'orphan: {
            if isTemp != 0 {
                /* If TEMP was specified, then the trigger name may not be qualified. */
                if pName2.n > 0 {
                    errmsg!(db, "temporary trigger may not have qualified name");
                    break 'cleanup;
                }
                iDb = 1;
                pName = pName1.clone();
            } else {
                /* Figure out the db that the trigger will be created in */
                let mut pUnqual = Token::default();
                let i = sqlite3TwoPartName(db, pName1, pName2, &mut pUnqual);
                if i < 0 {
                    break 'cleanup;
                }
                iDb = i;
                pName = pUnqual;
            }
            let mut iDb = iDb;
            if pTableName.is_null() || db.mallocFailed != 0 {
                break 'cleanup;
            }

            /* A long-standing parser bug is that this syntax was allowed:
             **
             **    CREATE TRIGGER attached.demo AFTER INSERT ON attached.tab ....
             **                                                 ^^^^^^^^
             **
             ** To maintain backwards compatibility, ignore the database
             ** name on pTableName if we are reparsing out of the schema table
             */
            if db.init.busy != 0 && iDb != 1 {
                let it = &mut db.srcm(pTableName).a[0];
                debug_assert!(it.fg.fixedSchema == 0);
                debug_assert!(it.fg.isSubquery == 0);
                it.u4.zDatabase = None;
            }

            /* If the trigger name was unqualified, and the table is a temp table,
             ** then set iDb to 1 to create the trigger in the temporary database.
             ** If sqlite3SrcListLookup() returns 0, indicating the table does not
             ** exist, the error is caught by the block below.
             */
            let pTab = sqlite3SrcListLookup(db, pTableName);
            if db.init.busy == 0 && pName2.n == 0 {
                if let Some(t) = &pTab {
                    if t.borrow().pSchema == db.aDb[1].pSchema.id {
                        iDb = 1;
                    }
                }
            }

            /* Ensure the table name matches database name and that the table exists */
            if db.mallocFailed != 0 {
                break 'cleanup;
            }
            debug_assert!(db.src(pTableName).nSrc() == 1);
            let mut sFix = DbFixer::default();
            sqlite3FixInit(db, &mut sFix, iDb, Some(b"trigger"), &pName);
            if sqlite3FixSrcList(db, &mut sFix, pTableName) != 0 {
                break 'cleanup;
            }
            let pTab = match sqlite3SrcListLookup(db, pTableName) {
                Some(t) => t,
                None => {
                    /* The table does not exist. */
                    orphan = true;
                    break 'orphan;
                }
            };
            if pTab.borrow().IsVirtual() {
                errmsg!(db, "cannot create triggers on virtual tables");
                orphan = true;
                break 'orphan;
            }
            if (pTab.borrow().tabFlags & TF_Shadow) != 0 && sqlite3ReadOnlyShadowTables(db) != 0 {
                errmsg!(db, "cannot create triggers on shadow tables");
                orphan = true;
                break 'orphan;
            }

            /* Check that the trigger name is not reserved and that no trigger of the
             ** specified name exists */
            let zName = match sqlite3NameFromToken(db, &pName) {
                Some(z) => z,
                None => break 'cleanup,
            };
            let zTabName = pTab.borrow().zName.clone();
            if sqlite3CheckObjectName(db, Some(&zName), Some(b"trigger"), Some(&zTabName)) != 0 {
                break 'cleanup;
            }
            if !db.ppr().IN_RENAME_OBJECT() {
                if db.aDb[iDb as usize].pSchema.trigHash.find(&zName).is_some() {
                    if noErr == 0 {
                        errmsg!(db, "trigger %T already exists", &pName);
                    } else if db.init.busy != 0 {
                        errmsg!(db, ""); /* Err msg generated by corruptSchema() */
                    } else {
                        sqlite3CodeVerifySchema(db, iDb);
                    }
                    break 'cleanup;
                }
            }

            /* Do not create a trigger on a system table */
            if sqlite3_strnicmp(Some(&zTabName), Some(b"sqlite_"), 7) == 0 {
                errmsg!(db, "cannot create trigger on system table");
                break 'cleanup;
            }

            /* INSTEAD of triggers are only for views and views only support INSTEAD
             ** of triggers.
             */
            let isView = pTab.borrow().IsView();
            if isView && tr_tm != TK_INSTEAD as i32 {
                let z: &[u8] = if tr_tm == TK_BEFORE as i32 {
                    b"BEFORE"
                } else {
                    b"AFTER"
                };
                errmsg!(
                    db,
                    "cannot create %s trigger on view: %S",
                    z,
                    SrcItemId(pTableName, 0)
                );
                orphan = true;
                break 'orphan;
            }
            if !isView && tr_tm == TK_INSTEAD as i32 {
                errmsg!(
                    db,
                    "cannot create INSTEAD OF trigger on table: %S",
                    SrcItemId(pTableName, 0)
                );
                orphan = true;
                break 'orphan;
            }

            if !db.ppr().IN_RENAME_OBJECT() {
                let iTabDb = db.schema_index(pTab.borrow().pSchema);
                let mut code = SQLITE_CREATE_TRIGGER;
                let zDb = db.aDb[iTabDb as usize].zDbSName.clone();
                let zDbTrig = if isTemp != 0 {
                    db.aDb[1].zDbSName.clone()
                } else {
                    zDb.clone()
                };
                if iTabDb == 1 || isTemp != 0 {
                    code = SQLITE_CREATE_TEMP_TRIGGER;
                }
                if sqlite3AuthCheck(db, code, Some(&zName), Some(&zTabName), zDbTrig.as_deref())
                    != 0
                {
                    break 'cleanup;
                }
                if sqlite3AuthCheck(
                    db,
                    SQLITE_INSERT,
                    Some(SCHEMA_TABLE(iTabDb)),
                    None,
                    zDb.as_deref(),
                ) != 0
                {
                    break 'cleanup;
                }
            }

            /* INSTEAD OF triggers can only appear on views and BEFORE triggers
             ** cannot appear on views.  So we might as well translate every
             ** INSTEAD OF trigger into a BEFORE trigger.  It simplifies code
             ** elsewhere.
             */
            if tr_tm == TK_INSTEAD as i32 {
                tr_tm = TK_BEFORE as i32;
            }

            /* Build the Trigger object */
            let zTable = db.src(pTableName).a[0].zName.clone().unwrap_or_default();
            let mut trig = Trigger {
                zName,
                table: zTable.clone(),
                pSchema: db.aDb[iDb as usize].pSchema.id,
                pTabSchema: pTab.borrow().pSchema,
                op: op as u8,
                tr_tm: if tr_tm == TK_BEFORE as i32 {
                    TRIGGER_BEFORE
                } else {
                    TRIGGER_AFTER
                },
                ..Default::default()
            };
            let inRename = db.ppr().IN_RENAME_OBJECT();
            if inRename {
                trig.pWhen = Tree::from_id(pWhen);
                pWhen = ExprId::NULL;
            } else {
                let w = sqlite3ExprDup(db, pWhen, EXPRDUP_REDUCE);
                trig.pWhen = Tree::from_id(w);
            }
            trig.pColumns = Tree::from_id(pColumns);
            pColumns = IdListId::NULL;
            debug_assert!(db.ppr().pNewTrigger.is_none());
            let pTrigger = Rc::new(RefCell::new(trig));
            if inRename {
                /* C: sqlite3RenameTokenRemap(pParse, pTrigger->table,
                 ** pTableName->a[0].zName) */
                crate::alter::sqlite3RenameTokenRemap(
                    db,
                    Some(crate::alter::rename_key_trigger_table(&pTrigger)),
                    crate::alter::rename_key_srcitem_name(SrcItemId(pTableName, 0)),
                );
            }
            db.pp().pNewTrigger = Some(pTrigger);
        }
        if orphan {
            // trigger_orphan_error:
            if db.init.iDb == 1 {
                /* Ticket #3810.
                 ** Normally, whenever a table is dropped, all associated triggers are
                 ** dropped too.  But if a TEMP trigger is created on a non-TEMP table
                 ** and the table is dropped by a different database connection, the
                 ** trigger is not visible to the database connection that does the
                 ** drop so the trigger cannot be dropped.  This results in an
                 ** "orphaned trigger" - a trigger whose associated table is missing.
                 **
                 ** 2020-11-05 see also https://sqlite.org/forum/forumpost/157dc791df
                 */
                db.init.orphanTrigger = 1;
            }
        }
    }

    // trigger_cleanup:
    sqlite3SrcListDelete(db, pTableName);
    sqlite3IdListDelete(db, pColumns);
    sqlite3ExprDelete(db, pWhen);
}

/// C: `void sqlite3FinishTrigger( Parse *pParse, TriggerStep *pStepList, Token *pAll )`
///
/// This routine is called after all of the trigger actions have been parsed
/// in order to complete the process of building the trigger.
pub fn sqlite3FinishTrigger(db: &mut Sqlite3, pStepList: TriggerStepId, pAll: &Token) {
    let mut pTrig = db.pp().pNewTrigger.take(); /* Trigger being finished */
    // C walks its pStepList parameter to the end of the list once it is
    // attached to the trigger, so the final sqlite3DeleteTriggerStep() only
    // deletes a list that was never attached.
    let mut bAttached = false;

    'cleanup: {
        if db.ppr().nErr != 0 || pTrig.is_none() {
            break 'cleanup;
        }
        let trig = pTrig.clone().unwrap();
        let (zName, pSchema) = {
            let t = trig.borrow();
            (t.zName.clone(), t.pSchema)
        };
        let iDb = db.schema_index(pSchema);
        debug_assert!(iDb >= 0 && iDb < db.nDb());
        trig.borrow_mut().step_list = Tree::from_id(pStepList);
        bAttached = true;
        let mut n: i64 = 0;
        let mut p = pStepList;
        while p.nn() {
            db.arena.tstepm(p).pTrig = Rc::downgrade(&trig);
            p = db.arena.tstep(p).pNext;
            n += 1;
        }
        if n > db.aLimit[SQLITE_LIMIT_TRIGGER_STEPS as usize] as i64 {
            errmsg!(db, "trigger \"%w\" contains too many steps", &zName);
            break 'cleanup;
        }
        let nameToken = Token::from_bytes(&zName);
        let mut sFix = DbFixer::default();
        sqlite3FixInit(db, &mut sFix, iDb, Some(b"trigger"), &nameToken);
        let pWhen = {
            let t = trig.borrow().pWhen.clone();
            db.tree(&t)
        };
        if sqlite3FixTriggerStep(db, &mut sFix, pStepList) != 0
            || sqlite3FixExpr(db, &mut sFix, pWhen) != 0
        {
            break 'cleanup;
        }

        if db.ppr().IN_RENAME_OBJECT() {
            debug_assert!(db.init.busy == 0);
            db.pp().pNewTrigger = Some(trig.clone());
            pTrig = None;
        } else if db.init.busy == 0 {
            /* if we are not initializing,
             ** build the sqlite_schema entry
             */

            /* If this is a new CREATE TABLE statement, and if shadow tables
             ** are read-only, and the trigger makes a change to a shadow table,
             ** then raise an error - do not allow the trigger to be created. */
            if sqlite3ReadOnlyShadowTables(db) != 0 {
                let mut pStep = pStepList;
                while pStep.nn() {
                    let (src, next) = {
                        let s = db.arena.tstep(pStep);
                        (s.pSrc, s.pNext)
                    };
                    if src.nn() {
                        let zTab = db.src(src).a[0].zName.clone();
                        if sqlite3ShadowTableName(db, zTab.as_deref()) != 0 {
                            errmsg!(
                                db,
                                "trigger \"%s\" may not write to shadow table \"%s\"",
                                &zName,
                                &zTab
                            );
                            break 'cleanup;
                        }
                    }
                    pStep = next;
                }
            }

            /* Make an entry in the sqlite_schema table */
            if !sqlite3GetVdbe(db) {
                break 'cleanup;
            }
            sqlite3BeginWriteOperation(db, 0, iDb);
            let z: ZStr = pAll.text().to_vec();
            let zDb = db.aDb[iDb as usize].zDbSName.clone();
            let zTable = trig.borrow().table.clone();
            sqlite3NestedParse(
                db,
                Some(b"INSERT INTO %Q.sqlite_master VALUES('trigger',%Q,%Q,0,'CREATE TRIGGER %q')"),
                &[
                    crate::printf::PfArg::from(&zDb),
                    crate::printf::PfArg::from(&zName),
                    crate::printf::PfArg::from(&zTable),
                    crate::printf::PfArg::from(&z),
                ],
            );
            sqlite3ChangeCookie(db, iDb);
            let zWhere = mprintf!(db, "type='trigger' AND name='%q'", &zName);
            sqlite3VdbeAddParseSchemaOp(db, iDb, Some(zWhere), 0);
        }

        if db.init.busy != 0 {
            if let Some(pLink) = pTrig.take() {
                /* The trigger enters the schema: its parse trees become owned. */
                {
                    let mut t = pLink.borrow_mut();
                    t.pWhen.publish(&db.arena);
                    t.pColumns.publish(&db.arena);
                    t.step_list.publish(&db.arena);
                }
                let _old = db.aDb[iDb as usize]
                    .pSchema
                    .trigHash
                    .insert(&zName, pLink.clone());
                let (ls, lts, ltable) = {
                    let t = pLink.borrow();
                    (t.pSchema, t.pTabSchema, t.table.clone())
                };
                if ls == lts {
                    let iTabDb = db.schema_index(lts);
                    let pTab = db.aDb[iTabDb as usize]
                        .pSchema
                        .tblHash
                        .find(&ltable)
                        .cloned();
                    debug_assert!(pTab.is_some());
                    if let Some(pTab) = pTab {
                        pTab.borrow_mut().pTrigger.insert(0, pLink.clone());
                    }
                }
            }
        }
    }

    // triggerfinish_cleanup:
    if let Some(t) = &pTrig {
        sqlite3DeleteTrigger(db, t);
    }
    debug_assert!(db.ppr().IN_RENAME_OBJECT() || db.ppr().pNewTrigger.is_none());
    if !bAttached {
        sqlite3DeleteTriggerStep(db, pStepList);
    }
}

/// Duplicate a range of text from an SQL statement, then convert all
/// whitespace characters into ordinary space characters.
///
/// Rust: `zStart` and `zEnd` stand for the C pointers into the SQL text:
/// they are offsets into the SQL text being parsed (db.ppr().zSql).  This
/// is sqlite3DbSpanDup() (leading and trailing white space trimmed).
fn triggerSpanDup(db: &Sqlite3, zStart: usize, zEnd: usize) -> Option<ZStr> {
    let zSql = db.ppr().zSql.clone()?;
    Some(spanDupText(&zSql, zStart, zEnd))
}

/// The text work of triggerSpanDup(): s[zStart..zEnd], trimmed, with every
/// white-space character converted to a space.
fn spanDupText(s: &[u8], zStart: usize, zEnd: usize) -> ZStr {
    let iEnd = zEnd.min(s.len());
    let iStart = zStart.min(iEnd);
    let mut z = &s[iStart..iEnd];
    while let Some(&c) = z.first() {
        if sqlite3Isspace(c) {
            z = &z[1..];
        } else {
            break;
        }
    }
    while let Some(&c) = z.last() {
        if sqlite3Isspace(c) {
            z = &z[..z.len() - 1];
        } else {
            break;
        }
    }
    /* sqlite3DbStrNDup() stops at a NUL byte */
    if let Some(p) = z.iter().position(|&c| c == 0) {
        z = &z[..p];
    }
    z.iter()
        .map(|&c| if sqlite3Isspace(c) { b' ' } else { c })
        .collect()
}

/// C: `TriggerStep * sqlite3TriggerSelectStep( sqlite3 *db, Select *pSelect, const char *zStart, const char *zEnd )`
///
/// Turn a SELECT statement (that the pSelect parameter points to) into
/// a trigger step.  Return a pointer to a TriggerStep structure.
///
/// The parser calls this routine when it finds a SELECT statement in
/// body of a TRIGGER.
pub fn sqlite3TriggerSelectStep(
    db: &mut Sqlite3,
    pSelect: SelectId,
    zStart: usize,
    zEnd: usize,
) -> TriggerStepId {
    let zSpan = triggerSpanDup(db, zStart, zEnd);
    db.arena.new_tstep(TriggerStep {
        op: TK_SELECT,
        pSelect,
        orconf: OE_Default,
        zSpan,
        ..Default::default()
    })
}

/// Allocate space to hold a new trigger step.  The allocated space
/// holds both the TriggerStep object and the TriggerStep.target.z string.
///
/// If an OOM error occurs, NULL is returned and db->mallocFailed is set.
fn triggerStepAllocate(
    db: &mut Sqlite3,
    op: u8,
    pTabList: SrcListId,
    zStart: usize,
    zEnd: usize,
) -> TriggerStepId {
    let mut pTriggerStep = TriggerStepId::NULL;

    if db.ppr().nErr == 0 {
        let qualified = match &db.ppr().pNewTrigger {
            Some(pNew) => {
                pNew.borrow().pSchema != db.aDb[1].pSchema.id
                    && db.src(pTabList).a[0].u4.zDatabase.is_some()
            }
            None => false,
        };
        if qualified {
            errmsg!(
                db,
                "qualified table names are not allowed on INSERT, UPDATE, and DELETE statements within triggers"
            );
        } else {
            let pSrc = sqlite3SrcListDup(db, pTabList, EXPRDUP_REDUCE);
            let zSpan = triggerSpanDup(db, zStart, zEnd);
            pTriggerStep = db.arena.new_tstep(TriggerStep {
                pSrc,
                op,
                zSpan,
                ..Default::default()
            });
            if pSrc.nn() && db.ppr().IN_RENAME_OBJECT() {
                crate::alter::sqlite3RenameTokenRemap(
                    db,
                    Some(crate::alter::rename_key_srcitem_name(SrcItemId(pSrc, 0))),
                    crate::alter::rename_key_srcitem_name(SrcItemId(pTabList, 0)),
                );
            }
        }
    }

    sqlite3SrcListDelete(db, pTabList);
    pTriggerStep
}

/// C: `TriggerStep * sqlite3TriggerInsertStep( Parse *pParse, SrcList *pTabList, IdList *pColumn, Select *pSelect, u8 orconf, Upsert *pUpsert, const char *zStart, const char *zEnd )`
///
/// Build a trigger step out of an INSERT statement.  Return a pointer
/// to the new trigger step.
///
/// The parser calls this routine when it sees an INSERT inside the
/// body of a trigger.
pub fn sqlite3TriggerInsertStep(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pColumn: IdListId,
    pSelect: SelectId,
    orconf: u8,
    pUpsert: UpsertId,
    zStart: usize,
    zEnd: usize,
) -> TriggerStepId {
    let mut pSelect = pSelect;
    debug_assert!(pSelect.nn() || db.mallocFailed != 0);

    let pTriggerStep = triggerStepAllocate(db, TK_INSERT, pTabList, zStart, zEnd);
    if pTriggerStep.nn() {
        let sel = if db.ppr().IN_RENAME_OBJECT() {
            let s = pSelect;
            pSelect = SelectId::NULL;
            s
        } else {
            sqlite3SelectDup(db, pSelect, EXPRDUP_REDUCE)
        };
        {
            let s = db.arena.tstepm(pTriggerStep);
            s.pSelect = sel;
            s.pIdList = pColumn;
            s.pUpsert = pUpsert;
            s.orconf = orconf;
        }
        if pUpsert.nn() {
            let t = db.arena.ups(pUpsert).pUpsertTarget;
            sqlite3HasExplicitNulls(db, t);
        }
    } else {
        sqlite3IdListDelete(db, pColumn);
        sqlite3UpsertDelete(db, pUpsert);
    }
    sqlite3SelectDelete(db, pSelect);

    pTriggerStep
}

/// C: `TriggerStep * sqlite3TriggerUpdateStep( Parse *pParse, SrcList *pTabList, SrcList *pFrom, ExprList *pEList, Expr *pWhere, u8 orconf, const char *zStart, const char *zEnd )`
///
/// Construct a trigger step that implements an UPDATE statement and return
/// a pointer to that trigger step.  The parser calls this routine when it
/// sees an UPDATE statement inside the body of a CREATE TRIGGER.
pub fn sqlite3TriggerUpdateStep(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pFrom: SrcListId,
    pEList: ExprListId,
    pWhere: ExprId,
    orconf: u8,
    zStart: usize,
    zEnd: usize,
) -> TriggerStepId {
    let mut pEList = pEList;
    let mut pWhere = pWhere;
    let mut pFrom = pFrom;

    let pTriggerStep = triggerStepAllocate(db, TK_UPDATE, pTabList, zStart, zEnd);
    if pTriggerStep.nn() {
        let mut pFromDup;
        let rename = db.ppr().IN_RENAME_OBJECT();
        if rename {
            let s = db.arena.tstepm(pTriggerStep);
            s.pExprList = pEList;
            s.pWhere = pWhere;
            pFromDup = pFrom;
            pEList = ExprListId::NULL;
            pWhere = ExprId::NULL;
            pFrom = SrcListId::NULL;
        } else {
            let l = sqlite3ExprListDup(db, pEList, EXPRDUP_REDUCE);
            let w = sqlite3ExprDup(db, pWhere, EXPRDUP_REDUCE);
            pFromDup = sqlite3SrcListDup(db, pFrom, EXPRDUP_REDUCE);
            let s = db.arena.tstepm(pTriggerStep);
            s.pExprList = l;
            s.pWhere = w;
        }
        db.arena.tstepm(pTriggerStep).orconf = orconf;

        if pFromDup.nn() && !rename {
            let pSub = sqlite3SelectNew(
                db,
                ExprListId::NULL,
                pFromDup,
                ExprId::NULL,
                ExprListId::NULL,
                ExprId::NULL,
                ExprListId::NULL,
                SF_NestedFrom,
                ExprId::NULL,
            );
            let tas = Token::default();
            pFromDup =
                sqlite3SrcListAppendFromTerm(db, SrcListId::NULL, None, None, &tas, pSub, None);
        }
        let src = db.arena.tstep(pTriggerStep).pSrc;
        if pFromDup.nn() && src.nn() {
            let n = sqlite3SrcListAppendList(db, src, pFromDup);
            db.arena.tstepm(pTriggerStep).pSrc = n;
        } else {
            sqlite3SrcListDelete(db, pFromDup);
        }
    }
    sqlite3ExprListDelete(db, pEList);
    sqlite3ExprDelete(db, pWhere);
    sqlite3SrcListDelete(db, pFrom);
    pTriggerStep
}

/// C: `TriggerStep * sqlite3TriggerDeleteStep( Parse *pParse, SrcList *pTabList, Expr *pWhere, const char *zStart, const char *zEnd )`
///
/// Construct a trigger step that implements a DELETE statement and return
/// a pointer to that trigger step.  The parser calls this routine when it
/// sees a DELETE statement inside the body of a CREATE TRIGGER.
pub fn sqlite3TriggerDeleteStep(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pWhere: ExprId,
    zStart: usize,
    zEnd: usize,
) -> TriggerStepId {
    let mut pWhere = pWhere;
    let pTriggerStep = triggerStepAllocate(db, TK_DELETE, pTabList, zStart, zEnd);
    if pTriggerStep.nn() {
        let w = if db.ppr().IN_RENAME_OBJECT() {
            let w = pWhere;
            pWhere = ExprId::NULL;
            w
        } else {
            sqlite3ExprDup(db, pWhere, EXPRDUP_REDUCE)
        };
        let s = db.arena.tstepm(pTriggerStep);
        s.pWhere = w;
        s.orconf = OE_Default;
    }
    sqlite3ExprDelete(db, pWhere);
    pTriggerStep
}

/// C: `void sqlite3DeleteTrigger(sqlite3 *db, Trigger *pTrigger)`
///
/// Recursively delete a Trigger structure.  Memory is reference counted;
/// only the live (arena) parse trees of the trigger are "deleted".
pub fn sqlite3DeleteTrigger(db: &mut Sqlite3, pTrigger: &TriggerRef) {
    let (bRet, steps, when, cols) = {
        let t = pTrigger.borrow();
        (
            t.bReturning,
            t.step_list.live(),
            t.pWhen.live(),
            t.pColumns.live(),
        )
    };
    if bRet != 0 {
        return;
    }
    if let Some(s) = steps {
        sqlite3DeleteTriggerStep(db, s);
    }
    if let Some(w) = when {
        sqlite3ExprDelete(db, w);
    }
    if let Some(c) = cols {
        sqlite3IdListDelete(db, c);
    }
}

/// C: `void sqlite3DropTrigger(Parse *pParse, SrcList *pName, int noErr)`
///
/// This function is called to drop a trigger from the database schema.
///
/// This may be called directly from the parser and therefore identifies
/// the trigger by name.  The sqlite3DropTriggerPtr() routine does the
/// same job as this routine except it takes a pointer to the trigger
/// instead of the trigger name.
pub fn sqlite3DropTrigger(db: &mut Sqlite3, pName: SrcListId, noErr: i32) {
    'cleanup: {
        if db.mallocFailed != 0 {
            break 'cleanup;
        }
        if SQLITE_OK != crate::prepare::sqlite3ReadSchema(db) {
            break 'cleanup;
        }

        debug_assert!(db.src(pName).nSrc() == 1);
        let (zDb, zName) = {
            let it = &db.src(pName).a[0];
            debug_assert!(it.fg.fixedSchema == 0 && it.fg.isSubquery == 0);
            (
                it.u4.zDatabase.clone(),
                it.zName.clone().unwrap_or_default(),
            )
        };
        let mut pTrigger: Option<TriggerRef> = None;
        let mut i = OMIT_TEMPDB;
        while i < db.nDb() {
            let j = if i < 2 { i ^ 1 } else { i }; /* Search TEMP before MAIN */
            if zDb.is_some() && sqlite3DbIsNamed(db, j, zDb.as_deref()) == 0 {
                i += 1;
                continue;
            }
            pTrigger = db.aDb[j as usize].pSchema.trigHash.find(&zName).cloned();
            if pTrigger.is_some() {
                break;
            }
            i += 1;
        }
        match pTrigger {
            None => {
                if noErr == 0 {
                    errmsg!(db, "no such trigger: %S", SrcItemId(pName, 0));
                } else {
                    sqlite3CodeVerifyNamedSchema(db, zDb.as_deref());
                }
                db.pp().checkSchema = 1;
                break 'cleanup;
            }
            Some(t) => sqlite3DropTriggerPtr(db, &t),
        }
    }

    // drop_trigger_cleanup:
    sqlite3SrcListDelete(db, pName);
}

/// Return a pointer to the Table structure for the table that a trigger
/// is set on.
fn tableOfTrigger(db: &Sqlite3, pTrigger: &TriggerRef) -> Option<TableRef> {
    let (ts, table) = {
        let t = pTrigger.borrow();
        (t.pTabSchema, t.table.clone())
    };
    let i = db.schema_index(ts);
    if i < 0 {
        return None;
    }
    db.aDb[i as usize].pSchema.tblHash.find(&table).cloned()
}

/// C: `void sqlite3DropTriggerPtr(Parse *pParse, Trigger *pTrigger)`
/// Drop a trigger given a pointer to that trigger.
pub fn sqlite3DropTriggerPtr(db: &mut Sqlite3, pTrigger: &TriggerRef) {
    let iDb = db.schema_index(pTrigger.borrow().pSchema);
    debug_assert!(iDb >= 0 && iDb < db.nDb());
    let pTable = tableOfTrigger(db, pTrigger);
    let zTrigName = pTrigger.borrow().zName.clone();
    if let Some(pTable) = &pTable {
        let mut code = SQLITE_DROP_TRIGGER;
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        let zTab = SCHEMA_TABLE(iDb);
        if iDb == 1 {
            code = SQLITE_DROP_TEMP_TRIGGER;
        }
        let zTabName = pTable.borrow().zName.clone();
        if sqlite3AuthCheck(db, code, Some(&zTrigName), Some(&zTabName), zDb.as_deref()) != 0
            || sqlite3AuthCheck(db, SQLITE_DELETE, Some(zTab), None, zDb.as_deref()) != 0
        {
            return;
        }
    }

    /* Generate code to destroy the database record of the trigger.
     */
    if sqlite3GetVdbe(db) {
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        sqlite3NestedParse(
            db,
            Some(b"DELETE FROM %Q.sqlite_master WHERE name=%Q AND type='trigger'"),
            &[
                crate::printf::PfArg::from(&zDb),
                crate::printf::PfArg::from(&zTrigName),
            ],
        );
        sqlite3ChangeCookie(db, iDb);
        sqlite3VdbeAddOp4(
            db.v(),
            OP_DropTrigger,
            iDb,
            0,
            0,
            P4::Z(zTrigName),
            P4_DYNAMIC as i32,
        );
    }
}

/// C: `void sqlite3UnlinkAndDeleteTrigger(sqlite3 *db, int iDb, const char *zName)`
/// Remove a trigger from the hash tables of the sqlite* pointer.
pub fn sqlite3UnlinkAndDeleteTrigger(db: &mut Sqlite3, iDb: i32, zName: Option<&[u8]>) {
    let zName = zName.unwrap_or(b"");
    let pTrigger = db.aDb[iDb as usize].pSchema.trigHash.remove(zName);
    if let Some(pTrigger) = pTrigger {
        let same = {
            let t = pTrigger.borrow();
            t.pSchema == t.pTabSchema
        };
        if same {
            if let Some(pTab) = tableOfTrigger(db, &pTrigger) {
                let mut t = pTab.borrow_mut();
                if let Some(pos) = t.pTrigger.iter().position(|x| Rc::ptr_eq(x, &pTrigger)) {
                    t.pTrigger.remove(pos);
                }
            }
        }
        sqlite3DeleteTrigger(db, &pTrigger);
        db.mDbFlags |= DBFLAG_SchemaChange;
    }
}

/// pEList is the SET clause of an UPDATE statement.  Each entry
/// in pEList is of the format <id>=<expr>.  If any of the entries
/// in pEList have an <id> which matches an identifier in pIdList,
/// then return TRUE.  If pIdList==NULL, then it is considered a
/// wildcard that matches anything.  Likewise if pEList==NULL then
/// it matches anything so always return true.  Return false only
/// if there is no match.
fn checkColumnOverlap(db: &Sqlite3, pIdList: &Tree<IdListId>, pEList: ExprListId) -> bool {
    let names = match trigger_columns(db, pIdList) {
        None => return true,
        Some(n) => n,
    };
    if pEList.is_null() {
        return true;
    }
    for it in db.el(pEList).a.iter() {
        /* sqlite3IdListIndex() */
        if names
            .iter()
            .any(|n| sqlite3StrICmp(n.as_deref(), it.zEName.as_deref()) == 0)
        {
            return true;
        }
    }
    false
}

/// Return true if any TEMP triggers exist
fn tempTriggersExist(db: &Sqlite3) -> bool {
    if db.aDb.len() < 2 {
        return false;
    }
    db.aDb[1].pSchema.trigHash.count() != 0
}

/// Return a list of all triggers on table pTab if there exists at least
/// one trigger that must be fired when an operation of type 'op' is
/// performed on the table, and, if that operation is an UPDATE, if at
/// least one of the columns in pChanges is being modified.
fn triggersReallyExist(
    db: &mut Sqlite3,
    pTab: &TableRef,
    op: i32,
    pChanges: ExprListId,
    pMask: Option<&mut i32>,
) -> Vec<TriggerRef> {
    let mut mask = 0i32;
    let mut pList = sqlite3TriggerList(db, pTab);
    debug_assert!(
        pList.is_empty()
            || !pTab.borrow().IsVirtual()
            || (pList[0].borrow().bReturning != 0 && pList.len() == 1)
    );
    'exit: {
        if !pList.is_empty() {
            let tabTrig: Vec<TriggerRef> = pTab.borrow().pTrigger.clone();
            if (db.flags & SQLITE_EnableTrigger) == 0
                && !tabTrig.is_empty()
                && db.schema_index(tabTrig[0].borrow().pSchema) != 1
            {
                /* The SQLITE_DBCONFIG_ENABLE_TRIGGER setting is off.  That means that
                 ** only TEMP triggers are allowed.  Truncate the pList so that it
                 ** includes only TEMP triggers */
                let nTemp = pList.len() - tabTrig.len();
                if nTemp == 0 {
                    pList.clear();
                    break 'exit;
                }
                pList.truncate(nTemp);
            }
            for p in pList.iter() {
                let (pop, ptm, bRet) = {
                    let t = p.borrow();
                    (t.op as i32, t.tr_tm as i32, t.bReturning)
                };
                let overlap = {
                    let cols = p.borrow().pColumns.clone();
                    checkColumnOverlap(db, &cols, pChanges)
                };
                if pop == op && overlap {
                    mask |= ptm;
                } else if pop == TK_RETURNING as i32 {
                    /* The first time a RETURNING trigger is seen, the "op" value tells
                     ** us what time of trigger it should be. */
                    debug_assert!(sqlite3IsToplevel(db));
                    let tm;
                    if pTab.borrow().IsVirtual() {
                        if op != TK_INSERT as i32 {
                            let z: &[u8] = if op == TK_DELETE as i32 {
                                b"DELETE"
                            } else {
                                b"UPDATE"
                            };
                            errmsg!(db, "%s RETURNING is not available on virtual tables", z);
                        }
                        tm = TRIGGER_BEFORE;
                    } else {
                        tm = TRIGGER_AFTER;
                    }
                    {
                        let mut t = p.borrow_mut();
                        t.op = op as u8;
                        t.tr_tm = tm;
                    }
                    mask |= tm as i32;
                } else if bRet != 0
                    && pop == TK_INSERT as i32
                    && op == TK_UPDATE as i32
                    && sqlite3IsToplevel(db)
                {
                    /* Also fire a RETURNING trigger for an UPSERT */
                    mask |= ptm;
                }
            }
        }
    }
    // exit_triggers_exist:
    if let Some(m) = pMask {
        *m = mask;
    }
    if mask != 0 { pList } else { Vec::new() }
}

/// C: `Trigger * sqlite3TriggersExist( Parse *pParse, Table *pTab, int op, ExprList *pChanges, int *pMask )`
///
/// Rust: returns the trigger list (empty for C NULL); `pMask` is None for
/// the C NULL pointer.
pub fn sqlite3TriggersExist(
    db: &mut Sqlite3,
    pTab: &TableRef,
    op: i32,
    pChanges: ExprListId,
    pMask: Option<&mut i32>,
) -> Vec<TriggerRef> {
    if (pTab.borrow().pTrigger.is_empty() && !tempTriggersExist(db))
        || db.ppr().disableTriggers != 0
    {
        if let Some(m) = pMask {
            *m = 0;
        }
        return Vec::new();
    }
    triggersReallyExist(db, pTab, op, pChanges, pMask)
}

/// Return true if the pExpr term from the RETURNING clause argument
/// list is of the form "*".  Raise an error if the terms if of the
/// form "table.*".
fn isAsteriskTerm(db: &mut Sqlite3, pTerm: ExprId) -> bool {
    let (op, l, r) = {
        let e = db.ex(pTerm);
        (e.op, e.pLeft, e.pRight)
    };
    if op == TK_ASTERISK {
        return true;
    }
    if op != TK_DOT {
        return false;
    }
    debug_assert!(r.nn());
    debug_assert!(l.nn());
    if db.ex(r).op != TK_ASTERISK {
        return false;
    }
    errmsg!(db, "RETURNING may not use \"TABLE.*\" wildcards");
    true
}

/// The input list pList is the list of result set terms from a RETURNING
/// clause.  The table that we are returning from is pTab.
///
/// This routine makes a copy of the pList, and at the same time expands
/// any "*" wildcards to be the complete set of columns from pTab.
fn sqlite3ExpandReturning(db: &mut Sqlite3, pList: ExprListId, pTab: &TableRef) -> ExprListId {
    let mut pNew = ExprListId::NULL;
    let n = db.el(pList).nExpr();
    for i in 0..n {
        let (pOldExpr, zEName, eEName) = {
            let it = &db.el(pList).a[i as usize];
            (it.pExpr, it.zEName.clone(), it.fg.eEName)
        };
        if pOldExpr.is_null() {
            continue;
        }
        if isAsteriskTerm(db, pOldExpr) {
            let cols: Vec<(bool, ZStr)> = pTab
                .borrow()
                .aCol
                .iter()
                .map(|c| (IsHiddenColumn(c), c.zCnName.clone()))
                .collect();
            for (hidden, zCn) in cols.iter() {
                if *hidden {
                    continue;
                }
                let pNewExpr = sqlite3Expr(db, TK_ID as i32, Some(zCn));
                pNew = sqlite3ExprListAppend(db, pNew, pNewExpr);
                if db.mallocFailed == 0 {
                    let it = db.elm(pNew).a.last_mut().unwrap();
                    it.zEName = Some(zCn.clone());
                    it.fg.eEName = ENAME_NAME;
                }
            }
        } else {
            let pNewExpr = sqlite3ExprDup(db, pOldExpr, 0);
            pNew = sqlite3ExprListAppend(db, pNew, pNewExpr);
            if db.mallocFailed == 0 && zEName.is_some() {
                let it = db.elm(pNew).a.last_mut().unwrap();
                it.zEName = zEName;
                it.fg.eEName = eEName;
            }
        }
    }
    pNew
}

/// If the Expr node is a subquery or an EXISTS operator or an IN operator that
/// uses a subquery, and if the subquery is SF_Correlated, then mark the
/// expression as EP_VarSelect.
fn sqlite3ReturningSubqueryVarSelect(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (useX, sel) = {
        let e = w.db.ex(pExpr);
        (e.UseXSelect(), e.x.pSelect)
    };
    if useX && sel.nn() && (w.db.sel(sel).selFlags & SF_Correlated) != 0 {
        w.db.exm(pExpr).SetProperty(EP_VarSelect);
    }
    WRC_Continue
}

/// If the SELECT references the table pWalker->u.pTab, then do two things:
///
///    (1) Mark the SELECT as as SF_Correlated.
///    (2) Set pWalker->eCode to non-zero so that the caller will know
///        that (1) has happened.
fn sqlite3ReturningSubqueryCorrelated(w: &mut Walker<'_>, pSelect: SelectId) -> i32 {
    let pSrc = w.db.sel(pSelect).pSrc;
    debug_assert!(pSrc.nn());
    let pTab = w.u.pTab.clone().unwrap();
    let n = w.db.src(pSrc).nSrc();
    for i in 0..n {
        let hit = match &w.db.src(pSrc).a[i as usize].pSTab {
            Some(t) => Rc::ptr_eq(t, &pTab),
            None => false,
        };
        if hit {
            w.db.selm(pSelect).selFlags |= SF_Correlated;
            w.eCode = 1;
            break;
        }
    }
    WRC_Continue
}

/// Scan the expression list that is the argument to RETURNING looking
/// for subqueries that depend on the table which is being modified in the
/// statement that is hosting the RETURNING clause (pTab).  Mark all such
/// subqueries as SF_Correlated.  If the subqueries are part of an
/// expression, mark the expression as EP_VarSelect.
///
/// https://sqlite.org/forum/forumpost/2c83569ce8945d39
fn sqlite3ProcessReturningSubqueries(db: &mut Sqlite3, pEList: ExprListId, pTab: &TableRef) {
    let mut w = Walker::new(db);
    w.xExprCallback = Some(sqlite3ExprWalkNoop);
    w.xSelectCallback = Some(sqlite3ReturningSubqueryCorrelated);
    w.u.pTab = Some(pTab.clone());
    sqlite3WalkExprList(&mut w, pEList);
    if w.eCode != 0 {
        w.xExprCallback = Some(sqlite3ReturningSubqueryVarSelect);
        w.xSelectCallback = Some(sqlite3SelectWalkNoop);
        sqlite3WalkExprList(&mut w, pEList);
    }
}

/// Generate code for the RETURNING trigger.  Unlike other triggers
/// that invoke a subprogram in the bytecode, the code for RETURNING
/// is generated in-line.
fn codeReturningTrigger(db: &mut Sqlite3, pTrigger: &TriggerRef, pTab: &TableRef, regIn: i32) {
    if db.ppr().bReturning == 0 {
        /* This RETURNING trigger must be for a different statement as
         ** this statement lacks a RETURNING clause. */
        return;
    }
    let pReturnEL = match &db.ppr().u1.d.pReturning {
        Some(r) => {
            if !Rc::ptr_eq(&r.retTrig, pTrigger) {
                /* This RETURNING trigger is for a different statement */
                return;
            }
            r.pReturnEL
        }
        None => return,
    };
    let zTabName = pTab.borrow().zName.clone();
    let el = sqlite3ExprListDup(db, pReturnEL, 0);
    let pFrom = db.arena.new_srclist(SrcList {
        a: vec![SrcItem {
            pSTab: Some(pTab.clone()),
            zName: Some(zTabName), /* tag-20240424-1 */
            iCursor: -1,
            ..Default::default()
        }],
    });
    let sSelect = db.arena.new_select(Select {
        pEList: el,
        pSrc: pFrom,
        ..Default::default()
    });
    sqlite3SelectPrep(db, sSelect, NcId::NULL);
    if db.ppr().nErr == 0 {
        debug_assert!(db.mallocFailed == 0);
        sqlite3GenerateColumnNames(db, sSelect);
    }
    let el = db.sel(sSelect).pEList;
    sqlite3ExprListDelete(db, el);
    let pNew = sqlite3ExpandReturning(db, pReturnEL, pTab);
    if db.ppr().nErr == 0 {
        let nNew = db.el(pNew).nExpr();
        let needInit = db.ppr().u1.d.pReturning.as_ref().unwrap().nRetCol == 0;
        if needInit {
            let iRetCur = db.ppr().nTab;
            db.pp().nTab += 1;
            let r = db.pp().u1.d.pReturning.as_mut().unwrap();
            r.nRetCol = nNew;
            r.iRetCur = iRetCur;
        }
        let sNC = db.arena.new_nc(NameContext {
            uNC: NameContextU {
                iBaseReg: regIn,
                ..Default::default()
            },
            ncFlags: NC_UBaseReg,
            ..Default::default()
        });
        let trigOp = pTrigger.borrow().op;
        db.pp().eTriggerOp = trigOp;
        db.pp().pTriggerTab = Some(pTab.clone());
        if sqlite3ResolveExprListNames(db, sNC, pNew) == SQLITE_OK && db.mallocFailed == 0 {
            let nCol = db.el(pNew).nExpr();
            let reg = db.ppr().nMem + 1;
            sqlite3ProcessReturningSubqueries(db, pNew, pTab);
            db.pp().nMem += nCol + 2;
            let iRetCur = {
                let r = db.pp().u1.d.pReturning.as_mut().unwrap();
                r.iRetReg = reg;
                r.iRetCur
            };
            let mut i = 0;
            while i < nCol {
                let pCol = db.el(pNew).a[i as usize].pExpr;
                debug_assert!(pCol.nn()); /* Due to !db->mallocFailed ~9 lines above */
                sqlite3ExprCodeFactorable(db, pCol, reg + i);
                if sqlite3ExprAffinity(db, pCol) == SQLITE_AFF_REAL {
                    sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, reg + i);
                }
                i += 1;
            }
            sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, reg, i, reg + i);
            sqlite3VdbeAddOp2(db.v(), OP_NewRowid, iRetCur, reg + i + 1);
            sqlite3VdbeAddOp3(db.v(), OP_Insert, iRetCur, reg + i, reg + i + 1);
        }
    }
    sqlite3ExprListDelete(db, pNew);
    db.pp().eTriggerOp = 0;
    db.pp().pTriggerTab = None;
}

/// Generate VDBE code for the statements inside the body of a single
/// trigger.
fn codeTriggerProgram(db: &mut Sqlite3, pStepList: TriggerStepId, orconf: i32) -> i32 {
    debug_assert!(db.ppr().pTriggerTab.is_some() && !sqlite3IsToplevel(db));
    debug_assert!(pStepList.nn());
    let mut pStep = pStepList;
    while pStep.nn() {
        let s = db.arena.tstep(pStep).clone();
        /* Figure out the ON CONFLICT policy that will be used for this step
         ** of the trigger program. If the statement that caused this trigger
         ** to fire had an explicit ON CONFLICT, then use it. Otherwise, use
         ** the ON CONFLICT policy that was specified as part of the trigger
         ** step statement. Example:
         **
         **   CREATE TRIGGER AFTER INSERT ON t1 BEGIN;
         **     INSERT OR REPLACE INTO t2 VALUES(new.a, new.b);
         **   END;
         **
         **   INSERT INTO t1 ... ;            -- insert into t2 uses REPLACE policy
         **   INSERT OR IGNORE INTO t1 ... ;  -- insert into t2 uses IGNORE policy
         */
        db.pp().eOrconf = if orconf == OE_Default as i32 {
            s.orconf
        } else {
            orconf as u8
        };
        debug_assert!(db.ppr().okConstFactor == 0);

        if let Some(zSpan) = &s.zSpan {
            let z = mprintf!(db, "-- %s", zSpan);
            sqlite3VdbeAddOp4(
                db.v(),
                OP_Trace,
                0x7fffffff,
                1,
                0,
                P4::Z(z),
                P4_DYNAMIC as i32,
            );
        }

        let eOrconf = db.ppr().eOrconf as i32;
        match s.op {
            TK_UPDATE => {
                let a = sqlite3SrcListDup(db, s.pSrc, 0);
                let b = sqlite3ExprListDup(db, s.pExprList, 0);
                let c = sqlite3ExprDup(db, s.pWhere, 0);
                sqlite3Update(
                    db,
                    a,
                    b,
                    c,
                    eOrconf,
                    ExprListId::NULL,
                    ExprId::NULL,
                    UpsertId::NULL,
                );
                sqlite3VdbeAddOp0(db.v(), OP_ResetCount);
            }
            TK_INSERT => {
                let a = sqlite3SrcListDup(db, s.pSrc, 0);
                let b = sqlite3SelectDup(db, s.pSelect, 0);
                let c = sqlite3IdListDup(db, s.pIdList);
                let d = sqlite3UpsertDup(db, s.pUpsert);
                sqlite3Insert(db, a, b, c, eOrconf, d);
                sqlite3VdbeAddOp0(db.v(), OP_ResetCount);
            }
            TK_DELETE => {
                let a = sqlite3SrcListDup(db, s.pSrc, 0);
                let b = sqlite3ExprDup(db, s.pWhere, 0);
                sqlite3DeleteFrom(db, a, b, ExprListId::NULL, ExprId::NULL);
                sqlite3VdbeAddOp0(db.v(), OP_ResetCount);
            }
            _ => {
                debug_assert!(s.op == TK_SELECT);
                let mut sDest = SelectDest::default();
                let pSelect = sqlite3SelectDup(db, s.pSelect, 0);
                sqlite3SelectDestInit(&mut sDest, SRT_Discard as i32, 0);
                sqlite3Select(db, pSelect, &mut sDest);
                sqlite3SelectDelete(db, pSelect);
            }
        }
        pStep = s.pNext;
    }

    0
}

/// Parse context structure pFrom has just been used to create a sub-vdbe
/// (trigger program). If an error has occurred, transfer error information
/// from pFrom to pTo.  (Rust: indexes into db.parses.)
fn transferParseError(db: &mut Sqlite3, iTo: usize, iFrom: usize) {
    if db.parses[iTo].nErr == 0 {
        let (msg, nErr, rc) = {
            let f = &mut db.parses[iFrom];
            (f.zErrMsg.take(), f.nErr, f.rc)
        };
        let t = &mut db.parses[iTo];
        t.zErrMsg = msg;
        t.nErr = nErr;
        t.rc = rc;
    } else {
        db.parses[iFrom].zErrMsg = None;
    }
}

/// Create and populate a new TriggerPrg object with a sub-program
/// implementing trigger pTrigger with ON CONFLICT policy orconf.
fn codeRowTrigger(
    db: &mut Sqlite3,
    pTrigger: &TriggerRef,
    pTab: &TableRef,
    orconf: i32,
) -> Option<TriggerPrg> {
    /* Ensure that triggers are not chained too deep.  This test is linear
     ** in the chaining depth, but sensible code ought not be chaining
     ** triggers excessively, so that shouldn't be a problem.
     */
    let nDepth = (db.parses.len() - 1) as i32;
    if nDepth >= db.aLimit[SQLITE_LIMIT_TRIGGER_DEPTH as usize] {
        errmsg!(db, "triggers nested too deep");
        return None;
    }

    let iTop = db.ppr().iToplevel;
    debug_assert!(db.parses[iTop].pVdbe.is_some());

    /* Allocate the TriggerPrg and SubProgram objects. To ensure that they
     ** are freed if an error occurs, link them into the Parse.pTriggerPrg
     ** list of the top-level Parse object sooner rather than later.  */
    let pProgram: SubProgramRef = Rc::new(RefCell::new(SubProgram::default()));
    sqlite3VdbeLinkSubProgram(db.parses[iTop].pVdbe.as_deref_mut().unwrap(), &pProgram);
    db.parses[iTop].pTriggerPrg.insert(
        0,
        TriggerPrg {
            pTrigger: pTrigger.clone(),
            pProgram: pProgram.clone(),
            orconf,
            aColmask: [0xffffffff, 0xffffffff],
        },
    );

    /* Allocate and populate a new Parse context to use for coding the
     ** trigger sub-program.  */
    let (zTrigName, trigOp, trigWhen, trigSteps) = {
        let t = pTrigger.borrow();
        (t.zName.clone(), t.op, t.pWhen.clone(), t.step_list.clone())
    };
    let (nQueryLoop, prepFlags) = {
        let p = db.ppr();
        (p.nQueryLoop, p.prepFlags)
    };
    let iOuter = db.parses.len() - 1;
    db.parses.push(Parse {
        iToplevel: iTop,
        pTriggerTab: Some(pTab.clone()),
        zAuthContext: if zTrigName.is_empty() {
            None
        } else {
            Some(zTrigName.clone())
        },
        eTriggerOp: trigOp,
        nQueryLoop,
        prepFlags,
        oldmask: 0,
        newmask: 0,
        ..Default::default()
    });
    let iSub = db.parses.len() - 1;
    let sNC = db.arena.new_nc(NameContext::default());
    let mut iEndTrigger = 0; /* Label to jump to if WHEN is false */

    if sqlite3GetVdbe(db) {
        if !zTrigName.is_empty() {
            let z = mprintf!(db, "-- TRIGGER %s", &zTrigName);
            sqlite3VdbeChangeP4(db.v(), -1, P4::Z(z), P4_DYNAMIC as i32);
        }

        /* If one was specified, code the WHEN clause. If it evaluates to false
         ** (or NULL) the sub-vdbe is immediately halted by jumping to the
         ** OP_Halt inserted at the end of the program.  */
        if trigWhen.is_some() {
            let w = db.tree(&trigWhen);
            let pWhen = sqlite3ExprDup(db, w, 0);
            if db.mallocFailed == 0 && SQLITE_OK == sqlite3ResolveExprNames(db, sNC, pWhen) {
                iEndTrigger = sqlite3VdbeMakeLabel(db);
                sqlite3ExprIfFalse(db, pWhen, iEndTrigger, SQLITE_JUMPIFNULL as i32);
            }
            sqlite3ExprDelete(db, pWhen);
        }

        /* Code the trigger program into the sub-vdbe. */
        let steps = db.tree(&trigSteps);
        codeTriggerProgram(db, steps, orconf);

        /* Insert an OP_Halt at the end of the sub-program. */
        if iEndTrigger != 0 {
            sqlite3VdbeResolveLabel(db.v(), iEndTrigger);
        }
        sqlite3VdbeAddOp0(db.v(), OP_Halt);
        transferParseError(db, iOuter, iSub);

        let mut v = db.parses[iSub].pVdbe.take().unwrap();
        if db.parses[iOuter].nErr == 0 {
            debug_assert!(db.mallocFailed == 0);
            let mut nMaxArg = db.parses[iTop].nMaxArg;
            let aOp = sqlite3VdbeTakeOpArray(&mut v, &mut nMaxArg);
            db.parses[iTop].nMaxArg = nMaxArg;
            pProgram.borrow_mut().aOp = aOp;
        }
        {
            let sub = &db.parses[iSub];
            let mut p = pProgram.borrow_mut();
            p.nMem = sub.nMem;
            p.nCsr = sub.nTab;
            p.token = Rc::as_ptr(pTrigger) as *const u8 as usize;
        }
        let (oldmask, newmask) = (db.parses[iSub].oldmask, db.parses[iSub].newmask);
        if let Some(prg) = db.parses[iTop]
            .pTriggerPrg
            .iter_mut()
            .find(|x| Rc::ptr_eq(&x.pProgram, &pProgram))
        {
            prg.aColmask[0] = oldmask;
            prg.aColmask[1] = newmask;
        }
        sqlite3VdbeDelete(db, &mut v);
    } else {
        transferParseError(db, iOuter, iSub);
    }

    debug_assert!(db.parses[iSub].pTriggerPrg.is_empty() && db.parses[iSub].nMaxArg == 0);
    // sqlite3ParseObjectReset(&sSubParse): pop the sub-parse.
    db.parses.truncate(iSub);
    db.parses[iTop]
        .pTriggerPrg
        .iter()
        .find(|x| Rc::ptr_eq(&x.pProgram, &pProgram))
        .cloned()
}

/// Return a pointer to a TriggerPrg object containing the sub-program for
/// trigger pTrigger with default ON CONFLICT algorithm orconf. If no such
/// TriggerPrg object exists, a new object is allocated and populated before
/// being returned.
fn getRowTrigger(
    db: &mut Sqlite3,
    pTrigger: &TriggerRef,
    pTab: &TableRef,
    orconf: i32,
) -> Option<TriggerPrg> {
    let iRoot = db.ppr().iToplevel;

    /* It may be that this trigger has already been coded (or is in the
     ** process of being coded). If this is the case, then an entry with
     ** a matching TriggerPrg.pTrigger field will be present somewhere
     ** in the Parse.pTriggerPrg list. Search for such an entry.  */
    let found = db.parses[iRoot]
        .pTriggerPrg
        .iter()
        .find(|p| Rc::ptr_eq(&p.pTrigger, pTrigger) && p.orconf == orconf)
        .cloned();

    /* If an existing TriggerPrg could not be located, create a new one. */
    match found {
        Some(p) => Some(p),
        None => {
            let p = codeRowTrigger(db, pTrigger, pTab, orconf);
            db.errByteOffset = -1;
            p
        }
    }
}

/// C: `void sqlite3CodeRowTriggerDirect( Parse *pParse, Trigger *p, Table *pTab, int reg, int orconf, int ignoreJump )`
///
/// Generate code for the trigger program associated with trigger p on
/// table pTab. The reg, orconf and ignoreJump parameters passed to this
/// function are the same as those described in the header function for
/// sqlite3CodeRowTrigger()
pub fn sqlite3CodeRowTriggerDirect(
    db: &mut Sqlite3,
    p: &TriggerRef,
    pTab: &TableRef,
    reg: i32,
    orconf: i32,
    ignoreJump: i32,
) {
    sqlite3GetVdbe(db); /* Main VM */
    let pPrg = getRowTrigger(db, p, pTab, orconf);
    debug_assert!(pPrg.is_some() || db.ppr().nErr != 0);

    /* Code the OP_Program opcode in the parent VDBE. P4 of the OP_Program
     ** is a pointer to the sub-vdbe containing the trigger program.  */
    if let Some(pPrg) = pPrg {
        // C: p->zName!=0 (FK action triggers have no name).
        let bRecursive = !p.borrow().zName.is_empty() && 0 == (db.flags & SQLITE_RecTriggers);
        db.pp().nMem += 1;
        let m = db.ppr().nMem;
        sqlite3VdbeAddOp4(
            db.v(),
            OP_Program,
            reg,
            ignoreJump,
            m,
            P4::Program(pPrg.pProgram.clone()),
            P4_SUBPROGRAM as i32,
        );

        /* Set the P5 operand of the OP_Program instruction to non-zero if
         ** recursive invocation of this trigger program is disallowed. Recursive
         ** invocation is disallowed if (a) the sub-program is really a trigger,
         ** not a foreign key action, and (b) the flag to enable recursive triggers
         ** is clear.  */
        sqlite3VdbeChangeP5(db.v(), bRecursive as u16);
    }
}

/// C: `void sqlite3CodeRowTrigger( Parse *pParse, Trigger *pTrigger, int op, ExprList *pChanges, int tr_tm, Table *pTab, int reg, int orconf, int ignoreJump )`
///
/// This is called to code the required FOR EACH ROW triggers for an operation
/// on table pTab. The operation to code triggers for (INSERT, UPDATE or DELETE)
/// is given by the op parameter. The tr_tm parameter determines whether the
/// BEFORE or AFTER triggers are coded. If the operation is an UPDATE, then
/// parameter pChanges is passed the list of columns being modified.
///
/// If there are no triggers that fire at the specified time for the specified
/// operation on pTab, this function is a no-op.
///
/// The reg argument is the address of the first in an array of registers
/// that contain the values substituted for the new.* and old.* references
/// in the trigger program. If N is the number of columns in table pTab
/// (a copy of pTab->nCol), then registers are populated as follows:
///
///   Register       Contains
///   ------------------------------------------------------
///   reg+0          OLD.rowid
///   reg+1          OLD.* value of left-most column of pTab
///   ...            ...
///   reg+N          OLD.* value of right-most column of pTab
///   reg+N+1        NEW.rowid
///   reg+N+2        NEW.* value of left-most column of pTab
///   ...            ...
///   reg+N+N+1      NEW.* value of right-most column of pTab
///
/// For ON DELETE triggers, the registers containing the NEW.* values will
/// never be accessed by the trigger program, so they are not allocated or
/// populated by the caller (there is no data to populate them with anyway).
/// Similarly, for ON INSERT triggers the values stored in the OLD.* registers
/// are never accessed, and so are not allocated by the caller. So, for an
/// ON INSERT trigger, the value passed to this function as parameter reg
/// is not a readable register, although registers (reg+N) through
/// (reg+N+N+1) are.
///
/// Parameter orconf is the default conflict resolution algorithm for the
/// trigger program to use (REPLACE, IGNORE etc.). Parameter ignoreJump
/// is the instruction that control should jump to if a trigger program
/// raises an IGNORE exception.
///
/// Rust: `pTrigger` is the trigger list.
pub fn sqlite3CodeRowTrigger(
    db: &mut Sqlite3,
    pTrigger: &[TriggerRef],
    op: i32,
    pChanges: ExprListId,
    tr_tm: i32,
    pTab: &TableRef,
    reg: i32,
    orconf: i32,
    ignoreJump: i32,
) {
    debug_assert!(op == TK_UPDATE as i32 || op == TK_INSERT as i32 || op == TK_DELETE as i32);
    debug_assert!(tr_tm == TRIGGER_BEFORE as i32 || tr_tm == TRIGGER_AFTER as i32);
    debug_assert!((op == TK_UPDATE as i32) == pChanges.nn());

    for p in pTrigger.iter() {
        let (pop, ptm, bRet, cols) = {
            let t = p.borrow();
            /* Sanity checking:  The schema for the trigger and for the table are
             ** always defined.  The trigger must be in the same schema as the table
             ** or else it must be a TEMP trigger. */
            debug_assert!(t.pSchema == t.pTabSchema || t.pSchema == db.aDb[1].pSchema.id);
            (
                t.op as i32,
                t.tr_tm as i32,
                t.bReturning,
                t.pColumns.clone(),
            )
        };

        /* Determine whether we should code this trigger.  One of two choices:
         **   1. The trigger is an exact match to the current DML statement
         **   2. This is a RETURNING trigger for INSERT but we are currently
         **      doing the UPDATE part of an UPSERT.
         */
        if (pop == op || (bRet != 0 && pop == TK_INSERT as i32 && op == TK_UPDATE as i32))
            && ptm == tr_tm
            && checkColumnOverlap(db, &cols, pChanges)
        {
            if bRet == 0 {
                sqlite3CodeRowTriggerDirect(db, p, pTab, reg, orconf, ignoreJump);
            } else if sqlite3IsToplevel(db) {
                codeReturningTrigger(db, p, pTab, reg);
            }
        }
    }
}

/// C: `u32 sqlite3TriggerColmask( Parse *pParse, Trigger *pTrigger, ExprList *pChanges, int isNew, int tr_tm, Table *pTab, int orconf )`
///
/// Triggers may access values stored in the old.* or new.* pseudo-table.
/// This function returns a 32-bit bitmask indicating which columns of the
/// old.* or new.* tables actually are used by triggers. This information
/// may be used by the caller, for example, to avoid having to load the entire
/// old.* record into memory when executing an UPDATE or DELETE command.
///
/// Bit 0 of the returned mask is set if the left-most column of the
/// table may be accessed using an [old|new].<col> reference. Bit 1 is set if
/// the second leftmost column value is required, and so on. If there
/// are more than 32 columns in the table, and at least one of the columns
/// with an index greater than 32 may be accessed, 0xffffffff is returned.
///
/// It is not possible to determine if the old.rowid or new.rowid column is
/// accessed by triggers. The caller must always assume that it is.
///
/// Parameter isNew must be either 1 or 0. If it is 0, then the mask returned
/// applies to the old.* table. If 1, the new.* table.
///
/// Parameter tr_tm must be a mask with one or both of the TRIGGER_BEFORE
/// and TRIGGER_AFTER bits set. Values accessed by BEFORE triggers are only
/// included in the returned mask if the TRIGGER_BEFORE bit is set in the
/// tr_tm parameter. Similarly, values accessed by AFTER triggers are only
/// included in the returned mask if the TRIGGER_AFTER bit is set in tr_tm.
///
/// Rust: `pTrigger` is the trigger list.
pub fn sqlite3TriggerColmask(
    db: &mut Sqlite3,
    pTrigger: &[TriggerRef],
    pChanges: ExprListId,
    isNew: i32,
    tr_tm: i32,
    pTab: &TableRef,
    orconf: i32,
) -> u32 {
    let op = if pChanges.nn() {
        TK_UPDATE as i32
    } else {
        TK_DELETE as i32
    };
    let mut mask: u32 = 0;

    debug_assert!(isNew == 1 || isNew == 0);
    if pTab.borrow().IsView() {
        return 0xffffffff;
    }
    for p in pTrigger.iter() {
        let (pop, ptm, bRet, cols) = {
            let t = p.borrow();
            (
                t.op as i32,
                t.tr_tm as i32,
                t.bReturning,
                t.pColumns.clone(),
            )
        };
        if pop == op && (tr_tm & ptm) != 0 && checkColumnOverlap(db, &cols, pChanges) {
            if bRet != 0 {
                mask = 0xffffffff;
            } else if let Some(pPrg) = getRowTrigger(db, p, pTab, orconf) {
                mask |= pPrg.aColmask[isNew as usize];
            }
        }
    }

    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_dup_trims_and_flattens_whitespace() {
        let sql: &[u8] = b"  INSERT INTO t\n\tVALUES(1);  END";
        /* the span stops before ';' (offset 26) */
        assert_eq!(
            spanDupText(sql, 0, 26),
            b"INSERT INTO t  VALUES(1)".to_vec()
        );
    }
}
