//! Port of insert.c: code generation for INSERT statements, plus the
//! constraint-check / record-insertion helpers shared with UPDATE.

use crate::auth::{sqlite3AuthCheck, sqlite3AuthReadCol};
use crate::build::*;
use crate::callback::sqlite3LocateCollSeq;
use crate::delete::{
    sqlite3CodeChangeCount, sqlite3GenerateRowDelete, sqlite3GenerateRowIndexDelete,
};
use crate::delete::{sqlite3IsReadOnly, sqlite3SrcListLookup};
use crate::expr::*;
use crate::fkey::{sqlite3FkCheck, sqlite3FkReferences, sqlite3FkRequired};
use crate::global::{sqlite3OpcodeProperty, sqlite3StrBINARY};
use crate::int::*;
use crate::printf::PfArg;
use crate::resolve::sqlite3ResolveExprListNames;
use crate::select::*;
use crate::trigger::{sqlite3CodeRowTrigger, sqlite3TriggersExist};
use crate::upsert::{
    sqlite3UpsertAnalyzeTarget, sqlite3UpsertDelete, sqlite3UpsertDoUpdate, sqlite3UpsertNextIsIPK,
    sqlite3UpsertOfIndex,
};
use crate::util::{sqlite3_stricmp, sqlite3FaultSim};
use crate::vdbeaux::*;
use crate::walker::{sqlite3WalkExpr, sqlite3WalkExprList};
use std::rc::Rc;

#[inline]
fn opt_rc_eq<T>(a: &Option<Rc<T>>, b: &Rc<T>) -> bool {
    match a {
        Some(x) => Rc::ptr_eq(x, b),
        None => false,
    }
}

/// C: `void sqlite3OpenTable( Parse *pParse, int iCur, int iDb, Table *pTab, int opcode )`
///
/// Generate code that will
///
///   (1) acquire a lock for table pTab then
///   (2) open pTab as cursor iCur.
///
/// If pTab is a WITHOUT ROWID table, then it is the PRIMARY KEY index
/// for that table that is actually opened.
pub fn sqlite3OpenTable(db: &mut Sqlite3, iCur: i32, iDb: i32, pTab: &TableRef, opcode: i32) {
    debug_assert!(!pTab.borrow().IsVirtual());
    debug_assert!(opcode == OP_OpenWrite as i32 || opcode == OP_OpenRead as i32);
    let (tnum, zName, hasRowid, nNVCol) = {
        let t = pTab.borrow();
        (t.tnum, t.zName.clone(), t.HasRowid(), t.nNVCol)
    };
    if db.noSharedCache == 0 {
        sqlite3TableLock(
            db,
            iDb,
            tnum,
            (opcode == OP_OpenWrite as i32) as u8,
            Some(&zName),
        );
    }
    if hasRowid {
        sqlite3VdbeAddOp4Int(db.v(), opcode as u8, iCur, tnum as i32, iDb, nNVCol as i32);
    } else {
        let pPk = sqlite3PrimaryKeyIndex(pTab).expect("WITHOUT ROWID table without PK");
        let pktnum = pPk.borrow().tnum;
        sqlite3VdbeAddOp3(db.v(), opcode as u8, iCur, pktnum as i32, iDb);
        sqlite3VdbeSetP4KeyInfo(db, &pPk);
    }
}

/// Return a pointer to the column affinity string associated with index
/// pIdx. A column affinity string has one character for each column in
/// the table, according to the affinity of the column:
///
///  Character      Column affinity
///  ------------------------------
///  'A'            BLOB
///  'B'            TEXT
///  'C'            NUMERIC
///  'D'            INTEGER
///  'F'            REAL
///
/// An extra 'D' is appended to the end of the string to cover the
/// rowid that appears as the last column in every index.
///
/// Memory for the buffer containing the column index affinity string
/// is managed along with the rest of the Index structure. It will be
/// released when sqlite3DeleteIndex() is called.
fn computeIndexAffStr(db: &mut Sqlite3, pIdx: &IndexRef) -> Option<ZStr> {
    /* The first time a column affinity string for a particular index is
     ** required, it is allocated and populated here. It is then stored as
     ** a member of the Index structure for subsequent use.
     */
    let pTab = pIdx.borrow().table();
    let nColumn = pIdx.borrow().nColumn as usize;
    let mut colExpr = ExprListId::NULL;
    let mut z: ZStr = Vec::with_capacity(nColumn);
    for n in 0..nColumn {
        let x = pIdx.borrow().aiColumn[n];
        let mut aff;
        if x >= 0 {
            aff = pTab.borrow().aCol[x as usize].affinity;
        } else if x == XN_ROWID {
            aff = SQLITE_AFF_INTEGER;
        } else {
            debug_assert!(x == XN_EXPR);
            debug_assert!(pIdx.borrow().bHasExpr != 0);
            if colExpr.is_null() {
                let t = pIdx.borrow().aColExpr.clone();
                colExpr = db.tree(&t);
            }
            let e = db.el(colExpr).a[n].pExpr;
            aff = sqlite3ExprAffinity(db, e);
        }
        if aff < SQLITE_AFF_BLOB {
            aff = SQLITE_AFF_BLOB;
        }
        if aff > SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_NUMERIC;
        }
        z.push(aff);
    }
    pIdx.borrow_mut().zColAff = Some(z.clone());
    Some(z)
}

/// C: `const char * sqlite3IndexAffinityStr(sqlite3 *db, Index *pIdx)`
/// (See computeIndexAffStr.)  The string is cached in Index::zColAff.
pub fn sqlite3IndexAffinityStr(db: &mut Sqlite3, pIdx: &IndexRef) -> Option<ZStr> {
    let z = pIdx.borrow().zColAff.clone();
    if z.is_none() {
        return computeIndexAffStr(db, pIdx);
    }
    z
}

/// The affinity string of a table, without the db (C allows db==0).
fn tableAffinityStr(pTab: &Table) -> ZStr {
    let mut z: ZStr = Vec::with_capacity(pTab.aCol.len());
    for c in pTab.aCol.iter().take(pTab.nCol.max(0) as usize) {
        if (c.colFlags & COLFLAG_VIRTUAL) == 0 {
            z.push(c.affinity);
        }
    }
    while let Some(&last) = z.last() {
        if last <= SQLITE_AFF_BLOB {
            z.pop();
        } else {
            break;
        }
    }
    z
}

/// C: `char * sqlite3TableAffinityStr(sqlite3 *db, const Table *pTab)`
///
/// Compute an affinity string for a table.  Trailing SQLITE_AFF_BLOB (and
/// NONE) affinities are omitted.
pub fn sqlite3TableAffinityStr(db: &mut Sqlite3, pTab: &TableRef) -> Option<ZStr> {
    let _ = db;
    Some(tableAffinityStr(&pTab.borrow()))
}

/// C: `void sqlite3TableAffinity(Vdbe *v, Table *pTab, int iReg)`
///
/// Make changes to the evolving bytecode to do affinity transformations
/// of values that are about to be gathered into a row for table pTab.
///
/// For ordinary (legacy, non-strict) tables:
/// -----------------------------------------
///
/// Compute the affinity string for table pTab, if it has not already been
/// computed.  As an optimization, omit trailing SQLITE_AFF_BLOB affinities.
///
/// If the affinity string is empty (because it was all SQLITE_AFF_BLOB entries
/// which were then optimized out) then this routine becomes a no-op.
///
/// Otherwise if iReg>0 then code an OP_Affinity opcode that will set the
/// affinities for register iReg and following.  Or if iReg==0,
/// then just set the P4 operand of the previous opcode (which should  be
/// an OP_MakeRecord) to the affinity string.
///
/// For STRICT tables:
/// ------------------
///
/// Generate an appropriate OP_TypeCheck opcode that will verify the
/// datatypes against the column definitions in pTab.  If iReg==0, that
/// means an OP_MakeRecord opcode has already been generated and should be
/// the last opcode generated.  The new OP_TypeCheck needs to be inserted
/// before the OP_MakeRecord.  The new OP_TypeCheck should use the same
/// register set as the OP_MakeRecord.  If iReg>0 then register iReg is
/// the first of a series of registers that will form the new record.
/// Apply the type checking to that array of registers.
pub fn sqlite3TableAffinity(v: &mut Vdbe, pTab: &TableRef, iReg: i32) {
    let (strict, nNVCol) = {
        let t = pTab.borrow();
        ((t.tabFlags & TF_Strict) != 0, t.nNVCol)
    };
    if strict {
        if iReg == 0 {
            /* Move the previous opcode (which should be OP_MakeRecord) forward
             ** by one slot and insert a new OP_TypeCheck where the current
             ** OP_MakeRecord is found */
            sqlite3VdbeAppendP4(v, P4::Tab(pTab.clone()), P4_TABLE as i32);
            let (p1, p2, p3) = {
                let pPrev = sqlite3VdbeGetLastOp(v).expect("no prior opcode");
                debug_assert!(pPrev.opcode == OP_MakeRecord);
                pPrev.opcode = OP_TypeCheck;
                let p3 = pPrev.p3;
                pPrev.p3 = 0;
                (pPrev.p1, pPrev.p2, p3)
            };
            sqlite3VdbeAddOp3(v, OP_MakeRecord, p1, p2, p3);
        } else {
            /* Insert an isolated OP_Typecheck */
            sqlite3VdbeAddOp2(v, OP_TypeCheck, iReg, nNVCol as i32);
            sqlite3VdbeAppendP4(v, P4::Tab(pTab.clone()), P4_TABLE as i32);
        }
        return;
    }
    let zColAff = {
        let cached = pTab.borrow().zColAff.clone();
        match cached {
            Some(z) => z,
            None => {
                let z = tableAffinityStr(&pTab.borrow());
                pTab.borrow_mut().zColAff = Some(z.clone());
                z
            }
        }
    };
    let i = zColAff.len() as i32;
    if i != 0 {
        if iReg != 0 {
            sqlite3VdbeAddOp4(
                v,
                OP_Affinity,
                iReg,
                i,
                0,
                P4::Z(zColAff),
                P4_DYNAMIC as i32,
            );
        } else {
            debug_assert!(sqlite3VdbeGetLastOp(v).map(|o| o.opcode) == Some(OP_MakeRecord));
            sqlite3VdbeChangeP4(v, -1, P4::Z(zColAff), P4_DYNAMIC as i32);
        }
    }
}

/// Return non-zero if the table pTab in database iDb or any of its indices
/// have been opened at any point in the VDBE program. This is used to see if
/// a statement of the form  "INSERT INTO <iDb, pTab> SELECT ..." can
/// run without using a temporary table for the results of the SELECT.
fn readsTable(db: &mut Sqlite3, iDb: i32, pTab: &TableRef) -> bool {
    sqlite3GetVdbe(db);
    let iEnd = sqlite3VdbeCurrentAddr(db.v());
    let (tnumTab, aTnum, isVirtual) = {
        let t = pTab.borrow();
        (
            t.tnum,
            t.pIndex.iter().map(|x| x.borrow().tnum).collect::<Vec<_>>(),
            t.IsVirtual(),
        )
    };
    for i in 1..iEnd {
        let (opcode, p2, p3) = {
            let pOp = sqlite3VdbeGetOp(db.v(), i);
            (pOp.opcode, pOp.p2, pOp.p3)
        };
        if opcode == OP_OpenRead && p3 == iDb {
            let tnum = p2 as Pgno;
            if tnum == tnumTab {
                return true;
            }
            if aTnum.iter().any(|&t| t == tnum) {
                return true;
            }
        }
        if isVirtual && opcode == OP_VOpen {
            // TODO(vtab): C compares pOp->p4.pVtab with sqlite3GetVTable(db,pTab).
            return true;
        }
    }
    false
}

/// This walker callback will compute the union of colFlags flags for all
/// referenced columns in a CHECK constraint or generated column expression.
fn exprColumnFlagUnion(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (op, iColumn) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iColumn)
    };
    if op == TK_COLUMN && iColumn >= 0 {
        let f = {
            let t = w.u.pTab.as_ref().unwrap().borrow();
            debug_assert!((iColumn as i32) < t.nCol as i32);
            t.aCol[iColumn as usize].colFlags
        };
        w.eCode |= f;
    }
    WRC_Continue
}

/// C: `void sqlite3ComputeGeneratedColumns( Parse *pParse, int iRegStore, Table *pTab )`
///
/// All regular columns for table pTab have been puts into registers
/// starting with iRegStore.  The registers that correspond to STORED
/// or VIRTUAL columns have not yet been initialized.  This routine goes
/// back and computes the values for those columns based on the previously
/// computed normal columns.
pub fn sqlite3ComputeGeneratedColumns(db: &mut Sqlite3, iRegStore: i32, pTab: &TableRef) {
    debug_assert!(pTab.borrow().tabFlags & TF_HasGenerated != 0);

    /* Before computing generated columns, first go through and make sure
     ** that appropriate affinity has been applied to the regular columns
     */
    sqlite3TableAffinity(db.v(), pTab, iRegStore);
    if (pTab.borrow().tabFlags & TF_HasStored) != 0 {
        let aFlags: Vec<u16> = pTab.borrow().aCol.iter().map(|c| c.colFlags).collect();
        let pOp = sqlite3VdbeGetLastOp(db.v()).expect("no last op");
        if pOp.opcode == OP_Affinity {
            /* Change the OP_Affinity argument to '@' (NONE) for all stored
             ** columns.  '@' is the no-op affinity and those columns have not
             ** yet been computed. */
            debug_assert!(pOp.p4type == P4_DYNAMIC);
            if let P4::Z(zP4) = &mut pOp.p4 {
                let mut ii = 0usize;
                let mut jj = 0usize;
                while jj < zP4.len() && zP4[jj] != 0 {
                    if aFlags[ii] & COLFLAG_VIRTUAL != 0 {
                        ii += 1;
                        continue;
                    }
                    if aFlags[ii] & COLFLAG_STORED != 0 {
                        zP4[jj] = SQLITE_AFF_NONE;
                    }
                    jj += 1;
                    ii += 1;
                }
            }
        } else if pOp.opcode == OP_TypeCheck {
            /* If an OP_TypeCheck was generated because the table is STRICT,
             ** then set the P3 operand to indicate that generated columns should
             ** not be checked */
            pOp.p3 = 1;
        }
    }

    /* Because there can be multiple generated columns that refer to one another,
     ** this is a two-pass algorithm.  On the first pass, mark all generated
     ** columns as "not available".
     */
    let nCol = pTab.borrow().nCol as usize;
    {
        let mut t = pTab.borrow_mut();
        for i in 0..nCol {
            if t.aCol[i].colFlags & COLFLAG_GENERATED != 0 {
                t.aCol[i].colFlags |= COLFLAG_NOTAVAIL;
            }
        }
    }

    /* On the second pass, compute the value of each NOT-AVAILABLE column.
     ** Companion code in the TK_COLUMN case of sqlite3ExprCodeTarget() will
     ** compute dependencies and mark remove the COLSPAN_NOTAVAIL mark, as
     ** they are needed.
     */
    db.pp().iSelfTab = -iRegStore;
    let mut pRedo: Option<usize>;
    loop {
        let mut eProgress = false;
        pRedo = None;
        for i in 0..nCol {
            if (pTab.borrow().aCol[i].colFlags & COLFLAG_NOTAVAIL) != 0 {
                pTab.borrow_mut().aCol[i].colFlags |= COLFLAG_BUSY;
                let e = sqlite3ColumnExpr(db, pTab, i);
                let eCode = {
                    let mut w = Walker::new(db);
                    w.u.pTab = Some(pTab.clone());
                    w.xExprCallback = Some(exprColumnFlagUnion);
                    w.xSelectCallback = None;
                    w.xSelectCallback2 = None;
                    w.eCode = 0;
                    sqlite3WalkExpr(&mut w, e);
                    w.eCode
                };
                pTab.borrow_mut().aCol[i].colFlags &= !COLFLAG_BUSY;
                if eCode & COLFLAG_NOTAVAIL != 0 {
                    pRedo = Some(i);
                    continue;
                }
                eProgress = true;
                debug_assert!(pTab.borrow().aCol[i].colFlags & COLFLAG_GENERATED != 0);
                let x = sqlite3TableColumnToStorage(pTab, i as i16) as i32 + iRegStore;
                sqlite3ExprCodeGeneratedColumn(db, pTab, i, x);
                pTab.borrow_mut().aCol[i].colFlags &= !COLFLAG_NOTAVAIL;
            }
        }
        if !(pRedo.is_some() && eProgress) {
            break;
        }
    }
    if let Some(r) = pRedo {
        let zName = pTab.borrow().aCol[r].zCnName.clone();
        errmsg!(db, "generated column loop on \"%s\"", &zName);
    }
    db.pp().iSelfTab = 0;
}

/// Locate or create an AutoincInfo structure associated with table pTab
/// which is in database iDb.  Return the register number for the register
/// that holds the maximum rowid.  Return zero if pTab is not an AUTOINCREMENT
/// table.  (Also return zero when doing a VACUUM since we do not want to
/// update the AUTOINCREMENT counters during a VACUUM.)
///
/// There is at most one AutoincInfo structure per table even if the
/// same table is autoincremented multiple times due to inserts within
/// triggers.  A new AutoincInfo structure is created if this is the
/// first use of table pTab.  On 2nd and subsequent uses, the original
/// AutoincInfo structure is used.
///
/// Four consecutive registers are allocated:
///
///   (1)  The name of the pTab table.
///   (2)  The maximum ROWID of pTab.
///   (3)  The rowid in sqlite_sequence of pTab
///   (4)  The original value of the max ROWID in pTab, or NULL if none
///
/// The 2nd register is the one that is returned.  That is all the
/// insert routine needs to know about.
fn autoIncBegin(db: &mut Sqlite3, iDb: i32, pTab: &TableRef) -> i32 {
    let mut memId = 0;
    if (pTab.borrow().tabFlags & TF_Autoincrement) != 0 && (db.mDbFlags & DBFLAG_Vacuum) == 0 {
        let pSeqTab = db.aDb[iDb as usize].pSchema.borrow().pSeqTab.clone();

        /* Verify that the sqlite_sequence table exists and is an ordinary
         ** rowid table with exactly two columns.
         ** Ticket d8dc2b3a58cd5dc2918a1d4acb 2018-05-23 */
        let bad = match &pSeqTab {
            None => true,
            Some(s) => {
                let s = s.borrow();
                !s.HasRowid() || s.IsVirtual() || s.nCol != 2
            }
        };
        if bad {
            db.pp().nErr += 1;
            db.pp().rc = SQLITE_CORRUPT_SEQUENCE;
            return 0;
        }

        let pToplevel = db.toplevel();
        if pToplevel.usesAinc == 0 {
            pToplevel.pAinc.clear();
        }
        let found = pToplevel
            .pAinc
            .iter()
            .find(|p| opt_rc_eq(&p.pTab, pTab))
            .map(|p| p.regCtr);
        match found {
            Some(r) => memId = r,
            None => {
                pToplevel.usesAinc = 1;
                pToplevel.nMem += 1; /* Register to hold name of table */
                pToplevel.nMem += 1;
                let regCtr = pToplevel.nMem; /* Max rowid register */
                pToplevel.nMem += 2; /* Rowid in sqlite_sequence + orig max val */
                pToplevel.pAinc.insert(
                    0,
                    AutoincInfo {
                        pTab: Some(pTab.clone()),
                        iDb,
                        regCtr,
                    },
                );
                memId = regCtr;
            }
        }
    }
    memId
}

/// C: `void sqlite3AutoincrementBegin(Parse *pParse)`
///
/// This routine generates code that will initialize all of the
/// register used by the autoincrement tracker.
pub fn sqlite3AutoincrementBegin(db: &mut Sqlite3) {
    /* This routine is never called during trigger-generation.  It is
     ** only called from the top-level */
    debug_assert!(db.ppr().pTriggerTab.is_none());
    debug_assert!(db.ppr().usesAinc != 0);
    static autoInc: [VdbeOpList; 12] = [
        /* 0  */
        VdbeOpList {
            opcode: OP_Null,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 1  */
        VdbeOpList {
            opcode: OP_Rewind,
            p1: 0,
            p2: 10,
            p3: 0,
        },
        /* 2  */
        VdbeOpList {
            opcode: OP_Column,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 3  */
        VdbeOpList {
            opcode: OP_Ne,
            p1: 0,
            p2: 9,
            p3: 0,
        },
        /* 4  */
        VdbeOpList {
            opcode: OP_Rowid,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 5  */
        VdbeOpList {
            opcode: OP_Column,
            p1: 0,
            p2: 1,
            p3: 0,
        },
        /* 6  */
        VdbeOpList {
            opcode: OP_AddImm,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 7  */
        VdbeOpList {
            opcode: OP_Copy,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 8  */
        VdbeOpList {
            opcode: OP_Goto,
            p1: 0,
            p2: 11,
            p3: 0,
        },
        /* 9  */
        VdbeOpList {
            opcode: OP_Next,
            p1: 0,
            p2: 2,
            p3: 0,
        },
        /* 10 */
        VdbeOpList {
            opcode: OP_Integer,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 11 */
        VdbeOpList {
            opcode: OP_Close,
            p1: 0,
            p2: 0,
            p3: 0,
        },
    ];
    let aAinc = db.ppr().pAinc.clone();
    for p in aAinc.iter() {
        let memId = p.regCtr;
        let pSeqTab = db.aDb[p.iDb as usize]
            .pSchema
            .borrow()
            .pSeqTab
            .clone()
            .unwrap();
        sqlite3OpenTable(db, 0, p.iDb, &pSeqTab, OP_OpenRead as i32);
        let zName = p.pTab.as_ref().unwrap().borrow().zName.clone();
        sqlite3VdbeLoadString(db.v(), memId - 1, &zName);
        let v = db.v();
        let a = sqlite3VdbeAddOpList(v, &autoInc, 0);
        sqlite3VdbeGetOp(v, a).p2 = memId;
        sqlite3VdbeGetOp(v, a).p3 = memId + 2;
        sqlite3VdbeGetOp(v, a + 2).p3 = memId;
        sqlite3VdbeGetOp(v, a + 3).p1 = memId - 1;
        sqlite3VdbeGetOp(v, a + 3).p3 = memId;
        sqlite3VdbeGetOp(v, a + 3).p5 = SQLITE_JUMPIFNULL;
        sqlite3VdbeGetOp(v, a + 4).p2 = memId + 1;
        sqlite3VdbeGetOp(v, a + 5).p3 = memId;
        sqlite3VdbeGetOp(v, a + 6).p1 = memId;
        sqlite3VdbeGetOp(v, a + 7).p2 = memId + 2;
        sqlite3VdbeGetOp(v, a + 7).p1 = memId;
        sqlite3VdbeGetOp(v, a + 10).p2 = memId;
        if db.ppr().nTab == 0 {
            db.pp().nTab = 1;
        }
    }
}

/// Update the maximum rowid for an autoincrement calculation.
///
/// This routine should be called when the regRowid register holds a
/// new rowid that is about to be inserted.  If that new rowid is
/// larger than the maximum rowid in the memId memory cell, then the
/// memory cell is updated.
fn autoIncStep(db: &mut Sqlite3, memId: i32, regRowid: i32) {
    if memId > 0 {
        sqlite3VdbeAddOp2(db.v(), OP_MemMax, memId, regRowid);
    }
}

/// This routine generates the code needed to write autoincrement
/// maximum rowid values back into the sqlite_sequence register.
/// Every statement that might do an INSERT into an autoincrement
/// table (either directly or through triggers) needs to call this
/// routine just before the "exit" code.
fn autoIncrementEnd(db: &mut Sqlite3) {
    static autoIncEnd: [VdbeOpList; 5] = [
        /* 0 */
        VdbeOpList {
            opcode: OP_NotNull,
            p1: 0,
            p2: 2,
            p3: 0,
        },
        /* 1 */
        VdbeOpList {
            opcode: OP_NewRowid,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 2 */
        VdbeOpList {
            opcode: OP_MakeRecord,
            p1: 0,
            p2: 2,
            p3: 0,
        },
        /* 3 */
        VdbeOpList {
            opcode: OP_Insert,
            p1: 0,
            p2: 0,
            p3: 0,
        },
        /* 4 */
        VdbeOpList {
            opcode: OP_Close,
            p1: 0,
            p2: 0,
            p3: 0,
        },
    ];
    debug_assert!(db.ppr().usesAinc != 0);
    let aAinc = db.ppr().pAinc.clone();
    for p in aAinc.iter() {
        let memId = p.regCtr;
        let iRec = sqlite3GetTempReg(db);
        let a = sqlite3VdbeCurrentAddr(db.v()) + 7;
        sqlite3VdbeAddOp3(db.v(), OP_Le, memId + 2, a, memId);
        let pSeqTab = db.aDb[p.iDb as usize]
            .pSchema
            .borrow()
            .pSeqTab
            .clone()
            .unwrap();
        sqlite3OpenTable(db, 0, p.iDb, &pSeqTab, OP_OpenWrite as i32);
        let v = db.v();
        let a = sqlite3VdbeAddOpList(v, &autoIncEnd, 0);
        sqlite3VdbeGetOp(v, a).p1 = memId + 1;
        sqlite3VdbeGetOp(v, a + 1).p2 = memId + 1;
        sqlite3VdbeGetOp(v, a + 2).p1 = memId - 1;
        sqlite3VdbeGetOp(v, a + 2).p3 = iRec;
        sqlite3VdbeGetOp(v, a + 3).p2 = iRec;
        sqlite3VdbeGetOp(v, a + 3).p3 = memId + 1;
        sqlite3VdbeGetOp(v, a + 3).p5 = OPFLAG_APPEND as u16;
        sqlite3ReleaseTempReg(db, iRec);
    }
}

/// C: `void sqlite3AutoincrementEnd(Parse *pParse)`
pub fn sqlite3AutoincrementEnd(db: &mut Sqlite3) {
    if db.ppr().usesAinc != 0 {
        autoIncrementEnd(db);
    }
}

/// C: `void sqlite3MultiValuesEnd(Parse *pParse, Select *pVal)`
///
/// If argument pVal is a Select object returned by an sqlite3MultiValues()
/// that was able to use the co-routine optimization, finish coding the
/// co-routine.
pub fn sqlite3MultiValuesEnd(db: &mut Sqlite3, pVal: SelectId) {
    if pVal.nn() {
        let src = db.sel(pVal).pSrc;
        if src.nn() && db.src(src).nSrc() > 0 {
            let pItem = SrcItemId(src, 0);
            let sub = {
                let it = db.item(pItem);
                debug_assert!(
                    (it.fg.isSubquery != 0 && it.u4.pSubq.is_some()) || db.ppr().nErr != 0
                );
                if it.fg.isSubquery != 0 {
                    it.u4.pSubq.as_ref().map(|s| (s.regReturn, s.addrFillSub))
                } else {
                    None
                }
            };
            if let Some((regReturn, addrFillSub)) = sub {
                sqlite3VdbeEndCoroutine(db, regReturn);
                sqlite3VdbeJumpHere(db.v(), addrFillSub - 1);
            }
        }
    }
}

/// Return true if all expressions in the expression-list passed as the
/// only argument are both constant and have no affinity.
fn exprListIsNoAffinity(db: &mut Sqlite3, pRow: ExprListId) -> bool {
    if sqlite3ExprListIsConstant(db, pRow, 0) == 0 {
        return false;
    }
    let n = db.el(pRow).nExpr();
    for ii in 0..n {
        let pExpr = db.el(pRow).a[ii as usize].pExpr;
        debug_assert!(db.ex(pExpr).op != TK_RAISE);
        debug_assert!(db.ex(pExpr).affExpr == 0);
        if 0 != sqlite3ExprAffinity(db, pExpr) {
            return false;
        }
    }
    true
}

/// C: `Select * sqlite3MultiValues(Parse *pParse, Select *pLeft, ExprList *pRow)`
///
/// This function is called by the parser for the second and subsequent
/// rows of a multi-row VALUES clause. Argument pLeft is the part of
/// the VALUES clause already parsed, argument pRow is the vector of values
/// for the new row. The Select object returned represents the complete
/// VALUES clause, including the new row.
///
/// There are two ways in which this may be achieved - by incremental
/// coding of a co-routine (the "co-routine" method) or by returning a
/// Select object equivalent to the following (the "UNION ALL" method):
///
///        "pLeft UNION ALL SELECT pRow"
///
/// If the VALUES clause contains a lot of rows, this compound Select
/// object may consume a lot of memory.
///
/// When the co-routine method is used, each row that will be returned
/// by the VALUES clause is coded into part of a co-routine as it is
/// passed to this function. The returned Select object is equivalent to:
///
///     SELECT * FROM (
///       Select object to read co-routine
///     )
///
/// The co-routine method is used in most cases. Exceptions are:
///
///    a) If the current statement has a WITH clause. This is to avoid
///       statements like:
///
///            WITH cte AS ( VALUES('x'), ('y') ... )
///            SELECT * FROM cte AS a, cte AS b;
///
///       This will not work, as the co-routine uses a hard-coded register
///       for its OP_Yield instructions, and so it is not possible for two
///       cursors to iterate through it concurrently.
///
///    b) The schema is currently being parsed (i.e. the VALUES clause is part
///       of a schema item like a VIEW or TRIGGER). In this case there is no VM
///       being generated when parsing is taking place, and so generating
///       a co-routine is not possible.
///
///    c) There are non-constant expressions in the VALUES clause (e.g.
///       the VALUES clause is part of a correlated sub-query).
///
///    d) One or more of the values in the first row of the VALUES clause
///       has an affinity (i.e. is a CAST expression). This causes problems
///       because the complex rules SQLite uses (see function
///       sqlite3SubqueryColumnTypes() in select.c) to determine the effective
///       affinity of such a column for all rows require access to all values in
///       the column simultaneously.
pub fn sqlite3MultiValues(db: &mut Sqlite3, pLeft: SelectId, pRow: ExprListId) -> SelectId {
    let mut pLeft = pLeft;
    let leftSrcN = {
        let s = db.sel(pLeft).pSrc;
        if s.nn() { db.src(s).nSrc() } else { 0 }
    };
    let cantCoroutine = db.ppr().bHasWith != 0                  /* condition (a) above */
        || db.init.busy != 0                                       /* condition (b) above */
        || sqlite3ExprListIsConstant(db, pRow, 1) == 0             /* condition (c) above */
        || (leftSrcN == 0 && {
            let el = db.sel(pLeft).pEList;
            !exprListIsNoAffinity(db, el)
        })                                                         /* condition (d) above */
        || db.ppr().IN_SPECIAL_PARSE();
    if cantCoroutine {
        /* The co-routine method cannot be used. Fall back to UNION ALL. */
        let mut f = SF_Values | SF_MultiValue;
        if leftSrcN != 0 {
            sqlite3MultiValuesEnd(db, pLeft);
            f = SF_Values;
        } else if db.sel(pLeft).pPrior.nn() {
            /* In this case set the SF_MultiValue flag only if it was set on pLeft */
            f = f & db.sel(pLeft).selFlags;
        }
        let pSelect = sqlite3SelectNew(
            db,
            pRow,
            SrcListId::NULL,
            ExprId::NULL,
            ExprListId::NULL,
            ExprId::NULL,
            ExprListId::NULL,
            f,
            ExprId::NULL,
        );
        db.selm(pLeft).selFlags &= !SF_MultiValue;
        if pSelect.nn() {
            db.selm(pSelect).op = TK_ALL;
            db.selm(pSelect).pPrior = pLeft;
            pLeft = pSelect;
        }
    } else {
        let p: SrcItemId; /* SrcItem that reads from co-routine */

        if leftSrcN == 0 {
            /* Co-routine has not yet been started and the special Select object
             ** that accesses the co-routine has not yet been created. This block
             ** does both those things. */
            sqlite3GetVdbe(db);
            let pRet = sqlite3SelectNew(
                db,
                ExprListId::NULL,
                SrcListId::NULL,
                ExprId::NULL,
                ExprListId::NULL,
                ExprId::NULL,
                ExprListId::NULL,
                0,
                ExprId::NULL,
            );

            /* Ensure the database schema has been read. This is to ensure we have
             ** the correct text encoding.  */
            if (db.mDbFlags & DBFLAG_SchemaKnownOk) == 0 {
                crate::prepare::sqlite3ReadSchema(db);
            }

            let mut dest = SelectDest::default();
            let retSrc = db.sel(pRet).pSrc;
            if db.src(retSrc).a.is_empty() {
                db.srcm(retSrc).a.push(SrcItem::default());
            }
            let (lprior, lop) = {
                let l = db.sel(pLeft);
                (l.pPrior, l.op)
            };
            db.selm(pRet).pPrior = lprior;
            db.selm(pRet).op = lop;
            if lprior.nn() {
                db.selm(pRet).selFlags |= SF_Values;
            }
            db.selm(pLeft).pPrior = SelectId::NULL;
            db.selm(pLeft).op = TK_SELECT;
            debug_assert!(db.sel(pLeft).pNext.is_null());
            debug_assert!(db.sel(pRet).pNext.is_null());
            p = SrcItemId(retSrc, 0);
            {
                let it = db.itemm(p);
                it.fg.viaCoroutine = 1;
                it.iCursor = -1;
                debug_assert!(it.fg.isIndexedBy == 0 && it.fg.isTabFunc == 0);
                it.u1.nRow = 2;
            }
            if sqlite3SrcItemAttachSubquery(db, p, pLeft, 0) != 0 {
                let addrFillSub = sqlite3VdbeCurrentAddr(db.v()) + 1;
                db.pp().nMem += 1;
                let regReturn = db.ppr().nMem;
                {
                    let q = db.itemm(p).u4.pSubq.as_mut().unwrap();
                    q.addrFillSub = addrFillSub;
                    q.regReturn = regReturn;
                }
                sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regReturn, 0, addrFillSub);
                sqlite3SelectDestInit(&mut dest, SRT_Coroutine as i32, regReturn);

                /* Allocate registers for the output of the co-routine. Do so so
                 ** that there are two unused registers immediately before those
                 ** used by the co-routine. This allows the code in sqlite3Insert()
                 ** to use these registers directly, instead of copying the output
                 ** of the co-routine to a separate array for processing.  */
                dest.iSdst = db.ppr().nMem + 3;
                let el = db.sel(pLeft).pEList;
                dest.nSdst = db.el(el).nExpr();
                db.pp().nMem += 2 + dest.nSdst;

                db.selm(pLeft).selFlags |= SF_MultiValue;
                sqlite3Select(db, pLeft, &mut dest);
                db.itemm(p).u4.pSubq.as_mut().unwrap().regResult = dest.iSdst;
                debug_assert!(db.ppr().nErr != 0 || dest.iSdst > 0);
            }
            pLeft = pRet;
        } else {
            let src = db.sel(pLeft).pSrc;
            p = SrcItemId(src, 0);
            let it = db.itemm(p);
            debug_assert!(it.fg.isTabFunc == 0 && it.fg.isIndexedBy == 0);
            it.u1.nRow += 1;
        }

        if db.ppr().nErr == 0 {
            let pSubq = {
                let it = db.item(p);
                debug_assert!(it.fg.isSubquery != 0);
                **it.u4.pSubq.as_ref().unwrap()
            };
            debug_assert!(pSubq.pSelect.nn());
            let subEl = db.sel(pSubq.pSelect).pEList;
            debug_assert!(subEl.nn());
            if db.el(subEl).nExpr() != db.el(pRow).nExpr() {
                sqlite3SelectWrongNumTermsError(db, pSubq.pSelect);
            } else {
                sqlite3ExprCodeExprList(db, pRow, pSubq.regResult, 0, 0);
                sqlite3VdbeAddOp1(db.v(), OP_Yield, pSubq.regReturn);
            }
        }
        sqlite3ExprListDelete(db, pRow);
    }

    pLeft
}

/// C: `void sqlite3Insert( Parse *pParse, SrcList *pTabList, Select *pSelect, IdList *pColumn, int onError, Upsert *pUpsert )`
///
/// This routine is called to handle SQL of the following forms:
///
///    insert into TABLE (IDLIST) values(EXPRLIST),(EXPRLIST),...
///    insert into TABLE (IDLIST) select
///    insert into TABLE (IDLIST) default values
///
/// The IDLIST following the table name is always optional.  If omitted,
/// then a list of all (non-hidden) columns for the table is substituted.
/// The IDLIST appears in the pColumn parameter.  pColumn is NULL if IDLIST
/// is omitted.
///
/// For the pSelect parameter holds the values to be inserted for the
/// first two forms shown above.  A VALUES clause is really just short-hand
/// for a SELECT statement that omits the FROM clause and everything else
/// that follows.  If the pSelect parameter is NULL, that means that the
/// DEFAULT VALUES form of the INSERT statement is intended.
///
/// The code generated follows one of four templates.  For a simple
/// insert with data coming from a single-row VALUES clause, the code executes
/// once straight down through.  Pseudo-code follows (we call this
/// the "1st template"):
///
///         open write cursor to <table> and its indices
///         put VALUES clause expressions into registers
///         write the resulting record into <table>
///         cleanup
///
/// The three remaining templates assume the statement is of the form
///
///   INSERT INTO <table> SELECT ...
///
/// If the SELECT clause is of the restricted form "SELECT * FROM <table2>" -
/// in other words if the SELECT pulls all columns from a single table
/// and there is no WHERE or LIMIT or GROUP BY or ORDER BY clauses, and
/// if <table2> and <table1> are distinct tables but have identical
/// schemas, including all the same indices, then a special optimization
/// is invoked that copies raw records from <table2> over to <table1>.
/// See the xferOptimization() function for the implementation of this
/// template.  This is the 2nd template.
///
///         open a write cursor to <table>
///         open read cursor on <table2>
///         transfer all records in <table2> over to <table>
///         close cursors
///         foreach index on <table>
///           open a write cursor on the <table> index
///           open a read cursor on the corresponding <table2> index
///           transfer all records from the read to the write cursors
///           close cursors
///         end foreach
///
/// The 3rd template is for when the second template does not apply
/// and the SELECT clause does not read from <table> at any time.
/// The generated code follows this template:
///
///         X <- A
///         goto B
///      A: setup for the SELECT
///         loop over the rows in the SELECT
///           load values into registers R..R+n
///           yield X
///         end loop
///         cleanup after the SELECT
///         end-coroutine X
///      B: open write cursor to <table> and its indices
///      C: yield X, at EOF goto D
///         insert the select result into <table> from R..R+n
///         goto C
///      D: cleanup
///
/// The 4th template is used if the insert statement takes its
/// values from a SELECT but the data is being inserted into a table
/// that is also read as part of the SELECT.  In the third form,
/// we have to use an intermediate table to store the results of
/// the select.  The template is like this:
///
///         X <- A
///         goto B
///      A: setup for the SELECT
///         loop over the tables in the SELECT
///           load value into register R..R+n
///           yield X
///         end loop
///         cleanup after the SELECT
///         end co-routine R
///      B: open temp table
///      L: yield X, at EOF goto M
///         insert row from R..R+n into temp table
///         goto L
///      M: open write cursor to <table> and its indices
///         rewind temp table
///      C: loop over rows of intermediate table
///           transfer values form intermediate table into <table>
///         end loop
///      D: cleanup
pub fn sqlite3Insert(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pSelect: SelectId,
    pColumn: IdListId,
    onError: i32,
    pUpsert: UpsertId,
) {
    let mut pSelect = pSelect;
    let mut pList = ExprListId::NULL; /* List of VALUES() to be inserted  */

    'cleanup: {
        if db.ppr().nErr != 0 {
            break 'cleanup;
        }
        let mut dest = SelectDest::default();
        dest.iSDParm = 0; /* Suppress a harmless compiler warning */

        /* If the Select object is really just a simple VALUES() list with a
         ** single row (the common case) then keep that one row of values
         ** and discard the other (unused) parts of the pSelect object
         */
        if pSelect.nn()
            && (db.sel(pSelect).selFlags & SF_Values) != 0
            && db.sel(pSelect).pPrior.is_null()
        {
            pList = db.sel(pSelect).pEList;
            db.selm(pSelect).pEList = ExprListId::NULL;
            sqlite3SelectDelete(db, pSelect);
            pSelect = SelectId::NULL;
        }

        /* Locate the table into which we will be inserting new information.
         */
        debug_assert!(db.src(pTabList).nSrc() == 1);
        let pTab = match sqlite3SrcListLookup(db, pTabList) {
            Some(t) => t,
            None => break 'cleanup,
        };
        let iDb = db.schema_index(pTab.borrow().pSchema);
        debug_assert!(iDb < db.nDb());
        let zTabName = pTab.borrow().zName.clone();
        let zDbSName = db.aDb[iDb as usize].zDbSName.clone();
        if sqlite3AuthCheck(
            db,
            SQLITE_INSERT,
            Some(&zTabName),
            None,
            zDbSName.as_deref(),
        ) != 0
        {
            break 'cleanup;
        }
        let withoutRowid = !pTab.borrow().HasRowid();

        /* Figure out if we have any triggers and if the table being
         ** inserted into is a view
         */
        let mut tmask = 0;
        let pTrigger = sqlite3TriggersExist(
            db,
            &pTab,
            TK_INSERT as i32,
            ExprListId::NULL,
            Some(&mut tmask),
        );
        let isView = pTab.borrow().IsView();
        debug_assert!((!pTrigger.is_empty() && tmask != 0) || (pTrigger.is_empty() && tmask == 0));

        /* If pTab is really a view, make sure it has been initialized.
         ** ViewGetColumnNames() is a no-op if pTab is not a view.
         */
        if sqlite3ViewGetColumnNames(db, &pTab) != 0 {
            break 'cleanup;
        }

        /* Cannot insert into a read-only table.
         */
        if sqlite3IsReadOnly(db, &pTab, &pTrigger) != 0 {
            break 'cleanup;
        }

        /* Allocate a VDBE
         */
        if !sqlite3GetVdbe(db) {
            break 'cleanup;
        }
        if db.ppr().nested == 0 {
            sqlite3VdbeCountChanges(db.v());
        }
        sqlite3BeginWriteOperation(db, (pSelect.nn() || !pTrigger.is_empty()) as i32, iDb);

        let mut regRowCount = 0; /* Memory cell used for the row counter */

        'insert_end: {
            /* If the statement is of the form
             **
             **       INSERT INTO <table1> SELECT * FROM <table2>;
             **
             ** Then special optimizations can be applied that make the transfer
             ** very fast and which reduce fragmentation of indices.
             **
             ** This is the 2nd template.
             */
            if pColumn.is_null()
                && pSelect.nn()
                && pTrigger.is_empty()
                && xferOptimization(db, &pTab, pSelect, onError, iDb) != 0
            {
                debug_assert!(pTrigger.is_empty());
                debug_assert!(pList.is_null());
                break 'insert_end;
            }

            let nCol = pTab.borrow().nCol as i32;
            let iPKey = pTab.borrow().iPKey as i32;
            let isVirtual = pTab.borrow().IsVirtual();
            let mut nHidden = 0; /* Number of hidden columns if TABLE is virtual */
            let mut iDataCur = 0; /* VDBE cursor that is the main data repository */
            let mut iIdxCur = 0; /* First index cursor */
            let mut ipkColumn = -1; /* Column that is the INTEGER PRIMARY KEY */
            let mut srcTab = 0; /* Data comes from this temporary cursor if >=0 */
            let mut addrInsTop = 0; /* Jump to label "D" */
            let mut addrCont = 0; /* Top of insert loop. Label "C" in templates 3 and 4 */
            let mut useTempTable = false; /* Store SELECT results in intermediate table */
            let mut appendFlag = 0; /* True if the insert is likely to be an append */
            let mut regFromSelect = 0; /* Base register for data coming from SELECT */
            let mut aRegIdx: Vec<i32> = Vec::new(); /* One register allocated to each index */
            let mut aTabColMap: Vec<i32> = Vec::new(); /* Mapping from pTab columns to pCol entries */
            let nColumn: i32; /* Number of columns in the data */

            /* If this is an AUTOINCREMENT table, look up the sequence number in the
             ** sqlite_sequence table and store it in memory cell regAutoinc.
             */
            let regAutoinc = autoIncBegin(db, iDb, &pTab);

            /* Allocate a block registers to hold the rowid and the values
             ** for all columns of the new row.
             */
            let mut regIns = db.ppr().nMem + 1;
            let mut regRowid = regIns;
            db.pp().nMem += nCol + 1;
            if isVirtual {
                regRowid += 1;
                db.pp().nMem += 1;
            }
            let mut regData = regRowid + 1;

            /* If the INSERT statement included an IDLIST term, then make sure
             ** all elements of the IDLIST really are columns of the table and
             ** remember the column indices.
             **
             ** If the table has an INTEGER PRIMARY KEY column and that column
             ** is named in the IDLIST, then record in the ipkColumn variable
             ** the index into IDLIST of the primary key column.  ipkColumn is
             ** the index of the primary key as it appears in IDLIST, not as
             ** is appears in the original table.  (The index of the INTEGER
             ** PRIMARY KEY in the original table is pTab->iPKey.)  After this
             ** loop, if ipkColumn==(-1), that means that integer primary key
             ** is unspecified, and hence the table is either WITHOUT ROWID or
             ** it will automatically generated an integer primary key.
             **
             ** bIdListInOrder is true if the columns in IDLIST are in storage
             ** order.  This enables an optimization that avoids shuffling the
             ** columns into storage order.  False negatives are harmless,
             ** but false positives will cause database corruption.
             */
            let mut bIdListInOrder = (pTab.borrow().tabFlags & (TF_OOOHidden | TF_HasStored)) == 0;
            if pColumn.nn() {
                aTabColMap = vec![0; nCol as usize];
                let nId = db.idl(pColumn).nId();
                for i in 0..nId {
                    let zName = db.idl(pColumn).a[i as usize].zName.clone();
                    let j = sqlite3ColumnIndex(&pTab, zName.as_deref());
                    if j >= 0 {
                        if aTabColMap[j as usize] == 0 {
                            aTabColMap[j as usize] = i + 1;
                        }
                        if i != j {
                            bIdListInOrder = false;
                        }
                        if j == iPKey {
                            ipkColumn = i;
                            debug_assert!(!withoutRowid);
                        }
                        let (cf, zCn) = {
                            let t = pTab.borrow();
                            (
                                t.aCol[j as usize].colFlags,
                                t.aCol[j as usize].zCnName.clone(),
                            )
                        };
                        if cf & (COLFLAG_STORED | COLFLAG_VIRTUAL) != 0 {
                            errmsg!(db, "cannot INSERT into generated column \"%s\"", &zCn);
                            break 'cleanup;
                        }
                    } else if sqlite3IsRowid(zName.as_deref()) != 0 && !withoutRowid {
                        ipkColumn = i;
                        bIdListInOrder = false;
                    } else {
                        errmsg!(
                            db,
                            "table %S has no column named %s",
                            SrcItemId(pTabList, 0),
                            &zName
                        );
                        db.pp().checkSchema = 1;
                        break 'cleanup;
                    }
                }
            }

            /* Figure out how many columns of data are supplied.  If the data
             ** is coming from a SELECT statement, then generate a co-routine that
             ** produces a single row of the SELECT on each invocation.  The
             ** co-routine is the common header to the 3rd and 4th templates.
             */
            if pSelect.nn() {
                /* Data is coming from a SELECT or from a multi-row VALUES clause.
                 ** Generate a co-routine to run the SELECT. */
                let (sSrc, sPrior) = {
                    let s = db.sel(pSelect);
                    (s.pSrc, s.pPrior)
                };
                let viaCo = sSrc.nn()
                    && db.src(sSrc).nSrc() == 1
                    && db.src(sSrc).a[0].fg.viaCoroutine != 0
                    && sPrior.is_null();
                if viaCo {
                    let pItem = SrcItemId(sSrc, 0);
                    let pSubq = {
                        let it = db.item(pItem);
                        debug_assert!(it.fg.isSubquery != 0);
                        **it.u4.pSubq.as_ref().unwrap()
                    };
                    dest.iSDParm = pSubq.regReturn;
                    regFromSelect = pSubq.regResult;
                    debug_assert!(pSubq.pSelect.nn());
                    let el = db.sel(pSubq.pSelect).pEList;
                    nColumn = db.el(el).nExpr();
                    sqlite3VdbeExplain(db, 0, b"SCAN %S", &[PfArg::from(pItem)]);
                    if bIdListInOrder && nColumn == nCol {
                        regData = regFromSelect;
                        regRowid = regData - 1;
                        regIns = regRowid - if isVirtual { 1 } else { 0 };
                    }
                } else {
                    db.pp().nMem += 1;
                    let regYield = db.ppr().nMem;
                    let addrTop = sqlite3VdbeCurrentAddr(db.v()) + 1;
                    sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regYield, 0, addrTop);
                    sqlite3SelectDestInit(&mut dest, SRT_Coroutine as i32, regYield);
                    dest.iSdst = if bIdListInOrder { regData } else { 0 };
                    dest.nSdst = nCol;
                    let rc = sqlite3Select(db, pSelect, &mut dest);
                    regFromSelect = dest.iSdst;
                    if rc != 0 || db.ppr().nErr != 0 {
                        break 'cleanup;
                    }
                    sqlite3VdbeEndCoroutine(db, regYield);
                    sqlite3VdbeJumpHere(db.v(), addrTop - 1); /* label B: */
                    let el = db.sel(pSelect).pEList;
                    debug_assert!(el.nn());
                    nColumn = db.el(el).nExpr();
                }

                /* Set useTempTable to TRUE if the result of the SELECT statement
                 ** should be written into a temporary table (template 4).  Set to
                 ** FALSE if each output row of the SELECT can be written directly into
                 ** the destination table (template 3).
                 **
                 ** A temp table must be used if the table being updated is also one
                 ** of the tables being read by the SELECT statement.  Also use a
                 ** temp table in the case of row triggers.
                 */
                if !pTrigger.is_empty() || readsTable(db, iDb, &pTab) {
                    useTempTable = true;
                }

                if useTempTable {
                    /* Invoke the coroutine to extract information from the SELECT
                     ** and add it to a transient table srcTab.  The code generated
                     ** here is from the 4th template:
                     **
                     **      B: open temp table
                     **      L: yield X, goto M at EOF
                     **         insert row from R..R+n into temp table
                     **         goto L
                     **      M: ...
                     */
                    srcTab = db.ppr().nTab;
                    db.pp().nTab += 1;
                    let regRec = sqlite3GetTempReg(db);
                    let regTempRowid = sqlite3GetTempReg(db);
                    let v = db.v();
                    sqlite3VdbeAddOp2(v, OP_OpenEphemeral, srcTab, nColumn);
                    let addrL = sqlite3VdbeAddOp1(v, OP_Yield, dest.iSDParm);
                    sqlite3VdbeAddOp3(v, OP_MakeRecord, regFromSelect, nColumn, regRec);
                    sqlite3VdbeAddOp2(v, OP_NewRowid, srcTab, regTempRowid);
                    sqlite3VdbeAddOp3(v, OP_Insert, srcTab, regRec, regTempRowid);
                    sqlite3VdbeGoto(v, addrL);
                    sqlite3VdbeJumpHere(v, addrL);
                    sqlite3ReleaseTempReg(db, regRec);
                    sqlite3ReleaseTempReg(db, regTempRowid);
                }
            } else {
                /* This is the case if the data for the INSERT is coming from a
                 ** single-row VALUES clause
                 */
                let sNC = db.arena.new_nc(NameContext::default());
                srcTab = -1;
                debug_assert!(!useTempTable);
                if pList.nn() {
                    nColumn = db.el(pList).nExpr();
                    if sqlite3ResolveExprListNames(db, sNC, pList) != 0 {
                        break 'cleanup;
                    }
                } else {
                    nColumn = 0;
                }
            }

            /* If there is no IDLIST term but the table has an integer primary
             ** key, the set the ipkColumn variable to the integer primary key
             ** column index in the original table definition.
             */
            if pColumn.is_null() && nColumn > 0 {
                ipkColumn = iPKey;
                if ipkColumn >= 0 && (pTab.borrow().tabFlags & TF_HasGenerated) != 0 {
                    let t = pTab.borrow();
                    let mut i = ipkColumn - 1;
                    while i >= 0 {
                        if t.aCol[i as usize].colFlags & COLFLAG_GENERATED != 0 {
                            ipkColumn -= 1;
                        }
                        i -= 1;
                    }
                }

                /* Make sure the number of columns in the source data matches the number
                 ** of columns to be inserted into the table.
                 */
                if (pTab.borrow().tabFlags & (TF_HasGenerated | TF_HasHidden)) != 0 {
                    let t = pTab.borrow();
                    for i in 0..nCol {
                        if t.aCol[i as usize].colFlags & COLFLAG_NOINSERT != 0 {
                            nHidden += 1;
                        }
                    }
                }
                if nColumn != (nCol - nHidden) {
                    errmsg!(
                        db,
                        "table %S has %d columns but %d values were supplied",
                        SrcItemId(pTabList, 0),
                        nCol - nHidden,
                        nColumn
                    );
                    break 'cleanup;
                }
            }
            if pColumn.nn() && nColumn != db.idl(pColumn).nId() {
                let nId = db.idl(pColumn).nId();
                errmsg!(db, "%d values for %d columns", nColumn, nId);
                break 'cleanup;
            }

            /* Initialize the count of rows to be inserted
             */
            if (db.flags & SQLITE_CountRows) != 0
                && db.ppr().nested == 0
                && db.ppr().pTriggerTab.is_none()
                && db.ppr().bReturning == 0
            {
                db.pp().nMem += 1;
                regRowCount = db.ppr().nMem;
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regRowCount);
            }

            /* If this is not a view, open the table and and all indices */
            if !isView {
                let nIdx = sqlite3OpenTableAndIndices(
                    db,
                    &pTab,
                    OP_OpenWrite as i32,
                    0,
                    -1,
                    None,
                    &mut iDataCur,
                    &mut iIdxCur,
                );
                aRegIdx = vec![0; (nIdx + 2) as usize];
                let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
                for i in 0..nIdx as usize {
                    db.pp().nMem += 1;
                    aRegIdx[i] = db.ppr().nMem;
                    db.pp().nMem += aIdx[i].borrow().nColumn as i32;
                }
                db.pp().nMem += 1;
                aRegIdx[nIdx as usize] = db.ppr().nMem; /* Register to store the table record */
            }
            if pUpsert.nn() {
                if isVirtual {
                    errmsg!(
                        db,
                        "UPSERT not implemented for virtual table \"%s\"",
                        &zTabName
                    );
                    break 'cleanup;
                }
                if pTab.borrow().IsView() {
                    errmsg!(db, "cannot UPSERT a view");
                    break 'cleanup;
                }
                let tgt = db.arena.ups(pUpsert).pUpsertTarget;
                if sqlite3HasExplicitNulls(db, tgt) != 0 {
                    break 'cleanup;
                }
                db.srcm(pTabList).a[0].iCursor = iDataCur;
                let mut pNx = pUpsert;
                loop {
                    {
                        let u = db.arena.upsm(pNx);
                        u.pUpsertSrc = pTabList;
                        u.regData = regData;
                        u.iDataCur = iDataCur;
                        u.iIdxCur = iIdxCur;
                    }
                    if db.arena.ups(pNx).pUpsertTarget.nn() {
                        if sqlite3UpsertAnalyzeTarget(db, pTabList, pNx, pUpsert) != 0 {
                            break 'cleanup;
                        }
                    }
                    pNx = db.arena.ups(pNx).pNextUpsert;
                    if pNx.is_null() {
                        break;
                    }
                }
            }

            /* This is the top of the main insertion loop */
            if useTempTable {
                /* This block codes the top of loop only.  The complete loop is the
                 ** following pseudocode (template 4):
                 **
                 **         rewind temp table, if empty goto D
                 **      C: loop over rows of intermediate table
                 **           transfer values form intermediate table into <table>
                 **         end loop
                 **      D: ...
                 */
                addrInsTop = sqlite3VdbeAddOp1(db.v(), OP_Rewind, srcTab);
                addrCont = sqlite3VdbeCurrentAddr(db.v());
            } else if pSelect.nn() {
                /* This block codes the top of loop only.  The complete loop is the
                 ** following pseudocode (template 3):
                 **
                 **      C: yield X, at EOF goto D
                 **         insert the select result into <table> from R..R+n
                 **         goto C
                 **      D: ...
                 */
                addrCont = sqlite3VdbeAddOp1(db.v(), OP_Yield, dest.iSDParm);
                addrInsTop = addrCont;
                if ipkColumn >= 0 {
                    /* tag-20191021-001: If the INTEGER PRIMARY KEY is being generated by the
                     ** SELECT, go ahead and copy the value into the rowid slot now, so that
                     ** the value does not get overwritten by a NULL at tag-20191021-002. */
                    sqlite3VdbeAddOp2(db.v(), OP_Copy, regFromSelect + ipkColumn, regRowid);
                }
            }

            /* Compute data for ordinary columns of the new entry.  Values
             ** are written in storage order into registers starting with regData.
             ** Only ordinary columns are computed in this loop. The rowid
             ** (if there is one) is computed later and generated columns are
             ** computed after the rowid since they might depend on the value
             ** of the rowid.
             */
            nHidden = 0;
            let mut iRegStore = regData;
            debug_assert!(regData == regRowid + 1);
            let mut i = 0;
            while i < nCol {
                'col: {
                    let k;
                    debug_assert!(i >= nHidden);
                    if i == iPKey {
                        /* tag-20191021-002: References to the INTEGER PRIMARY KEY are filled
                         ** using the rowid. So put a NULL in the IPK slot of the record to avoid
                         ** using excess space.  The file format definition requires this extra
                         ** NULL - we cannot optimize further by skipping the column completely */
                        sqlite3VdbeAddOp1(db.v(), OP_SoftNull, iRegStore);
                        break 'col;
                    }
                    let colFlags = pTab.borrow().aCol[i as usize].colFlags;
                    if (colFlags & COLFLAG_NOINSERT) != 0 {
                        nHidden += 1;
                        if (colFlags & COLFLAG_VIRTUAL) != 0 {
                            /* Virtual columns do not participate in OP_MakeRecord.  So back up
                             ** iRegStore by one slot to compensate for the iRegStore++ in the
                             ** outer for() loop */
                            iRegStore -= 1;
                            break 'col;
                        } else if (colFlags & COLFLAG_STORED) != 0 {
                            /* Stored columns are computed later.  But if there are BEFORE
                             ** triggers, the slots used for stored columns will be OP_Copy-ed
                             ** to a second block of registers, so the register needs to be
                             ** initialized to NULL to avoid an uninitialized register read */
                            if (tmask & TRIGGER_BEFORE as i32) != 0 {
                                sqlite3VdbeAddOp1(db.v(), OP_SoftNull, iRegStore);
                            }
                            break 'col;
                        } else if pColumn.is_null() {
                            /* Hidden columns that are not explicitly named in the INSERT
                             ** get their default value */
                            let e = sqlite3ColumnExprAuth(db, &pTab, i as usize);
                            sqlite3ExprCodeFactorable(db, e, iRegStore);
                            break 'col;
                        }
                    }
                    if pColumn.nn() {
                        let j = aTabColMap[i as usize];
                        debug_assert!(j >= 0 && j <= db.idl(pColumn).nId());
                        if j == 0 {
                            /* A column not named in the insert column list gets its
                             ** default value */
                            let e = sqlite3ColumnExprAuth(db, &pTab, i as usize);
                            sqlite3ExprCodeFactorable(db, e, iRegStore);
                            break 'col;
                        }
                        k = j - 1;
                    } else if nColumn == 0 {
                        /* This is INSERT INTO ... DEFAULT VALUES.  Load the default value. */
                        let e = sqlite3ColumnExprAuth(db, &pTab, i as usize);
                        sqlite3ExprCodeFactorable(db, e, iRegStore);
                        break 'col;
                    } else {
                        k = i - nHidden;
                    }

                    if useTempTable {
                        sqlite3VdbeAddOp3(db.v(), OP_Column, srcTab, k, iRegStore);
                    } else if pSelect.nn() {
                        if regFromSelect != regData {
                            sqlite3VdbeAddOp2(db.v(), OP_SCopy, regFromSelect + k, iRegStore);
                        }
                    } else {
                        let pX = db.el(pList).a[k as usize].pExpr;
                        let y = sqlite3ExprCodeTarget(db, pX, iRegStore);
                        if y != iRegStore {
                            let op = if db.ex(pX).HasProperty(EP_Subquery) {
                                OP_Copy
                            } else {
                                OP_SCopy
                            };
                            sqlite3VdbeAddOp2(db.v(), op, y, iRegStore);
                        }
                    }
                }
                i += 1;
                iRegStore += 1;
            }

            /* Run the BEFORE and INSTEAD OF triggers, if there are any
             */
            let endOfLoop = sqlite3VdbeMakeLabel(db);
            if (tmask & TRIGGER_BEFORE as i32) != 0 {
                let regCols = sqlite3GetTempRange(db, nCol + 1);

                /* build the NEW.* reference row.  Note that if there is an INTEGER
                 ** PRIMARY KEY into which a NULL is being inserted, that NULL will be
                 ** translated into a unique ID for the row.  But on a BEFORE trigger,
                 ** we do not know what the unique ID will be (because the insert has
                 ** not happened yet) so we substitute a rowid of -1
                 */
                if ipkColumn < 0 {
                    sqlite3VdbeAddOp2(db.v(), OP_Integer, -1, regCols);
                } else {
                    debug_assert!(!withoutRowid);
                    if useTempTable {
                        sqlite3VdbeAddOp3(db.v(), OP_Column, srcTab, ipkColumn, regCols);
                    } else {
                        debug_assert!(pSelect.is_null()); /* Otherwise useTempTable is true */
                        let e = db.el(pList).a[ipkColumn as usize].pExpr;
                        sqlite3ExprCode(db, e, regCols);
                    }
                    let addr1 = sqlite3VdbeAddOp1(db.v(), OP_NotNull, regCols);
                    sqlite3VdbeAddOp2(db.v(), OP_Integer, -1, regCols);
                    sqlite3VdbeJumpHere(db.v(), addr1);
                    sqlite3VdbeAddOp1(db.v(), OP_MustBeInt, regCols);
                    sqlite3MayAbort(db);
                }

                /* Copy the new data already generated. */
                let nNVCol = pTab.borrow().nNVCol as i32;
                debug_assert!(nNVCol > 0 || db.ppr().nErr > 0);
                sqlite3VdbeAddOp3(db.v(), OP_Copy, regRowid + 1, regCols + 1, nNVCol - 1);

                /* Compute the new value for generated columns after all other
                 ** columns have already been computed.  This must be done after
                 ** computing the ROWID in case one of the generated columns
                 ** refers to the ROWID. */
                if pTab.borrow().tabFlags & TF_HasGenerated != 0 {
                    sqlite3ComputeGeneratedColumns(db, regCols + 1, &pTab);
                }

                /* If this is an INSERT on a view with an INSTEAD OF INSERT trigger,
                 ** do not attempt any conversions before assembling the record.
                 ** If this is a real table, attempt conversions as required by the
                 ** table column affinities.
                 */
                if !isView {
                    sqlite3TableAffinity(db.v(), &pTab, regCols + 1);
                }

                /* Fire BEFORE or INSTEAD OF triggers */
                sqlite3CodeRowTrigger(
                    db,
                    &pTrigger,
                    TK_INSERT as i32,
                    ExprListId::NULL,
                    TRIGGER_BEFORE as i32,
                    &pTab,
                    regCols - nCol - 1,
                    onError,
                    endOfLoop,
                );

                sqlite3ReleaseTempRange(db, regCols, nCol + 1);
            }

            if !isView {
                if isVirtual {
                    /* The row that the VUpdate opcode will delete: none */
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regIns);
                }
                if ipkColumn >= 0 {
                    /* Compute the new rowid */
                    if useTempTable {
                        sqlite3VdbeAddOp3(db.v(), OP_Column, srcTab, ipkColumn, regRowid);
                    } else if pSelect.nn() {
                        /* Rowid already initialized at tag-20191021-001 */
                    } else {
                        let pIpk = db.el(pList).a[ipkColumn as usize].pExpr;
                        if db.ex(pIpk).op == TK_NULL && !isVirtual {
                            sqlite3VdbeAddOp3(db.v(), OP_NewRowid, iDataCur, regRowid, regAutoinc);
                            appendFlag = 1;
                        } else {
                            sqlite3ExprCode(db, pIpk, regRowid);
                        }
                    }
                    /* If the PRIMARY KEY expression is NULL, then use OP_NewRowid
                     ** to generate a unique primary key value.
                     */
                    if appendFlag == 0 {
                        if !isVirtual {
                            let addr1 = sqlite3VdbeAddOp1(db.v(), OP_NotNull, regRowid);
                            sqlite3VdbeAddOp3(db.v(), OP_NewRowid, iDataCur, regRowid, regAutoinc);
                            sqlite3VdbeJumpHere(db.v(), addr1);
                        } else {
                            let addr1 = sqlite3VdbeCurrentAddr(db.v());
                            sqlite3VdbeAddOp2(db.v(), OP_IsNull, regRowid, addr1 + 2);
                        }
                        sqlite3VdbeAddOp1(db.v(), OP_MustBeInt, regRowid);
                        sqlite3MayAbort(db);
                    }
                } else if isVirtual || withoutRowid {
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regRowid);
                } else {
                    sqlite3VdbeAddOp3(db.v(), OP_NewRowid, iDataCur, regRowid, regAutoinc);
                    appendFlag = 1;
                }
                autoIncStep(db, regAutoinc, regRowid);

                /* Compute the new value for generated columns after all other
                 ** columns have already been computed.  This must be done after
                 ** computing the ROWID in case one of the generated columns
                 ** is derived from the INTEGER PRIMARY KEY. */
                if pTab.borrow().tabFlags & TF_HasGenerated != 0 {
                    sqlite3ComputeGeneratedColumns(db, regRowid + 1, &pTab);
                }

                /* Generate code to check constraints and generate index keys and
                 ** do the insertion.
                 */
                if isVirtual {
                    crate::vtab::sqlite3GetVTable(db, &pTab);
                    crate::vtab::sqlite3VtabMakeWritable(db, &pTab);
                    // TODO(vtab): P4 should be the VTable (P4_VTAB).
                    sqlite3VdbeAddOp4(
                        db.v(),
                        OP_VUpdate,
                        1,
                        nCol + 2,
                        regIns,
                        P4::None,
                        P4_VTAB as i32,
                    );
                    sqlite3VdbeChangeP5(
                        db.v(),
                        if onError == OE_Default as i32 {
                            OE_Abort as u16
                        } else {
                            onError as u16
                        },
                    );
                    sqlite3MayAbort(db);
                } else {
                    let mut isReplace = 0; /* Set to true if constraints may cause a replace */
                    sqlite3GenerateConstraintChecks(
                        db,
                        &pTab,
                        &aRegIdx,
                        iDataCur,
                        iIdxCur,
                        regIns,
                        0,
                        (ipkColumn >= 0) as u8,
                        onError as u8,
                        endOfLoop,
                        &mut isReplace,
                        None,
                        pUpsert,
                    );
                    if db.flags & SQLITE_ForeignKeys != 0 {
                        sqlite3FkCheck(db, &pTab, 0, regIns, None, 0);
                    }

                    /* Set the OPFLAG_USESEEKRESULT flag if either (a) there are no REPLACE
                     ** constraints or (b) there are no triggers and this table is not a
                     ** parent table in a foreign key constraint. It is safe to set the
                     ** flag in the second case as if any REPLACE constraint is hit, an
                     ** OP_Delete or OP_IdxDelete instruction will be executed on each
                     ** cursor that is disturbed. And these instructions both clear the
                     ** VdbeCursor.seekResult variable, disabling the OPFLAG_USESEEKRESULT
                     ** functionality.  */
                    let bUseSeek = (isReplace == 0 || sqlite3VdbeHasSubProgram(db.v()) == 0) as i32;
                    sqlite3CompleteInsertion(
                        db, &pTab, iDataCur, iIdxCur, regIns, &aRegIdx, 0, appendFlag, bUseSeek,
                    );
                }
            }

            /* Update the count of rows that are inserted
             */
            if regRowCount != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_AddImm, regRowCount, 1);
            }

            if !pTrigger.is_empty() {
                /* Code AFTER triggers */
                sqlite3CodeRowTrigger(
                    db,
                    &pTrigger,
                    TK_INSERT as i32,
                    ExprListId::NULL,
                    TRIGGER_AFTER as i32,
                    &pTab,
                    regData - 2 - nCol,
                    onError,
                    endOfLoop,
                );
            }

            /* The bottom of the main insertion loop, if the data source
             ** is a SELECT statement.
             */
            sqlite3VdbeResolveLabel(db.v(), endOfLoop);
            if useTempTable {
                sqlite3VdbeAddOp2(db.v(), OP_Next, srcTab, addrCont);
                sqlite3VdbeJumpHere(db.v(), addrInsTop);
                sqlite3VdbeAddOp1(db.v(), OP_Close, srcTab);
            } else if pSelect.nn() {
                sqlite3VdbeGoto(db.v(), addrCont);
                sqlite3VdbeJumpHere(db.v(), addrInsTop);
            }
        }

        // insert_end:
        /* Update the sqlite_sequence table by storing the content of the
         ** maximum rowid counter values recorded while inserting into
         ** autoincrement tables.
         */
        if db.ppr().nested == 0 && db.ppr().pTriggerTab.is_none() {
            sqlite3AutoincrementEnd(db);
        }

        /*
         ** Return the number of rows inserted. If this routine is
         ** generating code because of a call to sqlite3NestedParse(), do not
         ** invoke the callback function.
         */
        if regRowCount != 0 {
            sqlite3CodeChangeCount(db.v(), regRowCount, Some(b"rows inserted"));
        }
    }

    // insert_cleanup:
    sqlite3SrcListDelete(db, pTabList);
    sqlite3ExprListDelete(db, pList);
    sqlite3UpsertDelete(db, pUpsert);
    sqlite3SelectDelete(db, pSelect);
    if pColumn.nn() {
        sqlite3IdListDelete(db, pColumn);
    }
}

/*
** Meanings of bits in of pWalker->eCode for
** sqlite3ExprReferencesUpdatedColumn()
*/
const CKCNSTRNT_COLUMN: u16 = 0x01; /* CHECK constraint uses a changing column */
const CKCNSTRNT_ROWID: u16 = 0x02; /* CHECK constraint references the ROWID */

/// This is the Walker callback from sqlite3ExprReferencesUpdatedColumn().
/// Set bit 0x01 of pWalker->eCode if pWalker->eCode to 0 and if this
/// expression node references any of the
/// columns that are being modified by an UPDATE statement.
fn checkConstraintExprNode(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let (op, iColumn) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iColumn)
    };
    if op == TK_COLUMN {
        debug_assert!(iColumn >= 0 || iColumn == -1);
        if iColumn >= 0 {
            if w.u.aiCol[iColumn as usize] >= 0 {
                w.eCode |= CKCNSTRNT_COLUMN;
            }
        } else {
            w.eCode |= CKCNSTRNT_ROWID;
        }
    }
    WRC_Continue
}

/// C: `int sqlite3ExprReferencesUpdatedColumn( Expr *pExpr, int *aiChng, int chngRowid )`
///
/// pExpr is a CHECK constraint on a row that is being UPDATE-ed.  The
/// only columns that are modified by the UPDATE are those for which
/// aiChng[i]>=0, and also the ROWID is modified if chngRowid is true.
///
/// Return true if CHECK constraint pExpr uses any of the
/// changing columns (or the rowid if it is changing).  In other words,
/// return true if this CHECK constraint must be validated for
/// the new row in the UPDATE statement.
///
/// 2018-09-15: pExpr might also be an expression for an index-on-expressions.
/// The operation of this routine is the same - return true if an only if
/// the expression uses one or more of columns identified by the second and
/// third arguments.
pub fn sqlite3ExprReferencesUpdatedColumn(
    db: &mut Sqlite3,
    pExpr: ExprId,
    aiChng: &[i32],
    chngRowid: i32,
) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = 0;
    w.xExprCallback = Some(checkConstraintExprNode);
    w.u.aiCol = aiChng.to_vec();
    sqlite3WalkExpr(&mut w, pExpr);
    if chngRowid == 0 {
        w.eCode &= !CKCNSTRNT_ROWID;
    }
    (w.eCode != 0) as i32
}

/// C: `void sqlite3GenerateConstraintChecks( Parse *pParse, Table *pTab, int *aRegIdx, int iDataCur, int iIdxCur, int regNewData, int regOldData, u8 pkChng, u8 overrideError, int ignoreDest, int *pbMayReplace, int *aiChng, Upsert *pUpsert )`
///
/// Generate code to do constraint checks prior to an INSERT or an UPDATE
/// on table pTab.
///
/// The regNewData parameter is the first register in a range that contains
/// the data to be inserted or the data after the update.  There will be
/// pTab->nCol+1 registers in this range.  The first register (the one
/// that regNewData points to) will contain the new rowid, or NULL in the
/// case of a WITHOUT ROWID table.  The second register in the range will
/// contain the content of the first table column.  The third register will
/// contain the content of the second table column.  And so forth.
///
/// The regOldData parameter is similar to regNewData except that it contains
/// the data prior to an UPDATE rather than afterwards.  regOldData is zero
/// for an INSERT.  This routine can distinguish between UPDATE and INSERT by
/// checking regOldData for zero.
///
/// For an UPDATE, the pkChng boolean is true if the true primary key (the
/// rowid for a normal table or the PRIMARY KEY for a WITHOUT ROWID table)
/// might be modified by the UPDATE.  If pkChng is false, then the key of
/// the iDataCur content table is guaranteed to be unchanged by the UPDATE.
///
/// For an INSERT, the pkChng boolean indicates whether or not the rowid
/// was explicitly specified as part of the INSERT statement.  If pkChng
/// is zero, it means that the either rowid is computed automatically or
/// that the table is a WITHOUT ROWID table and has no rowid.  On an INSERT,
/// pkChng will only be true if the INSERT statement provides an integer
/// value for either the rowid column or its INTEGER PRIMARY KEY alias.
///
/// The code generated by this routine will store new index entries into
/// registers identified by aRegIdx[].  No index entry is created for
/// indices where aRegIdx[i]==0.  The order of indices in aRegIdx[] is
/// the same as the order of indices on the linked list of indices
/// at pTab->pIndex.
///
/// (2019-05-07) The generated code also creates a new record for the
/// main table, if pTab is a rowid table, and stores that record in the
/// register identified by aRegIdx[nIdx] - in other words in the first
/// entry of aRegIdx[] past the last index.  It is important that the
/// record be generated during constraint checks to avoid affinity changes
/// to the register content that occur after constraint checks but before
/// the new record is inserted.
///
/// The caller must have already opened writeable cursors on the main
/// table and all applicable indices (that is to say, all indices for which
/// aRegIdx[] is not zero).  iDataCur is the cursor for the main table when
/// inserting or updating a rowid table, or the cursor for the PRIMARY KEY
/// index when operating on a WITHOUT ROWID table.  iIdxCur is the cursor
/// for the first index in the pTab->pIndex list.  Cursors for other indices
/// are at iIdxCur+N for the N-th element of the pTab->pIndex list.
///
/// This routine also generates code to check constraints.  NOT NULL,
/// CHECK, and UNIQUE constraints are all checked.  If a constraint fails,
/// then the appropriate action is performed.  There are five possible
/// actions: ROLLBACK, ABORT, FAIL, REPLACE, and IGNORE.
///
///  Constraint type  Action       What Happens
///  ---------------  ----------   ----------------------------------------
///  any              ROLLBACK     The current transaction is rolled back and
///                                sqlite3_step() returns immediately with a
///                                return code of SQLITE_CONSTRAINT.
///
///  any              ABORT        Back out changes from the current command
///                                only (do not do a complete rollback) then
///                                cause sqlite3_step() to return immediately
///                                with SQLITE_CONSTRAINT.
///
///  any              FAIL         Sqlite3_step() returns immediately with a
///                                return code of SQLITE_CONSTRAINT.  The
///                                transaction is not rolled back and any
///                                changes to prior rows are retained.
///
///  any              IGNORE       The attempt in insert or update the current
///                                row is skipped, without throwing an error.
///                                Processing continues with the next row.
///                                (There is an immediate jump to ignoreDest.)
///
///  NOT NULL         REPLACE      The NULL value is replace by the default
///                                value for that column.  If the default value
///                                is NULL, the action is the same as ABORT.
///
///  UNIQUE           REPLACE      The other row that conflicts with the row
///                                being inserted is removed.
///
///  CHECK            REPLACE      Illegal.  The results in an exception.
///
/// Which action to take is determined by the overrideError parameter.
/// Or if overrideError==OE_Default, then the pParse->onError parameter
/// is used.  Or if pParse->onError==OE_Default then the onError value
/// for the constraint is used.
///
/// Rust: `aiChng` is None for the C NULL pointer (INSERT).
pub fn sqlite3GenerateConstraintChecks(
    db: &mut Sqlite3,
    pTab: &TableRef,
    aRegIdx: &[i32],
    iDataCur: i32,
    iIdxCur: i32,
    regNewData: i32,
    regOldData: i32,
    pkChng: u8,
    overrideError: u8,
    ignoreDest: i32,
    pbMayReplace: &mut i32,
    aiChng: Option<&[i32]>,
    pUpsert: UpsertId,
) {
    let mut overrideError = overrideError as i32;
    let mut pUpsert = pUpsert;
    let mut onError: i32; /* Conflict resolution strategy */
    let mut seenReplace = 0; /* True if REPLACE is used to resolve INT PK conflict */
    let mut pUpsertClause = UpsertId::NULL; /* The specific ON CONFLICT clause for pIdx */
    let mut bAffinityDone = false; /* True if the OP_Affinity operation has been run */
    let mut upsertIpkReturn = 0; /* Address of Goto at end of IPK uniqueness check */
    let mut upsertIpkDelay = 0; /* Address of Goto to bypass initial IPK check */
    let mut ipkTop = 0; /* Top of the IPK uniqueness check */
    let mut ipkBottom = 0; /* OP_Goto at the end of the IPK uniqueness check */
    /* Variables associated with retesting uniqueness constraints after
     ** replace triggers fire have run */
    let mut regTrigCnt: i32; /* Register used to count replace trigger invocations */
    let mut addrRecheck = 0; /* Jump here to recheck all uniqueness constraints */
    let mut lblRecheckOk = 0; /* Each recheck jumps to this label if it passes */
    let pTrigger: Vec<TriggerRef>; /* List of DELETE triggers on the table pTab */
    let mut nReplaceTrig = 0; /* Number of replace triggers coded */

    let isUpdate = regOldData != 0;
    debug_assert!(!pTab.borrow().IsView()); /* This table is not a VIEW */
    let (nCol, iPKey, tabFlags, keyConf, hasRowid) = {
        let t = pTab.borrow();
        (
            t.nCol as i32,
            t.iPKey as i32,
            t.tabFlags,
            t.keyConf as i32,
            t.HasRowid(),
        )
    };
    let aIdxAll: Vec<IndexRef> = pTab.borrow().pIndex.clone();
    let nIdxAll = aIdxAll.len();

    /* pPk is the PRIMARY KEY index for WITHOUT ROWID tables and NULL for
     ** normal rowid tables.  nPkField is the number of key fields in the
     ** pPk index or 1 for a rowid table.  In other words, nPkField is the
     ** number of fields in the true primary key of the table. */
    let pPk: Option<IndexRef>;
    let nPkField: i32;
    if hasRowid {
        pPk = None;
        nPkField = 1;
    } else {
        pPk = sqlite3PrimaryKeyIndex(pTab);
        nPkField = pPk.as_ref().unwrap().borrow().nKeyCol as i32;
    }
    let is_pk = |x: &IndexRef| -> bool { opt_rc_eq(&pPk, x) };

    /* Test all NOT NULL constraints.
     */
    if tabFlags & TF_HasNotNull != 0 {
        let mut b2ndPass = false; /* True if currently running 2nd pass */
        let mut nSeenReplace = 0; /* Number of ON CONFLICT REPLACE operations */
        let mut nGenerated = 0; /* Number of generated columns with NOT NULL */
        loop {
            /* Make 2 passes over columns. Exit loop via "break" */
            for i in 0..nCol {
                let (notNull, colFlags, iDflt, zCnName) = {
                    let t = pTab.borrow();
                    let c = &t.aCol[i as usize];
                    (c.notNull as i32, c.colFlags, c.iDflt, c.zCnName.clone())
                };
                onError = notNull;
                if onError == OE_None as i32 {
                    continue; /* No NOT NULL on this column */
                }
                if i == iPKey {
                    continue; /* ROWID is never NULL */
                }
                let isGenerated = (colFlags & COLFLAG_GENERATED) != 0;
                if isGenerated && !b2ndPass {
                    nGenerated += 1;
                    continue; /* Generated columns processed on 2nd pass */
                }
                if let Some(a) = aiChng {
                    if a[i as usize] < 0 && !isGenerated {
                        /* Do not check NOT NULL on columns that do not change */
                        continue;
                    }
                }
                if overrideError != OE_Default as i32 {
                    onError = overrideError;
                } else if onError == OE_Default as i32 {
                    onError = OE_Abort as i32;
                }
                if onError == OE_Replace as i32 {
                    if b2ndPass        /* REPLACE becomes ABORT on the 2nd pass */
                        || iDflt == 0
                    /* REPLACE is ABORT if no DEFAULT value */
                    {
                        onError = OE_Abort as i32;
                    } else {
                        debug_assert!(!isGenerated);
                    }
                } else if b2ndPass && !isGenerated {
                    continue;
                }
                debug_assert!(
                    onError == OE_Rollback as i32
                        || onError == OE_Abort as i32
                        || onError == OE_Fail as i32
                        || onError == OE_Ignore as i32
                        || onError == OE_Replace as i32
                );
                let iReg = sqlite3TableColumnToStorage(pTab, i as i16) as i32 + regNewData + 1;
                if onError == OE_Replace as i32 {
                    let addr1 = sqlite3VdbeAddOp1(db.v(), OP_NotNull, iReg);
                    debug_assert!((colFlags & COLFLAG_GENERATED) == 0);
                    nSeenReplace += 1;
                    let e = sqlite3ColumnExprAuth(db, pTab, i as usize);
                    sqlite3ExprCodeCopy(db, e, iReg);
                    sqlite3VdbeJumpHere(db.v(), addr1);
                } else if onError == OE_Abort as i32
                    || onError == OE_Rollback as i32
                    || onError == OE_Fail as i32
                {
                    if onError == OE_Abort as i32 {
                        sqlite3MayAbort(db);
                    }
                    let zTab = pTab.borrow().zName.clone();
                    let zMsg = mprintf!(db, "%s.%s", &zTab, &zCnName);
                    sqlite3VdbeAddOp3(
                        db.v(),
                        OP_HaltIfNull,
                        SQLITE_CONSTRAINT_NOTNULL,
                        onError,
                        iReg,
                    );
                    sqlite3VdbeAppendP4(db.v(), P4::Z(zMsg), P4_DYNAMIC as i32);
                    sqlite3VdbeChangeP5(db.v(), P5_ConstraintNotNull as u16);
                } else {
                    debug_assert!(onError == OE_Ignore as i32);
                    sqlite3VdbeAddOp2(db.v(), OP_IsNull, iReg, ignoreDest);
                }
            } /* end loop i over columns */
            if nGenerated == 0 && nSeenReplace == 0 {
                /* If there are no generated columns with NOT NULL constraints
                 ** and no NOT NULL ON CONFLICT REPLACE constraints, then a single
                 ** pass is sufficient */
                break;
            }
            if b2ndPass {
                break; /* Never need more than 2 passes */
            }
            b2ndPass = true;
            if nSeenReplace > 0 && (tabFlags & TF_HasGenerated) != 0 {
                /* If any NOT NULL ON CONFLICT REPLACE constraints fired on the
                 ** first pass, recomputed values for all generated columns, as
                 ** those values might depend on columns affected by the REPLACE.
                 */
                sqlite3ComputeGeneratedColumns(db, regNewData + 1, pTab);
            }
        } /* end of 2-pass loop */
    } /* end if( has-not-null-constraints ) */

    /* Test all CHECK constraints
     */
    let pCheckT = pTab.borrow().pCheck.clone();
    if pCheckT.is_some() && (db.flags & SQLITE_IgnoreChecks) == 0 {
        let pCheck = db.tree(&pCheckT);
        db.pp().iSelfTab = -(regNewData + 1);
        onError = if overrideError != OE_Default as i32 {
            overrideError
        } else {
            OE_Abort as i32
        };
        let n = db.el(pCheck).nExpr();
        for i in 0..n {
            let pExpr = db.el(pCheck).a[i as usize].pExpr;
            if let Some(a) = aiChng {
                if sqlite3ExprReferencesUpdatedColumn(db, pExpr, a, pkChng as i32) == 0 {
                    /* The check constraints do not reference any of the columns being
                     ** updated so there is no point it verifying the check constraint */
                    continue;
                }
            }
            if !bAffinityDone {
                sqlite3TableAffinity(db.v(), pTab, regNewData + 1);
                bAffinityDone = true;
            }
            let allOk = sqlite3VdbeMakeLabel(db);
            let pCopy = sqlite3ExprDup(db, pExpr, 0);
            if db.mallocFailed == 0 {
                sqlite3ExprIfTrue(db, pCopy, allOk, SQLITE_JUMPIFNULL as i32);
            }
            sqlite3ExprDelete(db, pCopy);
            if onError == OE_Ignore as i32 {
                sqlite3VdbeGoto(db.v(), ignoreDest);
            } else {
                let zName = db.el(pCheck).a[i as usize].zEName.clone();
                debug_assert!(zName.is_some() || db.mallocFailed != 0);
                if onError == OE_Replace as i32 {
                    onError = OE_Abort as i32; /* IMP: R-26383-51744 */
                }
                sqlite3HaltConstraint(
                    db,
                    SQLITE_CONSTRAINT_CHECK,
                    onError,
                    zName.as_deref(),
                    P4_TRANSIENT,
                    P5_ConstraintCheck as u8,
                );
            }
            sqlite3VdbeResolveLabel(db.v(), allOk);
        }
        db.pp().iSelfTab = 0;
    }

    /* UNIQUE and PRIMARY KEY constraints should be handled in the following
     ** order:
     **
     **   (1)  OE_Update
     **   (2)  OE_Abort, OE_Fail, OE_Rollback, OE_Ignore
     **   (3)  OE_Replace
     **
     ** OE_Fail and OE_Ignore must happen before any changes are made.
     ** OE_Update guarantees that only a single row will change, so it
     ** must happen before OE_Replace.  Technically, OE_Abort and OE_Rollback
     ** could happen in any order, but they are grouped up front for
     ** convenience.
     **
     ** 2018-08-14: Ticket https://sqlite.org/src/info/908f001483982c43
     ** The order of constraints used to have OE_Update as (2) and OE_Abort
     ** and so forth as (1). But apparently PostgreSQL checks the OE_Update
     ** constraint before any others, so it had to be moved.
     **
     ** Constraint checking code is generated in this order:
     **   (A)  The rowid constraint
     **   (B)  Unique index constraints that do not have OE_Replace as their
     **        default conflict resolution strategy
     **   (C)  Unique index that do use OE_Replace by default.
     **
     ** The ordering of (2) and (3) is accomplished by making sure the linked
     ** list of indexes attached to a table puts all OE_Replace indexes last
     ** in the list.  See sqlite3CreateIndex() for where that happens.
     */
    /* The IndexIterator of the C code: the indexes to visit, in order, with
     ** their position in the original Table.pIndex list. */
    let mut sIdxIter: Vec<(IndexRef, i32)> = aIdxAll
        .iter()
        .enumerate()
        .map(|(i, x)| (x.clone(), i as i32))
        .collect();
    if pUpsert.nn() {
        if db.arena.ups(pUpsert).pUpsertTarget.is_null() {
            /* There is just on ON CONFLICT clause and it has no constraint-target */
            debug_assert!(db.arena.ups(pUpsert).pNextUpsert.is_null());
            if db.arena.ups(pUpsert).isDoUpdate == 0 {
                /* A single ON CONFLICT DO NOTHING clause, without a constraint-target.
                 ** Make all unique constraint resolution be OE_Ignore */
                overrideError = OE_Ignore as i32;
                pUpsert = UpsertId::NULL;
            } else {
                /* A single ON CONFLICT DO UPDATE.  Make all resolutions OE_Update */
                overrideError = OE_Update as i32;
            }
        } else if nIdxAll != 0 {
            /* Otherwise, we'll need to run the IndexListTerm array version of the
             ** iterator to ensure that all of the ON CONFLICT conditions are
             ** checked first and in order. */
            let nIdx = nIdxAll;
            for jj in 0..nIdx {
                debug_assert!(aRegIdx[jj] > 0);
            }
            let mut bUsed = vec![false; nIdx];
            let mut aIdx: Vec<(IndexRef, i32)> = Vec::with_capacity(nIdx);
            let mut pTerm = pUpsert;
            while pTerm.nn() {
                let (tgt, uidx, next) = {
                    let u = db.arena.ups(pTerm);
                    (u.pUpsertTarget, u.pUpsertIdx.clone(), u.pNextUpsert)
                };
                if tgt.is_null() {
                    break;
                }
                let uidx = match uidx {
                    Some(x) => x,
                    None => {
                        pTerm = next;
                        continue; /* Skip ON CONFLICT for the IPK */
                    }
                };
                let mut jj = 0;
                while jj < nIdx && !Rc::ptr_eq(&aIdxAll[jj], &uidx) {
                    jj += 1;
                }
                if jj < nIdx && !bUsed[jj] {
                    bUsed[jj] = true;
                    aIdx.push((aIdxAll[jj].clone(), jj as i32));
                }
                /* else: Duplicate ON CONFLICT clause ignored */
                pTerm = next;
            }
            for jj in 0..nIdx {
                if bUsed[jj] {
                    continue;
                }
                aIdx.push((aIdxAll[jj].clone(), jj as i32));
            }
            debug_assert!(aIdx.len() == nIdx);
            sIdxIter = aIdx;
        }
    }

    /* Determine if it is possible that triggers (either explicitly coded
     ** triggers or FK resolution actions) might run as a result of deletes
     ** that happen when OE_Replace conflict resolution occurs. (Call these
     ** "replace triggers".)  If any replace triggers run, we will need to
     ** recheck all of the uniqueness constraints after they have all run.
     ** But on the recheck, the resolution is OE_Abort instead of OE_Replace.
     **
     ** If replace triggers are a possibility, then
     **
     **   (1) Allocate register regTrigCnt and initialize it to zero.
     **       That register will count the number of replace triggers that
     **       fire.  Constraint recheck only occurs if the number is positive.
     **   (2) Initialize pTrigger to the list of all DELETE triggers on pTab.
     **   (3) Initialize addrRecheck and lblRecheckOk
     **
     ** The uniqueness rechecking code will create a series of tests to run
     ** in a second pass.  The addrRecheck and lblRecheckOk variables are
     ** used to link together these tests which are separated from each other
     ** in the generate bytecode.
     */
    if (db.flags & (SQLITE_RecTriggers | SQLITE_ForeignKeys)) == 0 {
        /* There are not DELETE triggers nor FK constraints.  No constraint
         ** rechecks are needed. */
        pTrigger = Vec::new();
        regTrigCnt = 0;
    } else {
        if db.flags & SQLITE_RecTriggers != 0 {
            pTrigger = sqlite3TriggersExist(db, pTab, TK_DELETE as i32, ExprListId::NULL, None);
            regTrigCnt = (!pTrigger.is_empty() || sqlite3FkRequired(db, pTab, None, 0) != 0) as i32;
        } else {
            pTrigger = Vec::new();
            regTrigCnt = sqlite3FkRequired(db, pTab, None, 0);
        }
        if regTrigCnt != 0 {
            /* At this point regTrigCnt is non-zero if there are DELETE triggers
             ** or FK triggers. But we only care about these things if there is
             ** a chance that a row will be deleted by an ON CONFLICT REPLACE
             ** constraint. So zero regTrigCnt if no such constraint can be found. */
            if overrideError != OE_Replace as i32 {
                if overrideError != OE_Default as i32 {
                    regTrigCnt = 0;
                } else if pkChng == 0 || pPk.is_some() || keyConf != OE_Replace as i32 {
                    if !aIdxAll.iter().any(|x| x.borrow().onError == OE_Replace) {
                        regTrigCnt = 0;
                    }
                }
            }
        }
        if regTrigCnt != 0 {
            /* Replace triggers might exist.  Allocate the counter and
             ** initialize it to zero. */
            db.pp().nMem += 1;
            regTrigCnt = db.ppr().nMem;
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regTrigCnt);
            lblRecheckOk = sqlite3VdbeMakeLabel(db);
            addrRecheck = lblRecheckOk;
        }
    }

    /* If rowid is changing, make sure the new rowid does not previously
     ** exist in the table.
     */
    if pkChng != 0 && pPk.is_none() {
        let addrRowidOk = sqlite3VdbeMakeLabel(db);

        /* Figure out what action to take in case of a rowid collision */
        onError = keyConf;
        if overrideError != OE_Default as i32 {
            onError = overrideError;
        } else if onError == OE_Default as i32 {
            onError = OE_Abort as i32;
        }

        /* figure out whether or not upsert applies in this case */
        if pUpsert.nn() {
            pUpsertClause = sqlite3UpsertOfIndex(db, pUpsert, None);
            if pUpsertClause.nn() {
                if db.arena.ups(pUpsertClause).isDoUpdate == 0 {
                    onError = OE_Ignore as i32; /* DO NOTHING is the same as INSERT OR IGNORE */
                } else {
                    onError = OE_Update as i32; /* DO UPDATE */
                }
            }
            if pUpsertClause != pUpsert {
                /* The first ON CONFLICT clause has a conflict target other than
                 ** the IPK.  We have to jump ahead to that first ON CONFLICT clause
                 ** and then come back here and deal with the IPK afterwards */
                upsertIpkDelay = sqlite3VdbeAddOp0(db.v(), OP_Goto);
            }
        }

        /* If the response to a rowid conflict is REPLACE but the response
         ** to some other UNIQUE constraint is FAIL or IGNORE, then we need
         ** to defer the running of the rowid conflict checking until after
         ** the UNIQUE constraints have run.
         */
        if onError == OE_Replace as i32      /* IPK rule is REPLACE */
            && onError != overrideError       /* Rules for other constraints are different */
            && nIdxAll != 0
        /* There exist other constraints */
        {
            if upsertIpkDelay != 0 {
                ipkTop = upsertIpkDelay + 1;
                upsertIpkDelay = 0;
            } else {
                ipkTop = sqlite3VdbeAddOp0(db.v(), OP_Goto) + 1;
            }
        }

        if isUpdate {
            /* pkChng!=0 does not mean that the rowid has changed, only that
             ** it might have changed.  Skip the conflict logic below if the rowid
             ** is unchanged. */
            sqlite3VdbeAddOp3(db.v(), OP_Eq, regNewData, addrRowidOk, regOldData);
            sqlite3VdbeChangeP5(db.v(), SQLITE_NOTNULL);
        }

        /* Check to see if the new rowid already exists in the table.  Skip
         ** the following conflict logic if it does not. */
        sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDataCur, addrRowidOk, regNewData);

        if onError == OE_Replace as i32 {
            /* If there are DELETE triggers on this table and the
             ** recursive-triggers flag is set, call GenerateRowDelete() to
             ** remove the conflicting row from the table. This will fire
             ** the triggers and remove both the table and index b-tree entries.
             **
             ** Otherwise, if there are no triggers or the recursive-triggers
             ** flag is not set, but the table has one or more indexes, call
             ** GenerateRowIndexDelete(). This removes the index b-tree entries
             ** only. The table b-tree entry will be replaced by the new entry
             ** when it is inserted.
             **
             ** If either GenerateRowDelete() or GenerateRowIndexDelete() is called,
             ** also invoke MultiWrite() to indicate that this VDBE may require
             ** statement rollback (if the statement is aborted after the delete
             ** takes place). Earlier versions called sqlite3MultiWrite() regardless,
             ** but being more selective here allows statements like:
             **
             **   REPLACE INTO t(rowid) VALUES($newrowid)
             **
             ** to run without a statement journal if there are no indexes on the
             ** table.
             */
            if regTrigCnt != 0 {
                sqlite3MultiWrite(db);
                sqlite3GenerateRowDelete(
                    db, pTab, &pTrigger, iDataCur, iIdxCur, regNewData, 1, 0, OE_Replace, 1, -1,
                );
                sqlite3VdbeAddOp2(db.v(), OP_AddImm, regTrigCnt, 1); /* incr trigger cnt */
                nReplaceTrig += 1;
            } else if nIdxAll != 0 {
                sqlite3MultiWrite(db);
                sqlite3GenerateRowIndexDelete(db, pTab, iDataCur, iIdxCur, None, -1);
            }
            seenReplace = 1;
        } else if onError == OE_Update as i32 {
            sqlite3UpsertDoUpdate(db, pUpsert, pTab, None, iDataCur);
            sqlite3VdbeGoto(db.v(), ignoreDest);
        } else if onError == OE_Ignore as i32 {
            sqlite3VdbeGoto(db.v(), ignoreDest);
        } else {
            if onError != OE_Rollback as i32
                && onError != OE_Abort as i32
                && onError != OE_Fail as i32
            {
                onError = OE_Abort as i32;
            }
            sqlite3RowidConstraint(db, onError, pTab);
        }
        sqlite3VdbeResolveLabel(db.v(), addrRowidOk);
        if ipkTop != 0 {
            ipkBottom = sqlite3VdbeAddOp0(db.v(), OP_Goto);
            sqlite3VdbeJumpHere(db.v(), ipkTop - 1);
        } else if pUpsert.nn() && pUpsertClause != pUpsert {
            upsertIpkReturn = sqlite3VdbeAddOp0(db.v(), OP_Goto);
        }
    }

    /* Test all UNIQUE constraints by creating entries for each UNIQUE
     ** index and making sure that duplicate entries do not already exist.
     ** Compute the revised record entries for indices as we go.
     **
     ** This loop also handles the case of the PRIMARY KEY index for a
     ** WITHOUT ROWID table.
     */
    for (pIdx, ix) in sIdxIter.iter() {
        let ix = *ix;
        if aRegIdx[ix as usize] == 0 {
            continue; /* Skip indices that do not change */
        }
        if pUpsert.nn() {
            pUpsertClause = sqlite3UpsertOfIndex(db, pUpsert, Some(pIdx));
            if upsertIpkDelay != 0 && pUpsertClause == pUpsert {
                sqlite3VdbeJumpHere(db.v(), upsertIpkDelay);
            }
        }
        let addrUniqueOk = sqlite3VdbeMakeLabel(db);
        if !bAffinityDone {
            sqlite3TableAffinity(db.v(), pTab, regNewData + 1);
            bAffinityDone = true;
        }
        let iThisCur = iIdxCur + ix;

        /* Skip partial indices for which the WHERE clause is not true */
        let pw = pIdx.borrow().pPartIdxWhere.clone();
        let hasPart = pw.is_some();
        if hasPart {
            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, aRegIdx[ix as usize]);
            db.pp().iSelfTab = -(regNewData + 1);
            let e = db.tree(&pw);
            sqlite3ExprIfFalseDup(db, e, addrUniqueOk, SQLITE_JUMPIFNULL as i32);
            db.pp().iSelfTab = 0;
        }

        /* Create a record for this index entry as it should appear after
         ** the insert or update.  Store that record in the aRegIdx[ix] register
         */
        let regIdx = aRegIdx[ix as usize] + 1;
        let (nColumn, nKeyCol, idxOnError) = {
            let x = pIdx.borrow();
            (x.nColumn as i32, x.nKeyCol as i32, x.onError as i32)
        };
        let mut colExpr = ExprListId::NULL;
        for i in 0..nColumn {
            let iField = pIdx.borrow().aiColumn[i as usize];
            if iField == XN_EXPR {
                if colExpr.is_null() {
                    let t = pIdx.borrow().aColExpr.clone();
                    colExpr = db.tree(&t);
                }
                db.pp().iSelfTab = -(regNewData + 1);
                let e = db.el(colExpr).a[i as usize].pExpr;
                sqlite3ExprCodeCopy(db, e, regIdx + i);
                db.pp().iSelfTab = 0;
            } else if iField == XN_ROWID || iField as i32 == iPKey {
                let x = regNewData;
                sqlite3VdbeAddOp2(db.v(), OP_IntCopy, x, regIdx + i);
            } else {
                let x = sqlite3TableColumnToStorage(pTab, iField) as i32 + regNewData + 1;
                sqlite3VdbeAddOp2(db.v(), OP_SCopy, x, regIdx + i);
            }
        }
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regIdx, nColumn, aRegIdx[ix as usize]);

        /* In an UPDATE operation, if this index is the PRIMARY KEY index
         ** of a WITHOUT ROWID table and there has been no change the
         ** primary key, then no collision is possible.  The collision detection
         ** logic below can all be skipped. */
        if isUpdate && is_pk(pIdx) && pkChng == 0 {
            sqlite3VdbeResolveLabel(db.v(), addrUniqueOk);
            continue;
        }

        /* Find out what action to take in case there is a uniqueness conflict */
        onError = idxOnError;
        if onError == OE_None as i32 {
            sqlite3VdbeResolveLabel(db.v(), addrUniqueOk);
            continue; /* pIdx is not a UNIQUE index */
        }
        if overrideError != OE_Default as i32 {
            onError = overrideError;
        } else if onError == OE_Default as i32 {
            onError = OE_Abort as i32;
        }

        /* Figure out if the upsert clause applies to this index */
        if pUpsertClause.nn() {
            if db.arena.ups(pUpsertClause).isDoUpdate == 0 {
                onError = OE_Ignore as i32; /* DO NOTHING is the same as INSERT OR IGNORE */
            } else {
                onError = OE_Update as i32; /* DO UPDATE */
            }
        }

        /* Collision detection may be omitted if all of the following are true:
         **   (1) The conflict resolution algorithm is REPLACE
         **   (2) The table is a WITHOUT ROWID table
         **   (3) There are no secondary indexes on the table
         **   (4) No delete triggers need to be fired if there is a conflict
         **   (5) No FK constraint counters need to be updated if a conflict occurs.
         **
         ** This is not possible for ENABLE_PREUPDATE_HOOK builds, as the row
         ** must be explicitly deleted in order to ensure any pre-update hook
         ** is invoked.  */
        debug_assert!(pTab.borrow().IsOrdinaryTable());
        if (ix == 0 && nIdxAll == 1)                       /* Condition 3 */
            && is_pk(pIdx)                                  /* Condition 2 */
            && onError == OE_Replace as i32                 /* Condition 1 */
            && (0 == (db.flags & SQLITE_RecTriggers)        /* Condition 4 */
                || sqlite3TriggersExist(db, pTab, TK_DELETE as i32, ExprListId::NULL, None)
                    .is_empty())
            && (0 == (db.flags & SQLITE_ForeignKeys)        /* Condition 5 */
                || (pTab.borrow().u.tab.pFKey.is_empty()
                    && sqlite3FkReferences(db, pTab).is_empty()))
        {
            sqlite3VdbeResolveLabel(db.v(), addrUniqueOk);
            continue;
        }

        /* Check to see if the new index entry will be unique */
        let mut addrConflictCk = sqlite3VdbeAddOp4Int(
            db.v(),
            OP_NoConflict,
            iThisCur,
            addrUniqueOk,
            regIdx,
            nKeyCol,
        );

        /* Generate code to handle collisions */
        let regR = if is_pk(pIdx) {
            regIdx
        } else {
            sqlite3GetTempRange(db, nPkField)
        };
        if isUpdate || onError == OE_Replace as i32 {
            if hasRowid {
                sqlite3VdbeAddOp2(db.v(), OP_IdxRowid, iThisCur, regR);
                /* Conflict only if the rowid of the existing index entry
                 ** is different from old-rowid */
                if isUpdate {
                    sqlite3VdbeAddOp3(db.v(), OP_Eq, regR, addrUniqueOk, regOldData);
                    sqlite3VdbeChangeP5(db.v(), SQLITE_NOTNULL);
                }
            } else {
                let pk = pPk.as_ref().unwrap();
                let pkNKey = pk.borrow().nKeyCol as i32;
                /* Extract the PRIMARY KEY from the end of the index entry and
                 ** store it in registers regR..regR+nPk-1 */
                if !is_pk(pIdx) {
                    for i in 0..pkNKey {
                        let c = pk.borrow().aiColumn[i as usize];
                        debug_assert!(c >= 0);
                        let x = sqlite3TableColumnToIndex(pIdx, c as i32);
                        sqlite3VdbeAddOp3(db.v(), OP_Column, iThisCur, x, regR + i);
                    }
                }
                if isUpdate {
                    /* If currently processing the PRIMARY KEY of a WITHOUT ROWID
                     ** table, only conflict if the new PRIMARY KEY values are actually
                     ** different from the old.  See TH3 withoutrowid04.test.
                     **
                     ** For a UNIQUE index, only conflict if the PRIMARY KEY values
                     ** of the matched index row are different from the original PRIMARY
                     ** KEY values of this row before the update.  */
                    let mut addrJump = sqlite3VdbeCurrentAddr(db.v()) + pkNKey;
                    let mut op = OP_Ne;
                    let regCmp = if pIdx.borrow().IsPrimaryKeyIndex() {
                        regIdx
                    } else {
                        regR
                    };

                    for i in 0..pkNKey {
                        let (zColl, c) = {
                            let x = pk.borrow();
                            (x.azColl[i as usize].clone(), x.aiColumn[i as usize])
                        };
                        let p4 = sqlite3LocateCollSeq(db, Some(&zColl));
                        debug_assert!(c >= 0);
                        if i == pkNKey - 1 {
                            addrJump = addrUniqueOk;
                            op = OP_Eq;
                        }
                        let x = sqlite3TableColumnToStorage(pTab, c) as i32;
                        sqlite3VdbeAddOp4(
                            db.v(),
                            op,
                            regOldData + 1 + x,
                            addrJump,
                            regCmp + i,
                            match p4 {
                                Some(c) => P4::Coll(c),
                                None => P4::None,
                            },
                            P4_COLLSEQ as i32,
                        );
                        sqlite3VdbeChangeP5(db.v(), SQLITE_NOTNULL);
                    }
                }
            }
        }

        let mut nConflictCk = sqlite3VdbeCurrentAddr(db.v()) - addrConflictCk;
        debug_assert!(nConflictCk > 0 || db.mallocFailed != 0);

        /* Generate code that executes if the new index entry is not unique */
        debug_assert!(
            onError == OE_Rollback as i32
                || onError == OE_Abort as i32
                || onError == OE_Fail as i32
                || onError == OE_Ignore as i32
                || onError == OE_Replace as i32
                || onError == OE_Update as i32
        );
        if onError == OE_Rollback as i32 || onError == OE_Abort as i32 || onError == OE_Fail as i32
        {
            sqlite3UniqueConstraint(db, onError, pIdx);
        } else if onError == OE_Update as i32 {
            sqlite3UpsertDoUpdate(db, pUpsert, pTab, Some(pIdx), iIdxCur + ix);
            sqlite3VdbeGoto(db.v(), ignoreDest);
        } else if onError == OE_Ignore as i32 {
            sqlite3VdbeGoto(db.v(), ignoreDest);
        } else {
            debug_assert!(onError == OE_Replace as i32);
            if regTrigCnt != 0 {
                sqlite3MultiWrite(db);
                nReplaceTrig += 1;
            }
            if !pTrigger.is_empty() && isUpdate {
                sqlite3VdbeAddOp1(db.v(), OP_CursorLock, iDataCur);
            }
            sqlite3GenerateRowDelete(
                db,
                pTab,
                &pTrigger,
                iDataCur,
                iIdxCur,
                regR,
                nPkField as i16,
                0,
                OE_Replace,
                if is_pk(pIdx) {
                    ONEPASS_SINGLE as u8
                } else {
                    ONEPASS_OFF as u8
                },
                iThisCur,
            );
            if !pTrigger.is_empty() && isUpdate {
                sqlite3VdbeAddOp1(db.v(), OP_CursorUnlock, iDataCur);
            }
            seenReplace = 1;
        }
        if regTrigCnt != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_AddImm, regTrigCnt, 1); /* incr trigger cnt */
            let addrBypass = sqlite3VdbeAddOp0(db.v(), OP_Goto); /* Bypass recheck */

            /* Here we insert code that will be invoked after all constraint
             ** checks have run, if and only if one or more replace triggers
             ** fired. */
            sqlite3VdbeResolveLabel(db.v(), lblRecheckOk);
            lblRecheckOk = sqlite3VdbeMakeLabel(db);
            if hasPart {
                /* Bypass the recheck if this partial index is not defined
                 ** for the current row */
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, regIdx - 1, lblRecheckOk);
            }
            /* Copy the constraint check code from above, except change
             ** the constraint-ok jump destination to be the address of
             ** the next retest block */
            while nConflictCk > 0 {
                /* The sqlite3VdbeAddOp4() call might reallocate the opcode array.
                 ** Hence, make a complete copy of the opcode, rather than using
                 ** a pointer to the opcode. */
                let x: VdbeOp = sqlite3VdbeGetOp(db.v(), addrConflictCk).clone();
                if x.opcode != OP_IdxRowid || isUpdate {
                    let p2 = if (sqlite3OpcodeProperty[x.opcode as usize] & OPFLG_JUMP) != 0 {
                        lblRecheckOk
                    } else {
                        x.p2
                    };
                    let v = db.v();
                    sqlite3VdbeAddOp4(v, x.opcode, x.p1, p2, x.p3, x.p4.clone(), x.p4type as i32);
                    sqlite3VdbeChangeP5(v, x.p5);
                }
                nConflictCk -= 1;
                addrConflictCk += 1;
            }
            /* If the retest fails, issue an abort */
            sqlite3UniqueConstraint(db, OE_Abort as i32, pIdx);

            sqlite3VdbeJumpHere(db.v(), addrBypass); /* Terminate the recheck bypass */
        }
        sqlite3VdbeResolveLabel(db.v(), addrUniqueOk);
        if regR != regIdx {
            sqlite3ReleaseTempRange(db, regR, nPkField);
        }
        if pUpsertClause.nn()
            && upsertIpkReturn != 0
            && sqlite3UpsertNextIsIPK(db, pUpsertClause) != 0
        {
            sqlite3VdbeGoto(db.v(), upsertIpkDelay + 1);
            sqlite3VdbeJumpHere(db.v(), upsertIpkReturn);
            upsertIpkReturn = 0;
        }
    }
    let ix = nIdxAll; /* the C iterator leaves ix at the number of indexes */

    /* If the IPK constraint is a REPLACE, run it last */
    if ipkTop != 0 {
        sqlite3VdbeGoto(db.v(), ipkTop);
        debug_assert!(ipkBottom > 0);
        sqlite3VdbeJumpHere(db.v(), ipkBottom);
    }

    /* Recheck all uniqueness constraints after replace triggers have run */
    debug_assert!(regTrigCnt != 0 || nReplaceTrig == 0);
    if nReplaceTrig != 0 {
        sqlite3VdbeAddOp2(db.v(), OP_IfNot, regTrigCnt, lblRecheckOk);
        if pPk.is_none() {
            if isUpdate {
                sqlite3VdbeAddOp3(db.v(), OP_Eq, regNewData, addrRecheck, regOldData);
                sqlite3VdbeChangeP5(db.v(), SQLITE_NOTNULL);
            }
            sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDataCur, addrRecheck, regNewData);
            sqlite3RowidConstraint(db, OE_Abort as i32, pTab);
        } else {
            sqlite3VdbeGoto(db.v(), addrRecheck);
        }
    }
    if regTrigCnt != 0 {
        sqlite3VdbeResolveLabel(db.v(), lblRecheckOk);
    }

    /* Generate the table record */
    if hasRowid {
        let regRec = aRegIdx[ix];
        let nNVCol = pTab.borrow().nNVCol as i32;
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regNewData + 1, nNVCol, regRec);
        sqlite3SetMakeRecordP5(db.v(), pTab);
        if !bAffinityDone {
            sqlite3TableAffinity(db.v(), pTab, 0);
        }
    }

    *pbMayReplace = seenReplace;
}

/// C: `void sqlite3SetMakeRecordP5(Vdbe *v, Table *pTab)`
///
/// Only compiled with SQLITE_ENABLE_NULL_TRIM, which the reference build
/// does not define (sqliteInt.h then makes it an empty macro): no-op.
pub fn sqlite3SetMakeRecordP5(v: &mut Vdbe, pTab: &TableRef) {
    let _ = (v, pTab);
}

/// C: `void sqlite3CompleteInsertion( Parse *pParse, Table *pTab, int iDataCur, int iIdxCur, int regNewData, int *aRegIdx, int update_flags, int appendBias, int useSeekResult )`
///
/// This routine generates code to finish the INSERT or UPDATE operation
/// that was started by a prior call to sqlite3GenerateConstraintChecks.
/// A consecutive range of registers starting at regNewData contains the
/// rowid and the content to be inserted.
///
/// The arguments to this routine should be the same as the first six
/// arguments to sqlite3GenerateConstraintChecks.
pub fn sqlite3CompleteInsertion(
    db: &mut Sqlite3,
    pTab: &TableRef,
    iDataCur: i32,
    iIdxCur: i32,
    regNewData: i32,
    aRegIdx: &[i32],
    update_flags: i32,
    appendBias: i32,
    useSeekResult: i32,
) {
    let mut pik_flags: u16; /* flag values passed to the btree insert */
    debug_assert!(
        update_flags == 0
            || update_flags == OPFLAG_ISUPDATE as i32
            || update_flags == (OPFLAG_ISUPDATE | OPFLAG_SAVEPOSITION) as i32
    );
    debug_assert!(!pTab.borrow().IsView()); /* This table is not a VIEW */
    let hasRowid = pTab.borrow().HasRowid();
    let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
    let mut i = 0usize;
    for pIdx in aIdx.iter() {
        let (hasPart, bHasExpr, isPk, uniqNotNull, nKeyCol, nColumn) = {
            let x = pIdx.borrow();
            (
                x.pPartIdxWhere.is_some(),
                x.bHasExpr != 0,
                x.IsPrimaryKeyIndex(),
                x.uniqNotNull != 0,
                x.nKeyCol as i32,
                x.nColumn as i32,
            )
        };
        if aRegIdx[i] == 0 {
            i += 1;
            continue;
        }
        if hasPart || (update_flags != 0 && bHasExpr) {
            /* If this is a partial index, or an UPDATE of an index on an
             ** expression, then the record register may be set to NULL to indicate
             ** that no record should be inserted into this index. */
            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
            sqlite3VdbeAddOp2(db.v(), OP_IsNull, aRegIdx[i], a);
        }
        pik_flags = if useSeekResult != 0 {
            OPFLAG_USESEEKRESULT as u16
        } else {
            0
        };
        if isPk && !hasRowid {
            pik_flags |= OPFLAG_NCHANGE as u16;
            pik_flags |= (update_flags & OPFLAG_SAVEPOSITION as i32) as u16;
            /* codeWithoutRowidPreupdate(): only with SQLITE_ENABLE_PREUPDATE_HOOK */
        }
        sqlite3VdbeAddOp4Int(
            db.v(),
            OP_IdxInsert,
            iIdxCur + i as i32,
            aRegIdx[i],
            aRegIdx[i] + 1,
            if uniqNotNull { nKeyCol } else { nColumn },
        );
        sqlite3VdbeChangeP5(db.v(), pik_flags);
        i += 1;
    }
    if !hasRowid {
        return;
    }
    if db.ppr().nested != 0 {
        pik_flags = 0;
    } else {
        pik_flags = OPFLAG_NCHANGE as u16;
        pik_flags |= if update_flags != 0 {
            update_flags as u16
        } else {
            OPFLAG_LASTROWID as u16
        };
    }
    if appendBias != 0 {
        pik_flags |= OPFLAG_APPEND as u16;
    }
    if useSeekResult != 0 {
        pik_flags |= OPFLAG_USESEEKRESULT as u16;
    }
    sqlite3VdbeAddOp3(db.v(), OP_Insert, iDataCur, aRegIdx[i], regNewData);
    if db.ppr().nested == 0 {
        sqlite3VdbeAppendP4(db.v(), P4::Tab(pTab.clone()), P4_TABLE as i32);
    }
    sqlite3VdbeChangeP5(db.v(), pik_flags);
}

/// C: `int sqlite3OpenTableAndIndices( Parse *pParse, Table *pTab, int op, u8 p5, int iBase, u8 *aToOpen, int *piDataCur, int *piIdxCur )`
///
/// Allocate cursors for the pTab table and all its indices and generate
/// code to open and initialized those cursors.
///
/// The cursor for the object that contains the complete data (normally
/// the table itself, but the PRIMARY KEY index in the case of a WITHOUT
/// ROWID table) is returned in *piDataCur.  The first index cursor is
/// returned in *piIdxCur.  The number of indices is returned.
///
/// Use iBase as the first cursor (either the *piDataCur for rowid tables
/// or the first index for WITHOUT ROWID tables) if it is non-negative.
/// If iBase is negative, then allocate the next available cursor.
///
/// For a rowid table, *piDataCur will be exactly one less than *piIdxCur.
/// For a WITHOUT ROWID table, *piDataCur will be somewhere in the range
/// of *piIdxCurs, depending on where the PRIMARY KEY index appears on the
/// pTab->pIndex list.
///
/// If pTab is a virtual table, then this routine is a no-op and the
/// *piDataCur and *piIdxCur values are left uninitialized.
///
/// Rust: `aToOpen` is None for the C NULL pointer.
pub fn sqlite3OpenTableAndIndices(
    db: &mut Sqlite3,
    pTab: &TableRef,
    op: i32,
    p5: u8,
    iBase: i32,
    aToOpen: Option<&[u8]>,
    piDataCur: &mut i32,
    piIdxCur: &mut i32,
) -> i32 {
    let mut p5 = p5;
    let mut iBase = iBase;
    debug_assert!(op == OP_OpenRead as i32 || op == OP_OpenWrite as i32);
    debug_assert!(op == OP_OpenWrite as i32 || p5 == 0);
    if pTab.borrow().IsVirtual() {
        /* This routine is a no-op for virtual tables. Leave the output
         ** variables *piDataCur and *piIdxCur set to illegal cursor numbers
         ** for improved error detection. */
        *piDataCur = -999;
        *piIdxCur = -999;
        return 0;
    }
    let iDb = db.schema_index(pTab.borrow().pSchema);
    if iBase < 0 {
        iBase = db.ppr().nTab;
    }
    let iDataCur = iBase;
    iBase += 1;
    *piDataCur = iDataCur;
    let (hasRowid, tnum, zName) = {
        let t = pTab.borrow();
        (t.HasRowid(), t.tnum, t.zName.clone())
    };
    if hasRowid && aToOpen.map_or(true, |a| a[0] != 0) {
        sqlite3OpenTable(db, iDataCur, iDb, pTab, op);
    } else if db.noSharedCache == 0 {
        sqlite3TableLock(
            db,
            iDb,
            tnum,
            (op == OP_OpenWrite as i32) as u8,
            Some(&zName),
        );
    }
    *piIdxCur = iBase;
    let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
    let mut i = 0usize;
    for pIdx in aIdx.iter() {
        let iIdxCur = iBase;
        iBase += 1;
        let (isPk, itnum) = {
            let x = pIdx.borrow();
            (x.IsPrimaryKeyIndex(), x.tnum)
        };
        if isPk && !hasRowid {
            *piDataCur = iIdxCur;
            p5 = 0;
        }
        if aToOpen.map_or(true, |a| a[i + 1] != 0) {
            sqlite3VdbeAddOp3(db.v(), op as u8, iIdxCur, itnum as i32, iDb);
            sqlite3VdbeSetP4KeyInfo(db, pIdx);
            sqlite3VdbeChangeP5(db.v(), p5 as u16);
        }
        i += 1;
    }
    if iBase > db.ppr().nTab {
        db.pp().nTab = iBase;
    }
    i as i32
}

/// Check to see if index pSrc is compatible as a source of data
/// for index pDest in an insert transfer optimization.  The rules
/// for a compatible index:
///
///    *   The index is over the same set of columns
///    *   The same DESC and ASC markings occurs on all columns
///    *   The same onError processing (OE_Abort, OE_Ignore, etc)
///    *   The same collating sequence on each column
///    *   The index has the exact same WHERE clause
fn xferCompatibleIndex(db: &mut Sqlite3, pDest: &IndexRef, pSrc: &IndexRef) -> bool {
    let (dk, dc, de) = {
        let d = pDest.borrow();
        (d.nKeyCol, d.nColumn, d.onError)
    };
    let (sk, sc, se) = {
        let s = pSrc.borrow();
        (s.nKeyCol, s.nColumn, s.onError)
    };
    if dk != sk || dc != sc {
        return false; /* Different number of columns */
    }
    if de != se {
        return false; /* Different conflict resolution strategies */
    }
    let mut srcColExpr = ExprListId::NULL;
    let mut destColExpr = ExprListId::NULL;
    for i in 0..sk as usize {
        let (sa, da) = (pSrc.borrow().aiColumn[i], pDest.borrow().aiColumn[i]);
        if sa != da {
            return false; /* Different columns indexed */
        }
        if sa == XN_EXPR {
            if srcColExpr.is_null() {
                let t = pSrc.borrow().aColExpr.clone();
                srcColExpr = db.tree(&t);
                let t = pDest.borrow().aColExpr.clone();
                destColExpr = db.tree(&t);
            }
            debug_assert!(srcColExpr.nn() && destColExpr.nn());
            let a = db.el(srcColExpr).a[i].pExpr;
            let b = db.el(destColExpr).a[i].pExpr;
            // C passes pParse==0.
            if sqlite3ExprCompare(db, false, a, b, -1) != 0 {
                return false; /* Different expressions in the index */
            }
        }
        if pSrc.borrow().aSortOrder[i] != pDest.borrow().aSortOrder[i] {
            return false; /* Different sort orders */
        }
        let (sColl, dColl) = (
            pSrc.borrow().azColl[i].clone(),
            pDest.borrow().azColl[i].clone(),
        );
        if sqlite3_stricmp(Some(&sColl), Some(&dColl)) != 0 {
            return false; /* Different collating sequences */
        }
    }
    let sw = pSrc.borrow().pPartIdxWhere.clone();
    let dw = pDest.borrow().pPartIdxWhere.clone();
    let sw = db.tree(&sw);
    let dw = db.tree(&dw);
    if sqlite3ExprCompare(db, false, sw, dw, -1) != 0 {
        return false; /* Different WHERE clauses */
    }

    /* If no test above fails then the indices must be compatible */
    true
}

/// Examine an expression node and abort if it references the ROWID.
/// This is a Walker callback used by xferCompatibleCheck()
fn xferCheckRowid(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let e = w.db.ex(pExpr);
    if e.op == TK_COLUMN && e.iColumn < 0 {
        w.eCode = 1;
        WRC_Abort
    } else {
        WRC_Continue
    }
}

/// Analyze CHECK constraints on the source and destination tables and
/// return true if those CHECK constraints are compatible with the
/// xfer-optimization.
///
///    *  The pDest and pSrc tables must have identical CHECK constraints.
///
///    *  If the destination table, pDest, does not have an
///       INTEGER PRIMARY KEY column, then no CHECK constraint may
///       referenced the ROWID.  (See forum post 2026-05-11T13:15:57Z)
fn xferCompatibleCheck(db: &mut Sqlite3, pDest: &TableRef, pSrc: &TableRef) -> bool {
    let sc = pSrc.borrow().pCheck.clone();
    let dc = pDest.borrow().pCheck.clone();
    let sc = db.tree(&sc);
    let dc = db.tree(&dc);
    if sqlite3ExprListCompare(db, sc, dc, -1) != 0 {
        return false;
    }
    if pDest.borrow().iPKey < 0 {
        let mut w = Walker::new(db);
        w.xExprCallback = Some(xferCheckRowid);
        sqlite3WalkExprList(&mut w, dc);
        if w.eCode != 0 {
            return false;
        }
    }
    true
}

/// Attempt the transfer optimization on INSERTs of the form
///
///     INSERT INTO tab1 SELECT * FROM tab2;
///
/// The xfer optimization transfers raw records from tab2 over to tab1.
/// Columns are not decoded and reassembled, which greatly improves
/// performance.  Raw index records are transferred in the same way.
///
/// The xfer optimization is only attempted if tab1 and tab2 are compatible.
/// There are lots of rules for determining compatibility - see comments
/// embedded in the code for details.
///
/// This routine returns TRUE if the optimization is guaranteed to be used.
/// Sometimes the xfer optimization will only work if the destination table
/// is empty - a factor that can only be determined at run-time.  In that
/// case, this routine generates code for the xfer optimization but also
/// does a test to see if the destination table is empty and jumps over the
/// xfer optimization code if the test fails.  In that case, this routine
/// returns FALSE so that the caller will know to go ahead and generate
/// an unoptimized transfer.  This routine also returns FALSE if there
/// is no chance that the xfer optimization can be applied.
///
/// This optimization is particularly useful at making VACUUM run faster.
fn xferOptimization(
    db: &mut Sqlite3,
    pDest: &TableRef,
    pSelect: SelectId,
    onError: i32,
    iDbDest: i32,
) -> i32 {
    let mut onError = onError;
    let mut emptyDestTest = 0; /* Address of test for empty pDest */
    let mut emptySrcTest = 0; /* Address of test for empty pSrc */
    let mut destHasUniqueIdx = false; /* True if pDest has a UNIQUE index */

    debug_assert!(pSelect.nn());
    if db.ppr().pWith.nn() || db.sel(pSelect).pWith.nn() {
        /* Do not attempt to process this query if there are an WITH clauses
         ** attached to it. Proceeding may generate a false "no such table: xxx"
         ** error if pSelect reads from a CTE named "xxx".  */
        return 0;
    }
    if pDest.borrow().IsVirtual() {
        return 0; /* tab1 must not be a virtual table */
    }
    if onError == OE_Default as i32 {
        let (iPKey, keyConf) = {
            let d = pDest.borrow();
            (d.iPKey, d.keyConf)
        };
        if iPKey >= 0 {
            onError = keyConf as i32;
        }
        if onError == OE_Default as i32 {
            onError = OE_Abort as i32;
        }
    }
    let s = db.sel(pSelect).clone();
    debug_assert!(s.pSrc.nn()); /* allocated even if there is no FROM clause */
    if db.src(s.pSrc).nSrc() != 1 {
        return 0; /* FROM clause must have exactly one term */
    }
    if db.src(s.pSrc).a[0].fg.isSubquery != 0 {
        return 0; /* FROM clause cannot contain a subquery */
    }
    if s.pWhere.nn() {
        return 0; /* SELECT may not have a WHERE clause */
    }
    if s.pOrderBy.nn() {
        return 0; /* SELECT may not have an ORDER BY clause */
    }
    /* Do not need to test for a HAVING clause.  If HAVING is present but
     ** there is no ORDER BY, we will get an error. */
    if s.pGroupBy.nn() {
        return 0; /* SELECT may not have a GROUP BY clause */
    }
    if s.pLimit.nn() {
        return 0; /* SELECT may not have a LIMIT clause */
    }
    if s.pPrior.nn() {
        return 0; /* SELECT may not be a compound query */
    }
    if s.selFlags & SF_Distinct != 0 {
        return 0; /* SELECT may not be DISTINCT */
    }
    let pEList = s.pEList;
    debug_assert!(pEList.nn());
    if db.el(pEList).nExpr() != 1 {
        return 0; /* The result set must have exactly one column */
    }
    let e0 = db.el(pEList).a[0].pExpr;
    if db.ex(e0).op != TK_ASTERISK {
        return 0; /* The result set must be the special operator "*" */
    }

    /* At this point we have established that the statement is of the
     ** correct syntactic form to participate in this optimization.  Now
     ** we have to check the semantics.
     */
    let pItem = SrcItemId(s.pSrc, 0);
    let pSrc = match sqlite3LocateTableItem(db, 0, pItem) {
        Some(t) => t,
        None => return 0, /* FROM clause does not contain a real table */
    };
    {
        let sr = pSrc.borrow();
        let de = pDest.borrow();
        if sr.tnum == de.tnum && sr.pSchema == de.pSchema {
            return 0; /* tab1 and tab2 may not be the same table */
        }
        if de.HasRowid() != sr.HasRowid() {
            return 0; /* source and destination must both be WITHOUT ROWID or not */
        }
        if !sr.IsOrdinaryTable() {
            return 0; /* tab2 may not be a view or virtual table */
        }
        if de.nCol != sr.nCol {
            return 0; /* Number of columns must be the same in tab1 and tab2 */
        }
        if de.iPKey != sr.iPKey {
            return 0; /* Both tables must have the same INTEGER PRIMARY KEY */
        }
        if (de.tabFlags & TF_Strict) != 0 {
            if (sr.tabFlags & TF_Strict) == 0 {
                return 0; /* Cannot feed from a non-strict into a strict table */
            }
            for i in 0..de.nCol as usize {
                let eDestType = de.aCol[i].eCType;
                let eSrcType = sr.aCol[i].eCType;
                if eDestType == COLTYPE_ANY {
                    continue;
                }
                if eDestType == eSrcType {
                    continue;
                }
                if eDestType == COLTYPE_INT && eSrcType == COLTYPE_INTEGER {
                    continue;
                }
                if eDestType == COLTYPE_INTEGER && eSrcType == COLTYPE_INT {
                    continue;
                }
                return 0; /* Incompatible types in source and destination */
            }
        }
    }
    let nCol = pDest.borrow().nCol as usize;
    for i in 0..nCol {
        let mut pDestCol = pDest.borrow().aCol[i].clone();
        let mut pSrcCol = pSrc.borrow().aCol[i].clone();
        /* Even if tables t1 and t2 have identical schemas, if they contain
         ** generated columns, then this statement is semantically incorrect:
         **
         **     INSERT INTO t2 SELECT * FROM t1;
         **
         ** The reason is that generated column values are returned by the
         ** the SELECT statement on the right but the INSERT statement on the
         ** left wants them to be omitted.
         **
         ** Nevertheless, this is a useful notational shorthand to tell SQLite
         ** to do a bulk transfer all of the content from t1 over to t2.
         **
         ** We could, in theory, disable this (except for internal use by the
         ** VACUUM command where it is actually needed).  But why do that?  It
         ** seems harmless enough, and provides a useful service.
         */
        if (pDestCol.colFlags & COLFLAG_GENERATED) != (pSrcCol.colFlags & COLFLAG_GENERATED) {
            return 0; /* Both columns have the same generated-column type */
        }
        /* But the transfer is only allowed if both the source and destination
         ** tables have the exact same expressions for generated columns.
         ** This requirement could be relaxed for VIRTUAL columns, I suppose.
         */
        if (pDestCol.colFlags & COLFLAG_GENERATED) != 0 {
            let a = sqlite3ColumnExpr(db, &pSrc, i);
            let b = sqlite3ColumnExpr(db, pDest, i);
            if sqlite3ExprCompare(db, false, a, b, -1) != 0 {
                return 0; /* Different generator expressions */
            }
        }
        if pDestCol.affinity != pSrcCol.affinity {
            return 0; /* Affinity must be the same on all columns */
        }
        let dColl = sqlite3ColumnColl(&mut pDestCol);
        let sColl = sqlite3ColumnColl(&mut pSrcCol);
        if sqlite3_stricmp(dColl.as_deref(), sColl.as_deref()) != 0 {
            return 0; /* Collating sequence must be the same on all columns */
        }
        if pDestCol.notNull != 0 && pSrcCol.notNull == 0 {
            return 0; /* tab2 must be NOT NULL if tab1 is */
        }
        /* Default values for second and subsequent columns need to match. */
        if (pDestCol.colFlags & COLFLAG_GENERATED) == 0 && i > 0 {
            let pDestExpr = sqlite3ColumnExpr(db, pDest, i);
            let pSrcExpr = sqlite3ColumnExpr(db, &pSrc, i);
            debug_assert!(pDestExpr.is_null() || db.ex(pDestExpr).op == TK_SPAN);
            debug_assert!(pSrcExpr.is_null() || db.ex(pSrcExpr).op == TK_SPAN);
            if pDestExpr.is_null() != pSrcExpr.is_null()
                || (pDestExpr.nn() && db.ex(pDestExpr).token() != db.ex(pSrcExpr).token())
            {
                return 0; /* Default values must be the same for all columns */
            }
        }
    }
    let aDestIdx: Vec<IndexRef> = pDest.borrow().pIndex.clone();
    let aSrcIdx: Vec<IndexRef> = pSrc.borrow().pIndex.clone();
    let sameSchema = pSrc.borrow().pSchema == pDest.borrow().pSchema;
    for pDestIdx in aDestIdx.iter() {
        if pDestIdx.borrow().IsUniqueIndex() {
            destHasUniqueIdx = true;
        }
        let mut found: Option<IndexRef> = None;
        for pSrcIdx in aSrcIdx.iter() {
            if xferCompatibleIndex(db, pDestIdx, pSrcIdx) {
                found = Some(pSrcIdx.clone());
                break;
            }
        }
        let pSrcIdx = match found {
            Some(x) => x,
            None => return 0, /* pDestIdx has no corresponding index in pSrc */
        };
        if pSrcIdx.borrow().tnum == pDestIdx.borrow().tnum
            && sameSchema
            && sqlite3FaultSim(411) == SQLITE_OK
        {
            /* The sqlite3FaultSim() call allows this corruption test to be
             ** bypassed during testing, in order to exercise other corruption tests
             ** further downstream. */
            return 0; /* Corrupt schema - two indexes on the same btree */
        }
    }
    if pDest.borrow().pCheck.is_some()
        && (db.mDbFlags & DBFLAG_Vacuum) == 0
        && !xferCompatibleCheck(db, pDest, &pSrc)
    {
        return 0; /* Tables have different CHECK constraints.  Ticket #2252 */
    }
    /* Disallow the transfer optimization if the destination table contains
     ** any foreign key constraints.  This is more restrictive than necessary.
     ** But the main beneficiary of the transfer optimization is the VACUUM
     ** command, and the VACUUM command disables foreign key constraints.  So
     ** the extra complication to make this rule less restrictive is probably
     ** not worth the effort.  Ticket [6284df89debdfa61db8073e062908af0c9b6118e]
     */
    debug_assert!(pDest.borrow().IsOrdinaryTable());
    if (db.flags & SQLITE_ForeignKeys) != 0 && !pDest.borrow().u.tab.pFKey.is_empty() {
        return 0;
    }
    if (db.flags & SQLITE_CountRows) != 0 {
        return 0; /* xfer opt does not play well with PRAGMA count_changes */
    }
    if db.xAuth.is_some() {
        let iDb = db.schema_index(pSrc.borrow().pSchema);
        if sqlite3AuthCheck(db, SQLITE_SELECT, None, None, None) != 0 {
            return 0;
        }
        let (zName, aNames) = {
            let sr = pSrc.borrow();
            (
                sr.zName.clone(),
                sr.aCol
                    .iter()
                    .map(|c| c.zCnName.clone())
                    .collect::<Vec<_>>(),
            )
        };
        for zCol in aNames.iter() {
            if sqlite3AuthReadCol(db, Some(&zName), Some(zCol), iDb) != 0 {
                return 0;
            }
        }
    }

    /* If we get this far, it means that the xfer optimization is at
     ** least a possibility, though it might only work if the destination
     ** table (tab1) is initially empty.
     */
    let iDbSrc = db.schema_index(pSrc.borrow().pSchema);
    sqlite3GetVdbe(db);
    sqlite3CodeVerifySchema(db, iDbSrc);
    let iSrc = db.ppr().nTab;
    db.pp().nTab += 1;
    let iDest = db.ppr().nTab;
    db.pp().nTab += 1;
    let regAutoinc = autoIncBegin(db, iDbDest, pDest);
    let regData = sqlite3GetTempReg(db);
    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regData);
    let regRowid = sqlite3GetTempReg(db);
    sqlite3OpenTable(db, iDest, iDbDest, pDest, OP_OpenWrite as i32);
    let (dIPKey, dHasIndex, dHasRowid, dTnum, dName) = {
        let d = pDest.borrow();
        (
            d.iPKey,
            !d.pIndex.is_empty(),
            d.HasRowid(),
            d.tnum,
            d.zName.clone(),
        )
    };
    debug_assert!(dHasRowid || destHasUniqueIdx);
    if (db.mDbFlags & DBFLAG_Vacuum) == 0
        && ((dIPKey < 0 && dHasIndex)                          /* (1) */
            || destHasUniqueIdx                                 /* (2) */
            || (onError != OE_Abort as i32 && onError != OE_Rollback as i32))
    /* (3) */
    {
        /* In some circumstances, we are able to run the xfer optimization
         ** only if the destination table is initially empty. Unless the
         ** DBFLAG_Vacuum flag is set, this block generates code to make
         ** that determination. If DBFLAG_Vacuum is set, then the destination
         ** table is always empty.
         **
         ** Conditions under which the destination must be empty:
         **
         ** (1) There is no INTEGER PRIMARY KEY but there are indices.
         **     (If the destination is not initially empty, the rowid fields
         **     of index entries might need to change.)
         **
         ** (2) The destination has a unique index.  (The xfer optimization
         **     is unable to test uniqueness.)
         **
         ** (3) onError is something other than OE_Abort and OE_Rollback.
         */
        let addr1 = sqlite3VdbeAddOp2(db.v(), OP_Rewind, iDest, 0);
        emptyDestTest = sqlite3VdbeAddOp0(db.v(), OP_Goto);
        sqlite3VdbeJumpHere(db.v(), addr1);
    }
    let (sHasRowid, sTnum, sName) = {
        let sr = pSrc.borrow();
        (sr.HasRowid(), sr.tnum, sr.zName.clone())
    };
    if sHasRowid {
        let insFlags: u16;
        let addr1;
        sqlite3OpenTable(db, iSrc, iDbSrc, &pSrc, OP_OpenRead as i32);
        emptySrcTest = sqlite3VdbeAddOp2(db.v(), OP_Rewind, iSrc, 0);
        if dIPKey >= 0 {
            addr1 = sqlite3VdbeAddOp2(db.v(), OP_Rowid, iSrc, regRowid);
            if (db.mDbFlags & DBFLAG_Vacuum) == 0 {
                let addr2 = sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDest, 0, regRowid);
                sqlite3RowidConstraint(db, onError, pDest);
                sqlite3VdbeJumpHere(db.v(), addr2);
            }
            autoIncStep(db, regAutoinc, regRowid);
        } else if !dHasIndex && (db.mDbFlags & DBFLAG_VacuumInto) == 0 {
            addr1 = sqlite3VdbeAddOp2(db.v(), OP_NewRowid, iDest, regRowid);
        } else {
            addr1 = sqlite3VdbeAddOp2(db.v(), OP_Rowid, iSrc, regRowid);
            debug_assert!((pDest.borrow().tabFlags & TF_Autoincrement) == 0);
        }

        if db.mDbFlags & DBFLAG_Vacuum != 0 {
            sqlite3VdbeAddOp1(db.v(), OP_SeekEnd, iDest);
            insFlags = (OPFLAG_APPEND | OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT) as u16;
        } else {
            insFlags =
                (OPFLAG_NCHANGE | OPFLAG_LASTROWID | OPFLAG_APPEND | OPFLAG_PREFORMAT) as u16;
        }
        sqlite3VdbeAddOp3(db.v(), OP_RowCell, iDest, iSrc, regRowid);
        sqlite3VdbeAddOp3(db.v(), OP_Insert, iDest, regData, regRowid);
        if (db.mDbFlags & DBFLAG_Vacuum) == 0 {
            sqlite3VdbeChangeP4(db.v(), -1, P4::Tab(pDest.clone()), P4_TABLE as i32);
        }
        sqlite3VdbeChangeP5(db.v(), insFlags);

        sqlite3VdbeAddOp2(db.v(), OP_Next, iSrc, addr1);
        sqlite3VdbeAddOp2(db.v(), OP_Close, iSrc, 0);
        sqlite3VdbeAddOp2(db.v(), OP_Close, iDest, 0);
    } else {
        sqlite3TableLock(db, iDbDest, dTnum, 1, Some(&dName));
        sqlite3TableLock(db, iDbSrc, sTnum, 0, Some(&sName));
    }
    for pDestIdx in aDestIdx.iter() {
        let mut idxInsFlags: u16 = 0;
        let mut found: Option<IndexRef> = None;
        for pSrcIdx in aSrcIdx.iter() {
            if xferCompatibleIndex(db, pDestIdx, pSrcIdx) {
                found = Some(pSrcIdx.clone());
                break;
            }
        }
        let pSrcIdx = found.expect("no compatible source index");
        let stnum = pSrcIdx.borrow().tnum;
        sqlite3VdbeAddOp3(db.v(), OP_OpenRead, iSrc, stnum as i32, iDbSrc);
        sqlite3VdbeSetP4KeyInfo(db, &pSrcIdx);
        let dtnum = pDestIdx.borrow().tnum;
        sqlite3VdbeAddOp3(db.v(), OP_OpenWrite, iDest, dtnum as i32, iDbDest);
        sqlite3VdbeSetP4KeyInfo(db, pDestIdx);
        sqlite3VdbeChangeP5(db.v(), OPFLAG_BULKCSR as u16);
        let addr1 = sqlite3VdbeAddOp2(db.v(), OP_Rewind, iSrc, 0);
        if db.mDbFlags & DBFLAG_Vacuum != 0 {
            /* This INSERT command is part of a VACUUM operation, which guarantees
             ** that the destination table is empty. If all indexed columns use
             ** collation sequence BINARY, then it can also be assumed that the
             ** index will be populated by inserting keys in strictly sorted
             ** order. In this case, instead of seeking within the b-tree as part
             ** of every OP_IdxInsert opcode, an OP_SeekEnd is added before the
             ** OP_IdxInsert to seek to the point within the b-tree where each key
             ** should be inserted. This is faster.
             **
             ** If any of the indexed columns use a collation sequence other than
             ** BINARY, this optimization is disabled. This is because the user
             ** might change the definition of a collation sequence and then run
             ** a VACUUM command. In that case keys may not be written in strictly
             ** sorted order.  */
            let allBinary = {
                let x = pSrcIdx.borrow();
                let n = x.nColumn as usize;
                x.azColl
                    .iter()
                    .take(n)
                    .all(|z| sqlite3_stricmp(Some(sqlite3StrBINARY), Some(z)) == 0)
            };
            if allBinary {
                idxInsFlags = (OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT) as u16;
                sqlite3VdbeAddOp1(db.v(), OP_SeekEnd, iDest);
                sqlite3VdbeAddOp2(db.v(), OP_RowCell, iDest, iSrc);
            }
        } else if !sHasRowid && pDestIdx.borrow().idxType == SQLITE_IDXTYPE_PRIMARYKEY {
            idxInsFlags |= OPFLAG_NCHANGE as u16;
        }
        if idxInsFlags != (OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT) as u16 {
            sqlite3VdbeAddOp3(db.v(), OP_RowData, iSrc, regData, 1);
            /* codeWithoutRowidPreupdate(): only with SQLITE_ENABLE_PREUPDATE_HOOK */
        }
        sqlite3VdbeAddOp2(db.v(), OP_IdxInsert, iDest, regData);
        sqlite3VdbeChangeP5(db.v(), idxInsFlags | OPFLAG_APPEND as u16);
        sqlite3VdbeAddOp2(db.v(), OP_Next, iSrc, addr1 + 1);
        sqlite3VdbeJumpHere(db.v(), addr1);
        sqlite3VdbeAddOp2(db.v(), OP_Close, iSrc, 0);
        sqlite3VdbeAddOp2(db.v(), OP_Close, iDest, 0);
    }
    if emptySrcTest != 0 {
        sqlite3VdbeJumpHere(db.v(), emptySrcTest);
    }
    sqlite3ReleaseTempReg(db, regRowid);
    sqlite3ReleaseTempReg(db, regData);
    if emptyDestTest != 0 {
        sqlite3AutoincrementEnd(db);
        sqlite3VdbeAddOp2(db.v(), OP_Halt, SQLITE_OK, 0);
        sqlite3VdbeJumpHere(db.v(), emptyDestTest);
        sqlite3VdbeAddOp2(db.v(), OP_Close, iDest, 0);
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(aff: u8, flags: u16) -> Column {
        Column {
            affinity: aff,
            colFlags: flags,
            ..Default::default()
        }
    }

    #[test]
    fn table_affinity_string() {
        let mut t = Table::default();
        t.aCol = vec![
            col(SQLITE_AFF_INTEGER, 0),
            col(SQLITE_AFF_TEXT, COLFLAG_VIRTUAL),
            col(SQLITE_AFF_REAL, 0),
            col(SQLITE_AFF_BLOB, 0),
            col(SQLITE_AFF_NONE, 0),
        ];
        t.nCol = 5;
        /* virtual columns are skipped, trailing BLOB/NONE affinities dropped */
        assert_eq!(
            tableAffinityStr(&t),
            vec![SQLITE_AFF_INTEGER, SQLITE_AFF_REAL]
        );
        t.aCol = vec![col(SQLITE_AFF_BLOB, 0)];
        t.nCol = 1;
        assert!(tableAffinityStr(&t).is_empty());
    }
}
