//! Port of fkey.c: foreign key constraint checking and actions.
//!
//! The FKey lists of the C code (pNextFrom: Table.u.tab.pFKey, pNextTo:
//! Schema.fkeyHash) are Vecs in list order.  The synthesized action triggers
//! (fkActionTrigger) are cached in FKey.apTrigger with *owned* parse trees,
//! since they outlive the parse that created them.

use crate::auth::sqlite3AuthReadCol;
use crate::build::*;
use crate::delete::sqlite3DeleteFrom;
use crate::expr::*;
use crate::global::sqlite3StrBINARY;
use crate::insert::{sqlite3IndexAffinityStr, sqlite3OpenTable};
use crate::int::*;
use crate::resolve::sqlite3ResolveExprNames;
use crate::select::{sqlite3GetVdbe, sqlite3SelectDelete, sqlite3SelectNew};
use crate::trigger::{sqlite3CodeRowTriggerDirect, sqlite3IsToplevel};
use crate::util::{sqlite3_stricmp, sqlite3StrICmp};
use crate::vdbeaux::*;
use crate::where_::{sqlite3WhereBegin, sqlite3WhereEnd};
use std::cell::RefCell;
use std::rc::Rc;

/*
** Deferred and Immediate FKs
** --------------------------
**
** Foreign keys in SQLite come in two flavours: deferred and immediate.
** If an immediate foreign key constraint is violated,
** SQLITE_CONSTRAINT_FOREIGNKEY is returned and the current
** statement transaction rolled back. If a
** deferred foreign key constraint is violated, no action is taken
** immediately. However if the application attempts to commit the
** transaction before fixing the constraint violation, the attempt fails.
**
** Deferred constraints are implemented using a simple counter associated
** with the database handle. The counter is set to zero each time a
** database transaction is opened. Each time a statement is executed
** that causes a foreign key violation, the counter is incremented. Each
** time a statement is executed that removes an existing violation from
** the database, the counter is decremented. When the transaction is
** committed, the commit fails if the current value of the counter is
** greater than zero. This scheme has two big drawbacks:
**
**   * When a commit fails due to a deferred foreign key constraint,
**     there is no way to tell which foreign constraint is not satisfied,
**     or which row it is not satisfied for.
**
**   * If the database contains foreign key violations when the
**     transaction is opened, this may cause the mechanism to malfunction.
**
** Despite these problems, this approach is adopted as it seems simpler
** than the alternatives.
**
** INSERT operations:
**
**   I.1) For each FK for which the table is the child table, search
**        the parent table for a match. If none is found increment the
**        constraint counter.
**
**   I.2) For each FK for which the table is the parent table,
**        search the child table for rows that correspond to the new
**        row in the parent table. Decrement the counter for each row
**        found (as the constraint is now satisfied).
**
** DELETE operations:
**
**   D.1) For each FK for which the table is the child table,
**        search the parent table for a row that corresponds to the
**        deleted row in the child table. If such a row is not found,
**        decrement the counter.
**
**   D.2) For each FK for which the table is the parent table, search
**        the child table for rows that correspond to the deleted row
**        in the parent table. For each found increment the counter.
**
** UPDATE operations:
**
**   An UPDATE command requires that all 4 steps above are taken, but only
**   for FK constraints for which the affected columns are actually
**   modified (values must be compared at runtime).
**
** Note that I.1 and D.1 are very similar operations, as are I.2 and D.2.
** This simplifies the implementation a bit.
**
** For the purposes of immediate FK constraints, the OR REPLACE conflict
** resolution is considered to delete rows before the new row is inserted.
** If a delete caused by OR REPLACE violates an FK constraint, an exception
** is thrown, even if the FK constraint would be satisfied after the new
** row is inserted.
**
** Immediate constraints are usually handled similarly. The only difference
** is that the counter used is stored as part of each individual statement
** object (struct Vdbe). If, after the statement has run, its immediate
** constraint counter is greater than zero,
** it returns SQLITE_CONSTRAINT_FOREIGNKEY
** and the statement transaction is rolled back. An exception is an INSERT
** statement that inserts a single row only (no triggers). In this case,
** instead of using a counter, an exception is thrown immediately if the
** INSERT violates a foreign key constraint. This is necessary as such
** an INSERT does not open a statement transaction.
**
** TODO: How should dropping a table be handled? How should renaming a
** table be handled?
**
**
** Query API Notes
** ---------------
**
** Before coding an UPDATE or DELETE row operation, the code-generator
** for those two operations needs to know whether or not the operation
** requires any FK processing and, if so, which columns of the original
** row are required by the FK processing VDBE code (i.e. if FKs were
** implemented using triggers, which of the old.* columns would be
** accessed). No information is required by the code-generator before
** coding an INSERT operation. The functions used by the UPDATE/DELETE
** generation code to query for this information are:
**
**   sqlite3FkRequired() - Test to see if FK processing is required.
**   sqlite3FkOldmask()  - Query for the set of required old.* columns.
**
**
** Externally accessible module functions
** --------------------------------------
**
**   sqlite3FkCheck()    - Check for foreign key violations.
**   sqlite3FkActions()  - Code triggers for ON UPDATE/ON DELETE actions.
**   sqlite3FkDelete()   - Delete an FKey structure.
*/

/*
** VDBE Calling Convention
** -----------------------
**
** Example:
**
**   For the following INSERT statement:
**
**     CREATE TABLE t1(a, b INTEGER PRIMARY KEY, c);
**     INSERT INTO t1 VALUES(1, 2, 3.1);
**
**   Register (x):        2    (type integer)
**   Register (x+1):      1    (type integer)
**   Register (x+2):      NULL (type NULL)
**   Register (x+3):      3.1  (type real)
*/

#[inline]
fn fk_from(pFKey: &FKeyRef) -> TableRef {
    pFKey
        .borrow()
        .pFrom
        .upgrade()
        .expect("FKey without child table")
}

/// C: `int sqlite3FkLocateIndex( Parse *pParse, Table *pParent, FKey *pFKey, Index **ppIdx, int **paiCol )`
///
/// A foreign key constraint requires that the key columns in the parent
/// table are collectively subject to a UNIQUE or PRIMARY KEY constraint.
/// Given that pParent is the parent table for foreign key constraint pFKey,
/// search the schema for a unique index on the parent key columns.
///
/// If successful, zero is returned. If the parent key is an INTEGER PRIMARY
/// KEY column, then output variable *ppIdx is set to NULL. Otherwise, *ppIdx
/// is set to point to the unique index.
///
/// If the parent key consists of a single column (the foreign key constraint
/// is not a composite foreign key), output variable *paiCol is set to NULL.
/// Otherwise, it is set to point to an allocated array of size N, where
/// N is the number of columns in the parent key. The first element of the
/// array is the index of the child table column that is mapped by the FK
/// constraint to the parent table column stored in the left-most column
/// of index *ppIdx. The second element of the array is the index of the
/// child table column that corresponds to the second left-most column of
/// *ppIdx, and so on.
///
/// If the required index cannot be found, either because:
///
///   1) The named parent key columns do not exist, or
///
///   2) The named parent key columns do exist, but are not subject to a
///      UNIQUE or PRIMARY KEY constraint, or
///
///   3) No parent key columns were provided explicitly as part of the
///      foreign key definition, and the parent table does not have a
///      PRIMARY KEY, or
///
///   4) No parent key columns were provided explicitly as part of the
///      foreign key definition, and the PRIMARY KEY of the parent table
///      consists of a different number of columns to the child key in
///      the child table.
///
/// then non-zero is returned, and a "foreign key mismatch" error loaded
/// into pParse. If an OOM error occurs, non-zero is returned and the
/// pParse->db->mallocFailed flag is set.
///
/// Rust: `paiCol` is None for the C NULL pointer; when given, it receives
/// Some(array) for a composite key (C non-NULL *paiCol) and stays None
/// otherwise.
pub fn sqlite3FkLocateIndex(
    db: &mut Sqlite3,
    pParent: &TableRef,
    pFKey: &FKeyRef,
    ppIdx: &mut Option<IndexRef>,
    paiCol: Option<&mut Option<Vec<i32>>>,
) -> i32 {
    let (nCol, zKey, fkCols) = {
        let f = pFKey.borrow();
        (
            f.nCol,
            f.aCol[0].zCol.clone(),
            f.aCol
                .iter()
                .map(|c| (c.iFrom, c.zCol.clone()))
                .collect::<Vec<_>>(),
        )
    };
    let mut aiCol: Option<Vec<i32>> = None;
    let mut paiCol = paiCol;

    /* The caller is responsible for zeroing output parameters. */
    debug_assert!(ppIdx.is_none());

    /* If this is a non-composite (single column) foreign key, check if it
     ** maps to the INTEGER PRIMARY KEY of table pParent. If so, leave *ppIdx
     ** and *paiCol set to zero and return early.
     **
     ** Otherwise, for a composite foreign key (more than one column), allocate
     ** space for the aiCol array (returned via output parameter *paiCol).
     ** Non-composite foreign keys do not require the aiCol array.
     */
    if nCol == 1 {
        /* The FK maps to the IPK if any of the following are true:
         **
         **   1) There is an INTEGER PRIMARY KEY column and the FK is implicitly
         **      mapped to the primary key of table pParent, or
         **   2) The FK is explicitly mapped to a column declared as INTEGER
         **      PRIMARY KEY.
         */
        let p = pParent.borrow();
        if p.iPKey >= 0 {
            match &zKey {
                None => return 0,
                Some(k) => {
                    if sqlite3StrICmp(Some(&p.aCol[p.iPKey as usize].zCnName), Some(k)) == 0 {
                        return 0;
                    }
                }
            }
        }
    } else if paiCol.is_some() {
        debug_assert!(nCol > 1);
        aiCol = Some(vec![0; nCol as usize]);
    }

    let aIdx: Vec<IndexRef> = pParent.borrow().pIndex.clone();
    let mut found: Option<IndexRef> = None;
    for pIdx in aIdx.iter() {
        let x = pIdx.borrow();
        if x.nKeyCol as i32 == nCol && x.IsUniqueIndex() && x.pPartIdxWhere.is_none() {
            /* pIdx is a UNIQUE index (or a PRIMARY KEY) and has the right number
             ** of columns. If each indexed column corresponds to a foreign key
             ** column of pFKey, then this index is a winner.  */

            if zKey.is_none() {
                /* If zKey is NULL, then this foreign key is implicitly mapped to
                 ** the PRIMARY KEY of table pParent. The PRIMARY KEY index may be
                 ** identified by the test.  */
                if x.IsPrimaryKeyIndex() {
                    if let Some(a) = aiCol.as_mut() {
                        for i in 0..nCol as usize {
                            a[i] = fkCols[i].0;
                        }
                    }
                    found = Some(pIdx.clone());
                    break;
                }
            } else {
                /* If zKey is non-NULL, then this foreign key was declared to
                 ** map to an explicit list of columns in table pParent. Check if this
                 ** index matches those columns. Also, check that the index uses
                 ** the default collation sequences for each column. */
                let p = pParent.borrow();
                let mut i = 0;
                while i < nCol as usize {
                    let iCol = x.aiColumn[i]; /* Index of column in parent tbl */
                    if iCol < 0 {
                        break; /* No foreign keys against expression indexes */
                    }

                    /* If the index uses a collation sequence that is different from
                     ** the default collation sequence for the column, this index is
                     ** unusable. Bail out early in this case.  */
                    let mut col = p.aCol[iCol as usize].clone();
                    let zDfltColl =
                        sqlite3ColumnColl(&mut col).unwrap_or_else(|| sqlite3StrBINARY.to_vec());
                    if sqlite3StrICmp(Some(&x.azColl[i]), Some(&zDfltColl)) != 0 {
                        break;
                    }

                    let zIdxCol = &p.aCol[iCol as usize].zCnName;
                    let mut j = 0;
                    while j < nCol as usize {
                        if sqlite3StrICmp(fkCols[j].1.as_deref(), Some(zIdxCol)) == 0 {
                            if let Some(a) = aiCol.as_mut() {
                                a[i] = fkCols[j].0;
                            }
                            break;
                        }
                        j += 1;
                    }
                    if j == nCol as usize {
                        break;
                    }
                    i += 1;
                }
                if i == nCol as usize {
                    found = Some(pIdx.clone()); /* pIdx is usable */
                    break;
                }
            }
        }
    }

    match found {
        None => {
            if db.ppr().disableTriggers == 0 {
                let zFrom = fk_from(pFKey).borrow().zName.clone();
                let zTo = pFKey.borrow().zTo.clone();
                errmsg!(
                    db,
                    "foreign key mismatch - \"%w\" referencing \"%w\"",
                    &zFrom,
                    &zTo
                );
            }
            1
        }
        Some(pIdx) => {
            *ppIdx = Some(pIdx);
            if let Some(p) = paiCol.as_mut() {
                **p = aiCol;
            }
            0
        }
    }
}

/// This function is called when a row is inserted into or deleted from the
/// child table of foreign key constraint pFKey. If an SQL UPDATE is executed
/// on the child table of pFKey, this function is invoked twice for each row
/// affected - once to "delete" the old row, and then again to "insert" the
/// new row.
///
/// Each time it is called, this function generates VDBE code to locate the
/// row in the parent table that corresponds to the row being inserted into
/// or deleted from the child table. If the parent row can be found, no
/// special action is taken. Otherwise, if the parent row can *not* be
/// found in the parent table:
///
///   Operation | FK type   | Action taken
///   --------------------------------------------------------------------------
///   INSERT      immediate   Increment the "immediate constraint counter".
///
///   DELETE      immediate   Decrement the "immediate constraint counter".
///
///   INSERT      deferred    Increment the "deferred constraint counter".
///
///   DELETE      deferred    Decrement the "deferred constraint counter".
///
/// These operations are identified in the comment at the top of this file
/// (fkey.c) as "I.1" and "D.1".
fn fkLookupParent(
    db: &mut Sqlite3,
    iDb: i32,
    pTab: &TableRef,
    pIdx: Option<&IndexRef>,
    pFKey: &FKeyRef,
    aiCol: &[i32],
    regData: i32,
    nIncr: i32,
    isIgnore: i32,
) {
    sqlite3GetVdbe(db);
    let iCur = db.ppr().nTab - 1; /* Cursor number to use */
    let iOk = sqlite3VdbeMakeLabel(db); /* jump here if parent key found */
    let (nCol, isDeferred) = {
        let f = pFKey.borrow();
        (f.nCol, f.isDeferred)
    };
    let pFrom = fk_from(pFKey);

    /* If nIncr is less than zero, then check at runtime if there are any
     ** outstanding constraints to resolve. If there are not, there is no need
     ** to check if deleting this row resolves any outstanding violations.
     **
     ** Check if any of the key columns in the child table row are NULL. If
     ** any are, then the constraint is considered satisfied. No need to
     ** search for a matching row in the parent table.  */
    if nIncr < 0 {
        sqlite3VdbeAddOp2(db.v(), OP_FkIfZero, isDeferred as i32, iOk);
    }
    for i in 0..nCol as usize {
        let iReg = sqlite3TableColumnToStorage(&pFrom, aiCol[i] as i16) as i32 + regData + 1;
        sqlite3VdbeAddOp2(db.v(), OP_IsNull, iReg, iOk);
    }

    if isIgnore == 0 {
        match pIdx {
            None => {
                /* If pIdx is NULL, then the parent key is the INTEGER PRIMARY KEY
                 ** column of the parent table (table pTab).  */
                let regTemp = sqlite3GetTempReg(db);

                /* Invoke MustBeInt to coerce the child key value to an integer (i.e.
                 ** apply the affinity of the parent key). If this fails, then there
                 ** is no matching parent key. Before using MustBeInt, make a copy of
                 ** the value. Otherwise, the value inserted into the child key column
                 ** will have INTEGER affinity applied to it, which may not be correct.  */
                let r = sqlite3TableColumnToStorage(&pFrom, aiCol[0] as i16) as i32 + 1 + regData;
                sqlite3VdbeAddOp2(db.v(), OP_SCopy, r, regTemp);
                let iMustBeInt = sqlite3VdbeAddOp2(db.v(), OP_MustBeInt, regTemp, 0);

                /* If the parent table is the same as the child table, and we are about
                 ** to increment the constraint-counter (i.e. this is an INSERT operation),
                 ** then check if the row being inserted matches itself. If so, do not
                 ** increment the constraint-counter.  */
                if Rc::ptr_eq(pTab, &pFrom) && nIncr == 1 {
                    sqlite3VdbeAddOp3(db.v(), OP_Eq, regData, iOk, regTemp);
                    sqlite3VdbeChangeP5(db.v(), SQLITE_NOTNULL);
                }

                sqlite3OpenTable(db, iCur, iDb, pTab, OP_OpenRead as i32);
                sqlite3VdbeAddOp3(db.v(), OP_NotExists, iCur, 0, regTemp);
                sqlite3VdbeGoto(db.v(), iOk);
                let a = sqlite3VdbeCurrentAddr(db.v()) - 2;
                sqlite3VdbeJumpHere(db.v(), a);
                sqlite3VdbeJumpHere(db.v(), iMustBeInt);
                sqlite3ReleaseTempReg(db, regTemp);
            }
            Some(pIdx) => {
                let nCol = nCol;
                let regTemp = sqlite3GetTempRange(db, nCol);

                let itnum = pIdx.borrow().tnum;
                sqlite3VdbeAddOp3(db.v(), OP_OpenRead, iCur, itnum as i32, iDb);
                sqlite3VdbeSetP4KeyInfo(db, pIdx);
                for i in 0..nCol {
                    let r = sqlite3TableColumnToStorage(&pFrom, aiCol[i as usize] as i16) as i32
                        + 1
                        + regData;
                    sqlite3VdbeAddOp2(db.v(), OP_Copy, r, regTemp + i);
                }

                /* If the parent table is the same as the child table, and we are about
                 ** to increment the constraint-counter (i.e. this is an INSERT operation),
                 ** then check if the row being inserted matches itself. If so, do not
                 ** increment the constraint-counter.
                 **
                 ** If any of the parent-key values are NULL, then the row cannot match
                 ** itself. So set JUMPIFNULL to make sure we do the OP_Found if any
                 ** of the parent-key values are NULL (at this point it is known that
                 ** none of the child key values are).
                 */
                if Rc::ptr_eq(pTab, &pFrom) && nIncr == 1 {
                    let iJump = sqlite3VdbeCurrentAddr(db.v()) + nCol + 1;
                    let idxTab = pIdx.borrow().table();
                    let iPKey = pTab.borrow().iPKey;
                    for i in 0..nCol as usize {
                        let iChild = sqlite3TableColumnToStorage(&pFrom, aiCol[i] as i16) as i32
                            + 1
                            + regData;
                        let ac = pIdx.borrow().aiColumn[i];
                        let mut iParent = 1 + regData;
                        iParent += sqlite3TableColumnToStorage(&idxTab, ac) as i32;
                        debug_assert!(ac >= 0);
                        debug_assert!(aiCol[i] != iPKey as i32);
                        if ac == iPKey {
                            /* The parent key is a composite key that includes the IPK column */
                            iParent = regData;
                        }
                        sqlite3VdbeAddOp3(db.v(), OP_Ne, iChild, iJump, iParent);
                        sqlite3VdbeChangeP5(db.v(), SQLITE_JUMPIFNULL);
                    }
                    sqlite3VdbeGoto(db.v(), iOk);
                }

                let mut zAff = sqlite3IndexAffinityStr(db, pIdx).unwrap_or_default();
                zAff.truncate(nCol as usize);
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Affinity,
                    regTemp,
                    nCol,
                    0,
                    P4::Z(zAff),
                    P4_DYNAMIC as i32,
                );
                sqlite3VdbeAddOp4Int(db.v(), OP_Found, iCur, iOk, regTemp, nCol);
                sqlite3ReleaseTempRange(db, regTemp, nCol);
            }
        }
    }

    if isDeferred == 0
        && (db.flags & SQLITE_DeferFKs) == 0
        && sqlite3IsToplevel(db)
        && db.ppr().isMultiWrite == 0
    {
        /* Special case: If this is an INSERT statement that will insert exactly
         ** one row into the table, raise a constraint immediately instead of
         ** incrementing a counter. This is necessary as the VM code is being
         ** generated for will not open a statement transaction.  */
        debug_assert!(nIncr == 1);
        sqlite3HaltConstraint(
            db,
            SQLITE_CONSTRAINT_FOREIGNKEY,
            OE_Abort as i32,
            None,
            P4_STATIC,
            P5_ConstraintFK as u8,
        );
    } else {
        if nIncr > 0 && isDeferred == 0 {
            sqlite3MayAbort(db);
        }
        sqlite3VdbeAddOp2(db.v(), OP_FkCounter, isDeferred as i32, nIncr);
    }

    sqlite3VdbeResolveLabel(db.v(), iOk);
    sqlite3VdbeAddOp1(db.v(), OP_Close, iCur);
}

/// Return an Expr object that refers to a memory register corresponding
/// to column iCol of table pTab.
///
/// regBase is the first of an array of register that contains the data
/// for pTab.  regBase itself holds the rowid.  regBase+1 holds the first
/// column.  regBase+2 holds the second column, and so forth.
fn exprTableRegister(db: &mut Sqlite3, pTab: &TableRef, regBase: i32, iCol: i16) -> ExprId {
    let mut pExpr = sqlite3Expr(db, TK_REGISTER as i32, None);
    if pExpr.nn() {
        let iPKey = pTab.borrow().iPKey;
        if iCol >= 0 && iCol != iPKey {
            let mut col = pTab.borrow().aCol[iCol as usize].clone();
            let iTable = regBase + sqlite3TableColumnToStorage(pTab, iCol) as i32 + 1;
            {
                let e = db.exm(pExpr);
                e.iTable = iTable;
                e.affExpr = col.affinity;
            }
            let zColl = match sqlite3ColumnColl(&mut col) {
                Some(z) => z,
                None => db
                    .pDfltColl
                    .as_ref()
                    .map(|c| c.zName.clone())
                    .unwrap_or_default(),
            };
            pExpr = sqlite3ExprAddCollateString(db, pExpr, Some(&zColl));
        } else {
            let e = db.exm(pExpr);
            e.iTable = regBase;
            e.affExpr = SQLITE_AFF_INTEGER;
        }
    }
    pExpr
}

/// Return an Expr object that refers to column iCol of table pTab which
/// has cursor iCur.
fn exprTableColumn(db: &mut Sqlite3, pTab: &TableRef, iCursor: i32, iCol: i16) -> ExprId {
    let pExpr = sqlite3Expr(db, TK_COLUMN as i32, None);
    if pExpr.nn() {
        let e = db.exm(pExpr);
        debug_assert!(e.UseYTab());
        e.y.pTab = Some(pTab.clone());
        e.iTable = iCursor;
        e.iColumn = iCol;
    }
    pExpr
}

/// This function is called to generate code executed when a row is deleted
/// from the parent table of foreign key constraint pFKey and, if pFKey is
/// deferred, when a row is inserted into the same table. When generating
/// code for an SQL UPDATE operation, this function may be called twice -
/// once to "delete" the old row and once to "insert" the new row.
///
/// Parameter nIncr is passed -1 when inserting a row (as this may decrease
/// the number of FK violations in the db) or +1 when deleting one (as this
/// may increase the number of FK constraint problems).
///
/// The code generated by this function scans through the rows in the child
/// table that correspond to the parent table row being deleted or inserted.
/// For each child row found, one of the following actions is taken:
///
///   Operation | FK type   | Action taken
///   --------------------------------------------------------------------------
///   DELETE      immediate   Increment the "immediate constraint counter".
///
///   INSERT      immediate   Decrement the "immediate constraint counter".
///
///   DELETE      deferred    Increment the "deferred constraint counter".
///
///   INSERT      deferred    Decrement the "deferred constraint counter".
///
/// These operations are identified in the comment at the top of this file
/// (fkey.c) as "I.2" and "D.2".
fn fkScanChildren(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    pTab: &TableRef,
    pIdx: Option<&IndexRef>,
    pFKey: &FKeyRef,
    aiCol: Option<&[i32]>,
    regData: i32,
    nIncr: i32,
) {
    let mut pWhere = ExprId::NULL; /* WHERE clause to scan with */
    let mut iFkIfZero = 0; /* Address of OP_FkIfZero */
    sqlite3GetVdbe(db);
    let (nCol, isDeferred, iFrom0) = {
        let f = pFKey.borrow();
        (f.nCol, f.isDeferred, f.aCol[0].iFrom)
    };
    let pFrom = fk_from(pFKey);

    debug_assert!(pIdx.map_or(true, |x| Rc::ptr_eq(&x.borrow().table(), pTab)));
    debug_assert!(pIdx.map_or(true, |x| x.borrow().nKeyCol as i32 == nCol));
    debug_assert!(pIdx.is_some() || nCol == 1);
    debug_assert!(pIdx.is_some() || pTab.borrow().HasRowid());

    if nIncr < 0 {
        iFkIfZero = sqlite3VdbeAddOp2(db.v(), OP_FkIfZero, isDeferred as i32, 0);
    }

    /* Create an Expr object representing an SQL expression like:
     **
     **   <parent-key1> = <child-key1> AND <parent-key2> = <child-key2> ...
     **
     ** The collation sequence used for the comparison should be that of
     ** the parent key columns. The affinity of the parent key column should
     ** be applied to each child key value before the comparison takes place.
     */
    for i in 0..nCol as usize {
        let iCol: i16 = match pIdx {
            Some(x) => x.borrow().aiColumn[i],
            None => -1,
        };
        let pLeft = exprTableRegister(db, pTab, regData, iCol); /* Value from parent table row */
        let iCol = match aiCol {
            Some(a) => a[i],
            None => iFrom0,
        };
        debug_assert!(iCol >= 0);
        let zCol = pFrom.borrow().aCol[iCol as usize].zCnName.clone();
        let pRight = sqlite3Expr(db, TK_ID as i32, Some(&zCol)); /* Column ref to child table */
        let pEq = sqlite3PExpr(db, TK_EQ as i32, pLeft, pRight);
        pWhere = sqlite3ExprAnd(db, pWhere, pEq);
    }

    /* If the child table is the same as the parent table, then add terms
     ** to the WHERE clause that prevent this entry from being scanned.
     ** The added WHERE clause terms are like this:
     **
     **     $current_rowid!=rowid
     **     NOT( $current_a==a AND $current_b==b AND ... )
     **
     ** The first form is used for rowid tables.  The second form is used
     ** for WITHOUT ROWID tables. In the second form, the *parent* key is
     ** (a,b,...). Either the parent or primary key could be used to
     ** uniquely identify the current row, but the parent key is more convenient
     ** as the required values have already been loaded into registers
     ** by the caller.
     */
    if Rc::ptr_eq(pTab, &pFrom) && nIncr > 0 {
        let pNe;
        if pTab.borrow().HasRowid() {
            let pLeft = exprTableRegister(db, pTab, regData, -1);
            let cur = db.src(pSrc).a[0].iCursor;
            let pRight = exprTableColumn(db, pTab, cur, -1);
            pNe = sqlite3PExpr(db, TK_NE as i32, pLeft, pRight);
        } else {
            let mut pAll = ExprId::NULL;
            let pIdx = pIdx.unwrap();
            let nKeyCol = pIdx.borrow().nKeyCol as usize;
            for i in 0..nKeyCol {
                let iCol = pIdx.borrow().aiColumn[i];
                debug_assert!(iCol >= 0);
                let pLeft = exprTableRegister(db, pTab, regData, iCol);
                let zCn = pTab.borrow().aCol[iCol as usize].zCnName.clone();
                let pRight = sqlite3Expr(db, TK_ID as i32, Some(&zCn));
                let pEq = sqlite3PExpr(db, TK_IS as i32, pLeft, pRight);
                pAll = sqlite3ExprAnd(db, pAll, pEq);
            }
            pNe = sqlite3PExpr(db, TK_NOT as i32, pAll, ExprId::NULL);
        }
        pWhere = sqlite3ExprAnd(db, pWhere, pNe);
    }

    /* Resolve the references in the WHERE clause. */
    let sNameContext = db.arena.new_nc(NameContext {
        pSrcList: pSrc,
        ..Default::default()
    });
    sqlite3ResolveExprNames(db, sNameContext, pWhere);

    /* Create VDBE to loop through the entries in pSrc that match the WHERE
     ** clause. For each row found, increment either the deferred or immediate
     ** foreign key constraint counter. */
    if db.ppr().nErr == 0 {
        let pWInfo = sqlite3WhereBegin(
            db,
            pSrc,
            pWhere,
            ExprListId::NULL,
            ExprListId::NULL,
            SelectId::NULL,
            0,
            0,
        );
        sqlite3VdbeAddOp2(db.v(), OP_FkCounter, isDeferred as i32, nIncr);
        if let Some(mut w) = pWInfo {
            sqlite3WhereEnd(db, &mut w);
        }
    }

    /* Clean up the WHERE clause constructed above. */
    sqlite3ExprDelete(db, pWhere);
    if iFkIfZero != 0 {
        sqlite3VdbeJumpHereOrPopInst(db.v(), iFkIfZero);
    }
}

/// C: `FKey * sqlite3FkReferences(Table *pTab)`
///
/// This function returns a linked list of FKey objects (connected by
/// FKey.pNextTo) holding all children of table pTab.  For example,
/// given the following schema:
///
///   CREATE TABLE t1(a PRIMARY KEY);
///   CREATE TABLE t2(b REFERENCES t1(a);
///
/// Calling this function with table "t1" as an argument returns a pointer
/// to the FKey structure representing the foreign key constraint on table
/// "t2". Calling this function with "t2" as the argument would return a
/// NULL pointer (as there are no FK constraints for which t2 is the parent
/// table).
///
/// Rust: takes `db` (to reach the table's Schema) and returns the list as
/// a Vec (empty for C NULL).
pub fn sqlite3FkReferences(db: &Sqlite3, pTab: &TableRef) -> Vec<FKeyRef> {
    let (sid, zName) = {
        let t = pTab.borrow();
        (t.pSchema, t.zName.clone())
    };
    let i = db.schema_index(sid);
    if i < 0 {
        return Vec::new();
    }
    db.aDb[i as usize]
        .pSchema
        .borrow()
        .fkeyHash
        .find(&zName)
        .cloned()
        .unwrap_or_default()
}

/// The second argument is a Trigger structure allocated by the
/// fkActionTrigger() routine. This function deletes the Trigger structure
/// and all of its sub-components.  (Rust: memory is reference counted;
/// dropping the cache entry is enough.)
fn fkTriggerDelete(_db: &mut Sqlite3, _p: Option<TriggerRef>) {}

/// C: `void sqlite3FkClearTriggerCache(sqlite3 *db, int iDb)`
///
/// Clear the apTrigger[] cache of CASCADE triggers for all foreign keys
/// in a particular database.  This needs to happen when the schema
/// changes.
pub fn sqlite3FkClearTriggerCache(db: &mut Sqlite3, iDb: i32) {
    let tabs: Vec<TableRef> = db.aDb[iDb as usize].pSchema.borrow().tblHash.values();
    for pTab in tabs.iter() {
        if !pTab.borrow().IsOrdinaryTable() {
            continue;
        }
        let fks: Vec<FKeyRef> = pTab.borrow().u.tab.pFKey.clone();
        for pFKey in fks.iter() {
            let (a, b) = {
                let mut f = pFKey.borrow_mut();
                (f.apTrigger[0].take(), f.apTrigger[1].take())
            };
            fkTriggerDelete(db, a);
            fkTriggerDelete(db, b);
        }
    }
}

/// C: `void sqlite3FkDropTable(Parse *pParse, SrcList *pName, Table *pTab)`
///
/// This function is called to generate code that runs when table pTab is
/// being dropped from the database. The SrcList passed as the second argument
/// to this function contains a single entry guaranteed to resolve to
/// table pTab.
///
/// Normally, no code is required. However, if either
///
///   (a) The table is the parent table of a FK constraint, or
///   (b) The table is the child table of a deferred FK constraint and it is
///       determined at runtime that there are outstanding deferred FK
///       constraint violations in the database,
///
/// then the equivalent of "DELETE FROM <tbl>" is executed before dropping
/// the table from the database. Triggers are disabled while running this
/// DELETE, but foreign key actions are not.
pub fn sqlite3FkDropTable(db: &mut Sqlite3, pName: SrcListId, pTab: &TableRef) {
    if (db.flags & SQLITE_ForeignKeys) != 0 && pTab.borrow().IsOrdinaryTable() {
        let mut iSkip = 0;
        sqlite3GetVdbe(db);

        debug_assert!(pTab.borrow().IsOrdinaryTable());
        if sqlite3FkReferences(db, pTab).is_empty() {
            /* Search for a deferred foreign key constraint for which this table
             ** is the child table. If one cannot be found, return without
             ** generating any VDBE code. If one can be found, then jump over
             ** the entire DELETE if there are no outstanding deferred constraints
             ** when this statement is run.  */
            let defer = (db.flags & SQLITE_DeferFKs) != 0;
            let found = pTab
                .borrow()
                .u
                .tab
                .pFKey
                .iter()
                .any(|p| p.borrow().isDeferred != 0 || defer);
            if !found {
                return;
            }
            iSkip = sqlite3VdbeMakeLabel(db);
            sqlite3VdbeAddOp2(db.v(), OP_FkIfZero, 1, iSkip);
        }

        db.pp().disableTriggers = 1;
        let d = sqlite3SrcListDup(db, pName, 0);
        sqlite3DeleteFrom(db, d, ExprId::NULL, ExprListId::NULL, ExprId::NULL);
        db.pp().disableTriggers = 0;

        /* If the DELETE has generated immediate foreign key constraint
         ** violations, halt the VDBE and return an error at this point, before
         ** any modifications to the schema are made. This is because statement
         ** transactions are not able to rollback schema changes.
         **
         ** If the SQLITE_DeferFKs flag is set, then this is not required, as
         ** the statement transaction will not be rolled back even if FK
         ** constraints are violated.
         */
        if (db.flags & SQLITE_DeferFKs) == 0 {
            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
            sqlite3VdbeAddOp2(db.v(), OP_FkIfZero, 0, a);
            sqlite3HaltConstraint(
                db,
                SQLITE_CONSTRAINT_FOREIGNKEY,
                OE_Abort as i32,
                None,
                P4_STATIC,
                P5_ConstraintFK as u8,
            );
        }

        if iSkip != 0 {
            sqlite3VdbeResolveLabel(db.v(), iSkip);
        }
    }
}

/// The second argument points to an FKey object representing a foreign key
/// for which pTab is the child table. An UPDATE statement against pTab
/// is currently being processed. For each column of the table that is
/// actually updated, the corresponding element in the aChange[] array
/// is zero or greater (if a column is unmodified the corresponding element
/// is set to -1). If the rowid column is modified by the UPDATE statement
/// the bChngRowid argument is non-zero.
///
/// This function returns true if any of the columns that are part of the
/// child key for FK constraint *p are modified.
fn fkChildIsModified(pTab: &TableRef, p: &FKeyRef, aChange: &[i32], bChngRowid: i32) -> bool {
    let iPKey = pTab.borrow().iPKey as i32;
    let f = p.borrow();
    for i in 0..f.nCol as usize {
        let iChildKey = f.aCol[i].iFrom;
        if aChange[iChildKey as usize] >= 0 {
            return true;
        }
        if iChildKey == iPKey && bChngRowid != 0 {
            return true;
        }
    }
    false
}

/// The second argument points to an FKey object representing a foreign key
/// for which pTab is the parent table. An UPDATE statement against pTab
/// is currently being processed. For each column of the table that is
/// actually updated, the corresponding element in the aChange[] array
/// is zero or greater (if a column is unmodified the corresponding element
/// is set to -1). If the rowid column is modified by the UPDATE statement
/// the bChngRowid argument is non-zero.
///
/// This function returns true if any of the columns that are part of the
/// parent key for FK constraint *p are modified.
fn fkParentIsModified(pTab: &TableRef, p: &FKeyRef, aChange: &[i32], bChngRowid: i32) -> bool {
    let t = pTab.borrow();
    let f = p.borrow();
    for i in 0..f.nCol as usize {
        let zKey = &f.aCol[i].zCol;
        for iKey in 0..t.nCol as usize {
            if aChange[iKey] >= 0 || (iKey as i32 == t.iPKey as i32 && bChngRowid != 0) {
                let pCol = &t.aCol[iKey];
                if let Some(zKey) = zKey {
                    if 0 == sqlite3StrICmp(Some(&pCol.zCnName), Some(zKey)) {
                        return true;
                    }
                } else if pCol.colFlags & COLFLAG_PRIMKEY != 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// Return true if the parser passed as the first argument is being
/// used to code a trigger that is really a "SET NULL" action belonging
/// to trigger pFKey.
fn isSetNullAction(db: &Sqlite3, pFKey: &FKeyRef) -> bool {
    let iTop = db.ppr().iToplevel;
    if let Some(prg) = db.parses[iTop].pTriggerPrg.first() {
        let p = &prg.pTrigger;
        let f = pFKey.borrow();
        let is = |t: &Option<TriggerRef>| t.as_ref().map_or(false, |x| Rc::ptr_eq(x, p));
        if (is(&f.apTrigger[0]) && f.aAction[0] == OE_SetNull)
            || (is(&f.apTrigger[1]) && f.aAction[1] == OE_SetNull)
        {
            debug_assert!((db.flags & SQLITE_FkNoAction) == 0);
            return true;
        }
    }
    false
}

/// C: `void sqlite3FkCheck( Parse *pParse, Table *pTab, int regOld, int regNew, int *aChange, int bChngRowid )`
///
/// This function is called when inserting, deleting or updating a row of
/// table pTab to generate VDBE code to perform foreign key constraint
/// processing for the operation.
///
/// For a DELETE operation, parameter regOld is passed the index of the
/// first register in an array of (pTab->nCol+1) registers containing the
/// rowid of the row being deleted, followed by each of the column values
/// of the row being deleted, from left to right. Parameter regNew is passed
/// zero in this case.
///
/// For an INSERT operation, regOld is passed zero and regNew is passed the
/// first register of an array of (pTab->nCol+1) registers containing the new
/// row data.
///
/// For an UPDATE operation, this function is called twice. Once before
/// the original record is deleted from the table using the calling convention
/// described for DELETE. Then again after the original record is deleted
/// but before the new record is inserted using the INSERT convention.
///
/// Rust: `aChange` is None for the C NULL pointer.
pub fn sqlite3FkCheck(
    db: &mut Sqlite3,
    pTab: &TableRef,
    regOld: i32,
    regNew: i32,
    aChange: Option<&[i32]>,
    bChngRowid: i32,
) {
    let isIgnoreErrors = db.ppr().disableTriggers;

    /* Exactly one of regOld and regNew should be non-zero. */
    debug_assert!((regOld == 0) != (regNew == 0));

    /* If foreign-keys are disabled, this function is a no-op. */
    if (db.flags & SQLITE_ForeignKeys) == 0 {
        return;
    }
    if !pTab.borrow().IsOrdinaryTable() {
        return;
    }

    let iDb = db.schema_index(pTab.borrow().pSchema);
    debug_assert!(iDb >= 0 && iDb < db.nDb());
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    let (zTabName, iPKey) = {
        let t = pTab.borrow();
        (t.zName.clone(), t.iPKey as i32)
    };

    /* Loop through all the foreign key constraints for which pTab is the
     ** child table (the table that the foreign key definition is part of).  */
    let fks: Vec<FKeyRef> = pTab.borrow().u.tab.pFKey.clone();
    for pFKey in fks.iter() {
        let mut pIdx: Option<IndexRef> = None; /* Index on key columns in pTo */
        let mut aiFree: Option<Vec<i32>> = None;
        let mut bIgnore = 0;
        let (zTo, nCol, isDeferred, iFrom0) = {
            let f = pFKey.borrow();
            (f.zTo.clone(), f.nCol, f.isDeferred, f.aCol[0].iFrom)
        };

        if let Some(a) = aChange {
            if sqlite3_stricmp(Some(&zTabName), Some(&zTo)) != 0
                && !fkChildIsModified(pTab, pFKey, a, bChngRowid)
            {
                continue;
            }
        }

        /* Find the parent table of this foreign key. Also find a unique index
         ** on the parent key columns in the parent table. If either of these
         ** schema items cannot be located, set an error in pParse and return
         ** early.  */
        let pTo = if db.ppr().disableTriggers != 0 {
            sqlite3FindTable(db, Some(&zTo), zDb.as_deref())
        } else {
            sqlite3LocateTable(db, 0, Some(&zTo), zDb.as_deref())
        };
        let failed = match &pTo {
            None => true,
            Some(t) => sqlite3FkLocateIndex(db, t, pFKey, &mut pIdx, Some(&mut aiFree)) != 0,
        };
        if failed {
            debug_assert!(isIgnoreErrors == 0 || (regOld != 0 && regNew == 0));
            if isIgnoreErrors == 0 || db.mallocFailed != 0 {
                return;
            }
            if pTo.is_none() {
                /* If isIgnoreErrors is true, then a table is being dropped. In this
                 ** case SQLite runs a "DELETE FROM xxx" on the table being dropped
                 ** before actually dropping it in order to check FK constraints.
                 ** If the parent table of an FK constraint on the current table is
                 ** missing, behave as if it is empty. i.e. decrement the relevant
                 ** FK counter for each row of the current table with non-NULL keys.
                 */
                sqlite3GetVdbe(db);
                let iJump = sqlite3VdbeCurrentAddr(db.v()) + nCol + 1;
                let pFrom = fk_from(pFKey);
                for i in 0..nCol as usize {
                    let iFromCol = pFKey.borrow().aCol[i].iFrom;
                    let iReg =
                        sqlite3TableColumnToStorage(&pFrom, iFromCol as i16) as i32 + regOld + 1;
                    sqlite3VdbeAddOp2(db.v(), OP_IsNull, iReg, iJump);
                }
                sqlite3VdbeAddOp2(db.v(), OP_FkCounter, isDeferred as i32, -1);
            }
            continue;
        }
        let pTo = pTo.unwrap();
        debug_assert!(nCol == 1 || (aiFree.is_some() && pIdx.is_some()));

        let mut aiCol: Vec<i32> = match aiFree.take() {
            Some(a) => a,
            None => vec![iFrom0],
        };
        for i in 0..nCol as usize {
            if aiCol[i] == iPKey {
                aiCol[i] = -1;
            }
            debug_assert!(pIdx.as_ref().map_or(true, |x| x.borrow().aiColumn[i] >= 0));
            /* Request permission to read the parent key columns. If the
             ** authorization callback returns SQLITE_IGNORE, behave as if any
             ** values read from the parent table are NULL. */
            if db.xAuth.is_some() {
                let (zCol, zToName) = {
                    let t = pTo.borrow();
                    let c = match &pIdx {
                        Some(x) => x.borrow().aiColumn[i] as usize,
                        None => t.iPKey as usize,
                    };
                    (t.aCol[c].zCnName.clone(), t.zName.clone())
                };
                let rcauth = sqlite3AuthReadCol(db, Some(&zToName), Some(&zCol), iDb);
                bIgnore = (rcauth == SQLITE_IGNORE) as i32;
            }
        }

        /* Take a shared-cache advisory read-lock on the parent table. Allocate
         ** a cursor to use to search the unique index on the parent key columns
         ** in the parent table.  */
        let (tnum, zToName) = {
            let t = pTo.borrow();
            (t.tnum, t.zName.clone())
        };
        sqlite3TableLock(db, iDb, tnum, 0, Some(&zToName));
        db.pp().nTab += 1;

        if regOld != 0 {
            /* A row is being removed from the child table. Search for the parent.
             ** If the parent does not exist, removing the child row resolves an
             ** outstanding foreign key constraint violation. */
            fkLookupParent(
                db,
                iDb,
                &pTo,
                pIdx.as_ref(),
                pFKey,
                &aiCol,
                regOld,
                -1,
                bIgnore,
            );
        }
        if regNew != 0 && !isSetNullAction(db, pFKey) {
            /* A row is being added to the child table. If a parent row cannot
             ** be found, adding the child row has violated the FK constraint.
             **
             ** If this operation is being performed as part of a trigger program
             ** that is actually a "SET NULL" action belonging to this very
             ** foreign key, then omit this scan altogether. As all child key
             ** values are guaranteed to be NULL, it is not possible for adding
             ** this row to cause an FK violation.  */
            fkLookupParent(
                db,
                iDb,
                &pTo,
                pIdx.as_ref(),
                pFKey,
                &aiCol,
                regNew,
                1,
                bIgnore,
            );
        }
    }

    /* Loop through all the foreign key constraints that refer to this table.
     ** (the "child" constraints) */
    let refs = sqlite3FkReferences(db, pTab);
    for pFKey in refs.iter() {
        let mut pIdx: Option<IndexRef> = None; /* Foreign key index for pFKey */
        let mut aiCol: Option<Vec<i32>> = None;

        if let Some(a) = aChange {
            if !fkParentIsModified(pTab, pFKey, a, bChngRowid) {
                continue;
            }
        }

        let isDeferred = pFKey.borrow().isDeferred;
        if isDeferred == 0
            && (db.flags & SQLITE_DeferFKs) == 0
            && sqlite3IsToplevel(db)
            && db.ppr().isMultiWrite == 0
        {
            debug_assert!(regOld == 0 && regNew != 0);
            /* Inserting a single row into a parent table cannot cause (or fix)
             ** an immediate foreign key violation. So do nothing in this case.  */
            continue;
        }

        if sqlite3FkLocateIndex(db, pTab, pFKey, &mut pIdx, Some(&mut aiCol)) != 0 {
            if isIgnoreErrors == 0 || db.mallocFailed != 0 {
                return;
            }
            continue;
        }
        debug_assert!(aiCol.is_some() || pFKey.borrow().nCol == 1);

        /* Create a SrcList structure containing the child table.  We need the
         ** child table as a SrcList for sqlite3WhereBegin() */
        let pSrc = sqlite3SrcListAppend(db, SrcListId::NULL, None, None);
        if pSrc.nn() {
            let pFrom = fk_from(pFKey);
            let zFrom = pFrom.borrow().zName.clone();
            pFrom.borrow_mut().nTabRef += 1;
            let iCursor = db.ppr().nTab;
            db.pp().nTab += 1;
            {
                let it = &mut db.srcm(pSrc).a[0];
                it.pSTab = Some(pFrom.clone());
                it.zName = Some(zFrom);
                it.iCursor = iCursor;
            }

            if regNew != 0 {
                fkScanChildren(
                    db,
                    pSrc,
                    pTab,
                    pIdx.as_ref(),
                    pFKey,
                    aiCol.as_deref(),
                    regNew,
                    -1,
                );
            }
            if regOld != 0 {
                let mut eAction = pFKey.borrow().aAction[aChange.is_some() as usize];
                if (db.flags & SQLITE_FkNoAction) != 0 {
                    eAction = OE_None;
                }

                fkScanChildren(
                    db,
                    pSrc,
                    pTab,
                    pIdx.as_ref(),
                    pFKey,
                    aiCol.as_deref(),
                    regOld,
                    1,
                );
                /* If this is a deferred FK constraint, or a CASCADE or SET NULL
                 ** action applies, then any foreign key violations caused by
                 ** removing the parent key will be rectified by the action trigger.
                 ** So do not set the "may-abort" flag in this case.
                 **
                 ** Note 1: If the FK is declared "ON UPDATE CASCADE", then the
                 ** may-abort flag will eventually be set on this statement anyway
                 ** (when this function is called as part of processing the UPDATE
                 ** within the action trigger).
                 **
                 ** Note 2: At first glance it may seem like SQLite could simply omit
                 ** all OP_FkCounter related scans when either CASCADE or SET NULL
                 ** applies. The trouble starts if the CASCADE or SET NULL action
                 ** trigger causes other triggers or action rules attached to the
                 ** child table to fire. In these cases the fk constraint counters
                 ** might be set incorrectly if any OP_FkCounter related scans are
                 ** omitted.  */
                if isDeferred == 0 && eAction != OE_Cascade && eAction != OE_SetNull {
                    sqlite3MayAbort(db);
                }
            }
            db.srcm(pSrc).a[0].zName = None;
            sqlite3SrcListDelete(db, pSrc);
        }
    }
}

/// COLUMN_MASK(x)
#[inline]
fn COLUMN_MASK(x: i32) -> u32 {
    if x > 31 { 0xffffffff } else { 1u32 << x }
}

/// C: `u32 sqlite3FkOldmask( Parse *pParse, Table *pTab )`
///
/// This function is called before generating code to update or delete a
/// row contained in table pTab.
pub fn sqlite3FkOldmask(db: &mut Sqlite3, pTab: &TableRef) -> u32 {
    let mut mask: u32 = 0;
    if (db.flags & SQLITE_ForeignKeys) != 0 && pTab.borrow().IsOrdinaryTable() {
        let fks: Vec<FKeyRef> = pTab.borrow().u.tab.pFKey.clone();
        for p in fks.iter() {
            let f = p.borrow();
            for i in 0..f.nCol as usize {
                mask |= COLUMN_MASK(f.aCol[i].iFrom);
            }
        }
        let refs = sqlite3FkReferences(db, pTab);
        for p in refs.iter() {
            let mut pIdx: Option<IndexRef> = None;
            sqlite3FkLocateIndex(db, pTab, p, &mut pIdx, None);
            if let Some(pIdx) = pIdx {
                let x = pIdx.borrow();
                for i in 0..x.nKeyCol as usize {
                    debug_assert!(x.aiColumn[i] >= 0);
                    mask |= COLUMN_MASK(x.aiColumn[i] as i32);
                }
            }
        }
    }
    mask
}

/// C: `int sqlite3FkRequired( Parse *pParse, Table *pTab, int *aChange, int chngRowid )`
///
/// This function is called before generating code to update or delete a
/// row contained in table pTab. If the operation is a DELETE, then
/// parameter aChange is passed a NULL value. For an UPDATE, aChange points
/// to an array of size N, where N is the number of columns in table pTab.
/// If the i'th column is not modified by the UPDATE, then the corresponding
/// entry in the aChange[] array is set to -1. If the column is modified,
/// the value is 0 or greater. Parameter chngRowid is set to true if the
/// UPDATE statement modifies the rowid fields of the table.
///
/// If any foreign key processing will be required, this function returns
/// non-zero. If there is no foreign key related processing, this function
/// returns zero.
///
/// For an UPDATE, this function returns 2 if:
///
///   * There are any FKs for which pTab is the child and the parent table
///     and any FK processing at all is required (even of a different FK), or
///
///   * the UPDATE modifies one or more parent keys for which the action is
///     not "NO ACTION" (i.e. is CASCADE, SET DEFAULT or SET NULL).
///
/// Or, assuming some other foreign key processing is required, 1.
///
/// Rust: `aChange` is None for the C NULL pointer.
pub fn sqlite3FkRequired(
    db: &mut Sqlite3,
    pTab: &TableRef,
    aChange: Option<&[i32]>,
    chngRowid: i32,
) -> i32 {
    let mut eRet = 1; /* Value to return if bHaveFK is true */
    let mut bHaveFK = false; /* If FK processing is required */
    if (db.flags & SQLITE_ForeignKeys) != 0 && pTab.borrow().IsOrdinaryTable() {
        match aChange {
            None => {
                /* A DELETE operation. Foreign key processing is required if the
                 ** table in question is either the child or parent table for any
                 ** foreign key constraint.  */
                bHaveFK = !sqlite3FkReferences(db, pTab).is_empty()
                    || !pTab.borrow().u.tab.pFKey.is_empty();
            }
            Some(aChange) => {
                /* This is an UPDATE. Foreign key processing is only required if the
                 ** operation modifies one or more child or parent key columns. */
                let zTabName = pTab.borrow().zName.clone();

                /* Check if any child key columns are being modified. */
                let fks: Vec<FKeyRef> = pTab.borrow().u.tab.pFKey.clone();
                for p in fks.iter() {
                    if fkChildIsModified(pTab, p, aChange, chngRowid) {
                        if 0 == sqlite3_stricmp(Some(&zTabName), Some(&p.borrow().zTo)) {
                            eRet = 2;
                        }
                        bHaveFK = true;
                    }
                }

                /* Check if any parent key columns are being modified. */
                let refs = sqlite3FkReferences(db, pTab);
                for p in refs.iter() {
                    if fkParentIsModified(pTab, p, aChange, chngRowid) {
                        if (db.flags & SQLITE_FkNoAction) == 0 && p.borrow().aAction[1] != OE_None {
                            return 2;
                        }
                        bHaveFK = true;
                    }
                }
            }
        }
    }
    if bHaveFK { eRet } else { 0 }
}

/// This function is called when an UPDATE or DELETE operation is being
/// compiled on table pTab, which is the parent table of foreign-key pFKey.
/// If the current operation is an UPDATE, then the pChanges parameter is
/// passed a pointer to the list of columns being modified. If it is a
/// DELETE, pChanges is passed a NULL pointer.
///
/// It returns a pointer to a Trigger structure containing a trigger
/// equivalent to the ON UPDATE or ON DELETE action specified by pFKey.
/// If the action is "NO ACTION" then a NULL pointer is returned (these actions
/// require no special handling by the triggers sub-system, code for them is
/// created by fkScanChildren()).
///
/// For example, if pFKey is the foreign key and pTab is table "p" in
/// the following schema:
///
///   CREATE TABLE p(pk PRIMARY KEY);
///   CREATE TABLE c(ck REFERENCES p ON DELETE CASCADE);
///
/// then the returned trigger structure is equivalent to:
///
///   CREATE TRIGGER ... DELETE ON p BEGIN
///     DELETE FROM c WHERE ck = old.pk;
///   END;
///
/// The returned pointer is cached as part of the foreign key object. It
/// is eventually freed along with the rest of the foreign key object by
/// sqlite3FkDelete().
fn fkActionTrigger(
    db: &mut Sqlite3,
    pTab: &TableRef,
    pFKey: &FKeyRef,
    pChanges: ExprListId,
) -> Option<TriggerRef> {
    let iAction = pChanges.nn() as usize; /* 1 for UPDATE, 0 for DELETE */

    let mut action = pFKey.borrow().aAction[iAction];
    if (db.flags & SQLITE_FkNoAction) != 0 {
        action = OE_None;
    }
    if action == OE_Restrict && (db.flags & SQLITE_DeferFKs) != 0 {
        return None;
    }
    let mut pTrigger = pFKey.borrow().apTrigger[iAction].clone();

    if action != OE_None && pTrigger.is_none() {
        let mut pIdx: Option<IndexRef> = None; /* Parent key index for this FK */
        let mut aiCol: Option<Vec<i32>> = None; /* child table cols -> parent key cols */
        let mut pWhere = ExprId::NULL; /* WHERE clause of trigger step */
        let mut pList = ExprListId::NULL; /* Changes list if ON UPDATE CASCADE */
        let mut pSelect = SelectId::NULL; /* If RESTRICT, "SELECT RAISE(...)" */
        let mut pWhen = ExprId::NULL; /* WHEN clause for the trigger */

        if sqlite3FkLocateIndex(db, pTab, pFKey, &mut pIdx, Some(&mut aiCol)) != 0 {
            return None;
        }
        let (nCol, iFrom0) = {
            let f = pFKey.borrow();
            (f.nCol, f.aCol[0].iFrom)
        };
        debug_assert!(aiCol.is_some() || nCol == 1);
        let pFrom = fk_from(pFKey);

        for i in 0..nCol as usize {
            let tOld = Token::from_bytes(b"old"); /* Literal "old" token */
            let tNew = Token::from_bytes(b"new"); /* Literal "new" token */

            let iFromCol = match &aiCol {
                Some(a) => a[i],
                None => iFrom0,
            };
            debug_assert!(iFromCol >= 0);
            let toColName = {
                let t = pTab.borrow();
                debug_assert!(pIdx.is_some() || (t.iPKey >= 0 && t.iPKey < t.nCol));
                let c = match &pIdx {
                    Some(x) => {
                        debug_assert!(x.borrow().aiColumn[i] >= 0);
                        x.borrow().aiColumn[i] as usize
                    }
                    None => t.iPKey as usize,
                };
                t.aCol[c].zCnName.clone()
            };
            let tToCol = Token::from_bytes(&toColName);
            let fromColName = pFrom.borrow().aCol[iFromCol as usize].zCnName.clone();
            let tFromCol = Token::from_bytes(&fromColName);

            /* Create the expression "OLD.zToCol = zFromCol". It is important
             ** that the "OLD.zToCol" term is on the LHS of the = operator, so
             ** that the affinity and collation sequence associated with the
             ** parent table are used for the comparison. */
            let a = sqlite3ExprAlloc(db, TK_ID as i32, &tOld, 0);
            let b = sqlite3ExprAlloc(db, TK_ID as i32, &tToCol, 0);
            let l = sqlite3PExpr(db, TK_DOT as i32, a, b);
            let r = sqlite3ExprAlloc(db, TK_ID as i32, &tFromCol, 0);
            let pEq = sqlite3PExpr(db, TK_EQ as i32, l, r);
            pWhere = sqlite3ExprAnd(db, pWhere, pEq);

            /* For ON UPDATE, construct the next term of the WHEN clause.
             ** The final WHEN clause will be like this:
             **
             **    WHEN NOT(old.col1 IS new.col1 AND ... AND old.colN IS new.colN)
             */
            if pChanges.nn() {
                let a = sqlite3ExprAlloc(db, TK_ID as i32, &tOld, 0);
                let b = sqlite3ExprAlloc(db, TK_ID as i32, &tToCol, 0);
                let l = sqlite3PExpr(db, TK_DOT as i32, a, b);
                let a = sqlite3ExprAlloc(db, TK_ID as i32, &tNew, 0);
                let b = sqlite3ExprAlloc(db, TK_ID as i32, &tToCol, 0);
                let r = sqlite3PExpr(db, TK_DOT as i32, a, b);
                let pEq = sqlite3PExpr(db, TK_IS as i32, l, r);
                pWhen = sqlite3ExprAnd(db, pWhen, pEq);
            }

            if action != OE_Restrict && (action != OE_Cascade || pChanges.nn()) {
                let pNew;
                if action == OE_Cascade {
                    let a = sqlite3ExprAlloc(db, TK_ID as i32, &tNew, 0);
                    let b = sqlite3ExprAlloc(db, TK_ID as i32, &tToCol, 0);
                    pNew = sqlite3PExpr(db, TK_DOT as i32, a, b);
                } else if action == OE_SetDflt {
                    let cf = pFrom.borrow().aCol[iFromCol as usize].colFlags;
                    let pDflt = if cf & COLFLAG_GENERATED != 0 {
                        ExprId::NULL
                    } else {
                        sqlite3ColumnExprAuth(db, &pFrom, iFromCol as usize)
                    };
                    if pDflt.nn() {
                        pNew = sqlite3ExprDup(db, pDflt, 0);
                    } else {
                        pNew = sqlite3ExprAlloc(db, TK_NULL as i32, &Token::default(), 0);
                    }
                } else {
                    pNew = sqlite3ExprAlloc(db, TK_NULL as i32, &Token::default(), 0);
                }
                pList = sqlite3ExprListAppend(db, pList, pNew);
                sqlite3ExprListSetName(db, pList, &tFromCol, 0);
            }
        }

        let zFrom = pFrom.borrow().zName.clone(); /* Name of child table */
        let tabSchema = pTab.borrow().pSchema;

        if action == OE_Restrict {
            let pRaise = sqlite3Expr(db, TK_STRING as i32, Some(b"FOREIGN KEY constraint failed"));
            let pRaise = sqlite3PExpr(db, TK_RAISE as i32, pRaise, ExprId::NULL);
            if pRaise.nn() {
                db.exm(pRaise).affExpr = OE_Abort;
            }
            let pSrc = sqlite3SrcListAppend(db, SrcListId::NULL, None, None);
            if pSrc.nn() {
                let it = &mut db.srcm(pSrc).a[0];
                it.zName = Some(zFrom.clone());
                it.fg.fixedSchema = 1;
                it.u4.pSchema = Some(tabSchema);
            }
            let el = sqlite3ExprListAppend(db, ExprListId::NULL, pRaise);
            pSelect = sqlite3SelectNew(
                db,
                el,
                pSrc,
                pWhere,
                ExprListId::NULL,
                ExprId::NULL,
                ExprListId::NULL,
                0,
                ExprId::NULL,
            );
            pWhere = ExprId::NULL;
        }

        /* Build the trigger and its single step (DisableLookaside: n/a). */
        let stepSrc = sqlite3SrcListAppend(db, SrcListId::NULL, None, None);
        if stepSrc.nn() {
            let it = &mut db.srcm(stepSrc).a[0];
            it.zName = Some(zFrom.clone());
            it.u4.pSchema = Some(tabSchema);
            it.fg.fixedSchema = 1;
        }
        let stepWhere = sqlite3ExprDup(db, pWhere, EXPRDUP_REDUCE);
        let stepList = sqlite3ExprListDup(db, pList, EXPRDUP_REDUCE);
        let stepSelect = sqlite3SelectDup(db, pSelect, EXPRDUP_REDUCE);
        let mut trigWhen = ExprId::NULL;
        if pWhen.nn() {
            pWhen = sqlite3PExpr(db, TK_NOT as i32, pWhen, ExprId::NULL);
            trigWhen = sqlite3ExprDup(db, pWhen, EXPRDUP_REDUCE);
        }

        sqlite3ExprDelete(db, pWhere);
        sqlite3ExprDelete(db, pWhen);
        sqlite3ExprListDelete(db, pList);
        sqlite3SelectDelete(db, pSelect);
        if db.mallocFailed == 1 {
            return None;
        }

        let stepOp = match action {
            OE_Restrict => TK_SELECT,
            OE_Cascade if pChanges.is_null() => TK_DELETE,
            _ => TK_UPDATE,
        };
        let trig: TriggerRef = Rc::new(RefCell::new(Trigger {
            pSchema: tabSchema,
            pTabSchema: tabSchema,
            op: if pChanges.nn() { TK_UPDATE } else { TK_DELETE },
            ..Default::default()
        }));
        let pStep = db.arena.new_tstep(TriggerStep {
            op: stepOp,
            pTrig: Rc::downgrade(&trig),
            pSrc: stepSrc,
            pWhere: stepWhere,
            pExprList: stepList,
            pSelect: stepSelect,
            ..Default::default()
        });
        {
            /* The trigger is cached on the FKey (schema object): its parse
             ** trees must outlive this parse, so they are published (owned). */
            let mut t = trig.borrow_mut();
            t.step_list = Tree::from_id(pStep);
            t.pWhen = Tree::from_id(trigWhen);
            t.step_list.publish(&db.arena);
            t.pWhen.publish(&db.arena);
        }
        pFKey.borrow_mut().apTrigger[iAction] = Some(trig.clone());
        pTrigger = Some(trig);
    }

    pTrigger
}

/// C: `void sqlite3FkActions( Parse *pParse, Table *pTab, ExprList *pChanges, int regOld, int *aChange, int bChngRowid )`
///
/// This function is called when deleting or updating a row to implement
/// any required CASCADE, SET NULL or SET DEFAULT actions.
///
/// Rust: `aChange` is None for the C NULL pointer.
pub fn sqlite3FkActions(
    db: &mut Sqlite3,
    pTab: &TableRef,
    pChanges: ExprListId,
    regOld: i32,
    aChange: Option<&[i32]>,
    bChngRowid: i32,
) {
    /* If foreign-key support is enabled, iterate through all FKs that
     ** refer to table pTab. If there is an action associated with the FK
     ** for this operation (either update or delete), invoke the associated
     ** trigger sub-program.  */
    if (db.flags & SQLITE_ForeignKeys) != 0 {
        let refs = sqlite3FkReferences(db, pTab);
        for pFKey in refs.iter() {
            let doit = match aChange {
                None => true,
                Some(a) => fkParentIsModified(pTab, pFKey, a, bChngRowid),
            };
            if doit {
                if let Some(pAct) = fkActionTrigger(db, pTab, pFKey, pChanges) {
                    sqlite3CodeRowTriggerDirect(db, &pAct, pTab, regOld, OE_Abort as i32, 0);
                }
            }
        }
    }
}

/// C: `void sqlite3FkDelete(sqlite3 *db, Table *pTab)`
///
/// Free all memory associated with foreign key definitions attached to
/// table pTab. Remove the deleted foreign keys from the Schema.fkeyHash
/// hash table.
pub fn sqlite3FkDelete(db: &mut Sqlite3, pTab: &TableRef) {
    debug_assert!(pTab.borrow().IsOrdinaryTable());
    let (fks, sid) = {
        let t = pTab.borrow();
        (t.u.tab.pFKey.clone(), t.pSchema)
    };
    let iDb = db.schema_index(sid);
    for pFKey in fks.iter() {
        /* Remove the FK from the fkeyHash hash table. */
        if iDb >= 0 {
            let zTo = pFKey.borrow().zTo.clone();
            let h = &mut db.aDb[iDb as usize].pSchema.borrow_mut().fkeyHash;
            if let Some(list) = h.find_mut(&zTo) {
                if let Some(pos) = list.iter().position(|x| Rc::ptr_eq(x, pFKey)) {
                    list.remove(pos);
                    if list.is_empty() {
                        h.remove(&zTo);
                    } else if pos == 0 {
                        /* C re-inserts the list under the next FKey's zTo. */
                        let l = list.clone();
                        let z = l[0].borrow().zTo.clone();
                        h.insert(&z, l);
                    }
                }
            }
        }

        /* Delete any triggers created to implement actions for this FK. */
        let (a, b) = {
            let mut f = pFKey.borrow_mut();
            (f.apTrigger[0].take(), f.apTrigger[1].take())
        };
        fkTriggerDelete(db, a);
        fkTriggerDelete(db, b);
    }
}
