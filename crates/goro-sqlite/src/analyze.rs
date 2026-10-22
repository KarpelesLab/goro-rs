//! Port of analyze.c (without SQLITE_ENABLE_STAT4): the ANALYZE command,
//! the stat_init()/stat_push()/stat_get() internal functions and the
//! loading of sqlite_stat1 into the in-memory schema.
//!
//! Rust notes: the StatAccum object that stat_init() returns (in C, as a
//! BLOB that is really a pointer) is a zero-length BLOB whose `Mem::pPtr`
//! holds an `Rc<RefCell<StatAccum>>`; the VDBE passes registers to the
//! functions with `Mem::pPtr` intact (Mem::clone shares it).

use crate::build::*;
use crate::int::*;
use crate::vdbeapi::*;
use crate::vdbeaux::*;
use std::cell::RefCell;
use std::rc::Rc;

/// This routine generates code that opens the sqlite_statN tables.
/// The sqlite_stat1 table is always relevant.  sqlite_stat2 is now
/// obsolete.  sqlite_stat3 and sqlite_stat4 are only opened when
/// appropriate compile-time options are provided.
///
/// If the sqlite_statN tables do not previously exist, it is created.
///
/// Argument zWhere may be a pointer to a buffer containing a table name,
/// or it may be a NULL pointer. If it is not NULL, then all entries in
/// the sqlite_statN tables associated with the named table are deleted.
/// If zWhere==0, then code is generated to delete all stat table entries.
fn openStatTable(
    db: &mut Sqlite3,
    iDb: i32,
    iStatCur: i32,
    zWhere: Option<&[u8]>,
    zWhereType: Option<&str>,
) {
    struct StatTab {
        zName: &'static str,
        zCols: Option<&'static str>,
    }
    const aTable: [StatTab; 3] = [
        StatTab {
            zName: "sqlite_stat1",
            zCols: Some("tbl,idx,stat"),
        },
        StatTab {
            zName: "sqlite_stat4",
            zCols: None,
        },
        StatTab {
            zName: "sqlite_stat3",
            zCols: None,
        },
    ];
    let mut aRoot = [0u32; 3];
    let mut aCreateTbl = [0u16; 3];
    let nToOpen = 1usize;

    if !crate::select::sqlite3GetVdbe(db) {
        return;
    }
    let zDbSName = db.aDb[iDb as usize].zDbSName.clone();

    // Create new statistic tables if they do not exist, or clear them
    // if they do already exist.
    for i in 0..aTable.len() {
        let zTab = aTable[i].zName;
        aCreateTbl[i] = 0;
        match sqlite3FindTable(db, Some(zTab.as_bytes()), zDbSName.as_deref()) {
            None => {
                if i < nToOpen {
                    // The sqlite_statN table does not exist. Create it. Note
                    // that a side-effect of the CREATE TABLE statement is to
                    // leave the rootpage of the new table in register
                    // pParse->regRoot. This is important because the
                    // OpenWrite opcode below will be needing it.
                    sqlite3NestedParse(
                        db,
                        Some(b"CREATE TABLE %Q.%s(%s)"),
                        &[
                            crate::printf::PfArg::from(zDbSName.as_deref()),
                            crate::printf::PfArg::from(zTab),
                            crate::printf::PfArg::from(aTable[i].zCols.unwrap_or("")),
                        ],
                    );
                    aRoot[i] = db.ppr().u1.cr.regRoot as u32;
                    aCreateTbl[i] = OPFLAG_P2ISREG;
                }
            }
            Some(pStat) => {
                // The table already exists. If zWhere is not NULL, delete all
                // entries associated with the table zWhere. If zWhere is
                // NULL, delete the entire contents of the table.
                aRoot[i] = pStat.borrow().tnum;
                sqlite3TableLock(db, iDb, aRoot[i], 1, Some(zTab.as_bytes()));
                if let Some(zWhere) = zWhere {
                    sqlite3NestedParse(
                        db,
                        Some(b"DELETE FROM %Q.%s WHERE %s=%Q"),
                        &[
                            crate::printf::PfArg::from(zDbSName.as_deref()),
                            crate::printf::PfArg::from(zTab),
                            crate::printf::PfArg::from(zWhereType.unwrap_or("")),
                            crate::printf::PfArg::from(zWhere),
                        ],
                    );
                } else {
                    // The sqlite_stat[134] table already exists.  Delete all
                    // rows.
                    sqlite3VdbeAddOp2(db.v(), OP_Clear, aRoot[i] as i32, iDb);
                }
            }
        }
    }

    // Open the sqlite_stat[134] tables for writing.
    for i in 0..nToOpen {
        sqlite3VdbeAddOp4Int(
            db.v(),
            OP_OpenWrite,
            iStatCur + i as i32,
            aRoot[i] as i32,
            iDb,
            3,
        );
        sqlite3VdbeChangeP5(db.v(), aCreateTbl[i]);
    }
}

/// Three SQL functions - stat_init(), stat_push(), and stat_get() -
/// share an instance of the following structure to hold their state
/// information.
#[derive(Default)]
pub struct StatAccum {
    /// Estimated number of rows
    pub nEst: tRowcnt,
    /// Number of rows visited so far
    pub nRow: tRowcnt,
    /// Analysis row-scan limit
    pub nLimit: i32,
    /// Number of columns in index + pk/rowid
    pub nCol: i32,
    /// Number of index columns w/o the pk/rowid
    pub nKeyCol: i32,
    /// Number of times of skip-ahead
    pub nSkipAhead: u8,
    /// Current row as a StatSample (C current.anDLt)
    pub anDLt: Vec<tRowcnt>,
}

/// The StatAccum object carried by a stat_init() result.
fn stat_accum(m: &Mem) -> Option<Rc<RefCell<StatAccum>>> {
    m.pPtr
        .clone()
        .and_then(|p| p.downcast::<RefCell<StatAccum>>().ok())
}

/// Implementation of the stat_init(N,K,C,L) SQL function. The four
/// parameters are:
///     N:    The number of columns in the index including the rowid/pk (note 1)
///     K:    The number of columns in the index excluding the rowid/pk.
///     C:    Estimated number of rows in the index
///     L:    A limit on the number of rows to scan, or 0 for no-limit
///
/// Note 1:  In the special case of the covering index that implements a
/// WITHOUT ROWID table, N is the number of PRIMARY KEY columns, not the
/// total number of columns in the table.
///
/// For indexes on ordinary rowid tables, N==K+1.  But for indexes on
/// WITHOUT ROWID tables, N=K+P where P is the number of columns in the
/// PRIMARY KEY of the table.  The covering index that implements the
/// original WITHOUT ROWID table as N==K as a special case.
///
/// This routine allocates the StatAccum object in heap memory. The return
/// value is a pointer to the StatAccum object.  The datatype of the
/// return value is BLOB, but it is really just a pointer to the StatAccum
/// object.
fn statInit(context: &mut Context, argv: &mut [Mem]) {
    // Decode the three function arguments
    let nCol = sqlite3_value_int(&mut argv[0]);
    debug_assert!(nCol > 0);
    let nKeyCol = sqlite3_value_int(&mut argv[1]);
    debug_assert!(nKeyCol <= nCol);
    debug_assert!(nKeyCol > 0);

    // Allocate the space required for the StatAccum object
    let p = StatAccum {
        nEst: sqlite3_value_int64(&mut argv[2]) as tRowcnt,
        nRow: 0,
        nLimit: sqlite3_value_int(&mut argv[3]),
        nCol,
        nKeyCol,
        nSkipAhead: 0,
        anDLt: vec![0; nCol.max(0) as usize],
    };

    // Return a pointer to the allocated object to the caller.  Note that
    // only the pointer (the 2nd parameter) matters.  The size of the object
    // (given by the 3rd parameter) is never used and can be any positive
    // value.
    sqlite3_result_blob(context, &[], 0);
    context.pOut.pPtr = Some(Rc::new(RefCell::new(p)));
}

fn statInitFuncdef() -> FuncDefRef {
    Rc::new(FuncDef {
        nArg: 4,
        funcFlags: SQLITE_UTF8 as u32,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(statInit)),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: b"stat_init".to_vec(),
        pDestructor: None,
    })
}

/// Implementation of the stat_push SQL function:  stat_push(P,C,R)
/// Arguments:
///
///    P     Pointer to the StatAccum object created by stat_init()
///    C     Index of left-most column to differ from previous row
///    R     Rowid for the current row.  Might be a key record for
///          WITHOUT ROWID tables.
///
/// The purpose of this routine is to collect statistical data and/or
/// samples from the index being analyzed into the StatAccum object.
/// The stat_get() SQL function will be used afterwards to
/// retrieve the information gathered.
///
/// This SQL function usually returns NULL, but might return an integer
/// if it wants the byte-code to do special processing.
///
/// The R parameter is only used for STAT4
fn statPush(context: &mut Context, argv: &mut [Mem]) {
    // The three function arguments
    let p = match stat_accum(&argv[0]) {
        Some(p) => p,
        None => return,
    };
    let iChng = sqlite3_value_int(&mut argv[1]);

    let ret = {
        let mut p = p.borrow_mut();
        debug_assert!(p.nCol > 0);
        debug_assert!(iChng < p.nCol);

        if p.nRow == 0 {
            // This is the first call to this function. Do initialization.
        } else {
            // Second and subsequent calls get processed here

            // Update anDLt[], anLt[] and anEq[] to reflect the values that
            // apply to the current row of the index.
            for i in iChng.max(0)..p.nCol {
                p.anDLt[i as usize] += 1;
            }
        }

        p.nRow += 1;
        if p.nLimit != 0 && p.nRow > (p.nLimit as tRowcnt) * (p.nSkipAhead as tRowcnt + 1) {
            p.nSkipAhead += 1;
            Some((p.anDLt[0] > 0) as i32)
        } else {
            None
        }
    };
    if let Some(r) = ret {
        sqlite3_result_int(context, r);
    }
}

fn statPushFuncdef() -> FuncDefRef {
    Rc::new(FuncDef {
        nArg: 2,
        funcFlags: SQLITE_UTF8 as u32,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(statPush)),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: b"stat_push".to_vec(),
        pDestructor: None,
    })
}

/// "stat" column of stat1 table
const STAT_GET_STAT1: i32 = 0;

/// Implementation of the stat_get(P,J) SQL function.  This routine is
/// used to query statistical information that has been gathered into
/// the StatAccum object by prior calls to stat_push().  The P parameter
/// has type BLOB but it is really just a pointer to the StatAccum object.
/// The content to returned is determined by the parameter J
/// which is one of the STAT_GET_xxxx values defined above.
///
/// The stat_get(P,J) function is not available to generic SQL.  It is
/// inserted as part of a manually constructed bytecode program.  (See
/// the callStatGet() routine below.)  It is guaranteed that the P
/// parameter will always be a pointer to a StatAccum object, never a
/// NULL.
///
/// If STAT4 is not enabled, then J is always
/// STAT_GET_STAT1 and is hence omitted and this routine becomes
/// a one-parameter function, stat_get(P), that always returns the
/// stat1 table entry information.
fn statGet(context: &mut Context, argv: &mut [Mem]) {
    let p = match stat_accum(&argv[0]) {
        Some(p) => p,
        None => return,
    };
    debug_assert!(argv.len() == 1);
    // Return the value to store in the "stat" column of the sqlite_stat1
    // table for this index.
    //
    // The value is a string composed of a list of integers describing
    // the index. The first integer in the list is the total number of
    // entries in the index. There is one additional integer in the list
    // for each indexed column. This additional integer is an estimate of
    // the number of rows matched by a equality query on the index using
    // a key with the corresponding number of fields. In other words,
    // if the index is on columns (a,b) and the sqlite_stat1 value is
    // "100 10 2", then SQLite estimates that:
    //
    //   * the index contains 100 rows,
    //   * "WHERE a=?" matches 10 rows, and
    //   * "WHERE a=? AND b=?" matches 2 rows.
    //
    // If D is the count of distinct values and K is the total number of
    // rows, then each estimate is usually computed as:
    //
    //        I = (K+D-1)/D
    //
    // In other words, I is K/D rounded up to the next whole integer.
    // However, if I is between 1.0 and 1.1 (in other words if I is
    // close to 1.0 but just a little larger) then do not round up but
    // instead keep the I value at 1.0.
    let mut sStat = StrAccum::default();
    {
        let p = p.borrow();
        crate::printf::sqlite3StrAccumInit(&mut sStat, 0, (p.nKeyCol + 1) * 100);
        let n0: u64 = if p.nSkipAhead != 0 { p.nEst } else { p.nRow };
        crate::printf::sqlite3_str_appendf(&mut sStat, b"%llu", &[crate::printf::PfArg::from(n0)]);
        for i in 0..p.nKeyCol.max(0) as usize {
            let nDistinct: u64 = p.anDLt[i] + 1;
            let mut iVal: u64 = (p.nRow + nDistinct - 1) / nDistinct;
            if iVal == 2 && p.nRow * 10 <= nDistinct * 11 {
                iVal = 1;
            }
            crate::printf::sqlite3_str_appendf(
                &mut sStat,
                b" %llu",
                &[crate::printf::PfArg::from(iVal)],
            );
        }
    }
    sqlite3_result_str(context, &mut sStat, SQLITE_XFER);
}

fn statGetFuncdef() -> FuncDefRef {
    Rc::new(FuncDef {
        nArg: 1,
        funcFlags: SQLITE_UTF8 as u32,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(statGet)),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: b"stat_get".to_vec(),
        pDestructor: None,
    })
}

fn callStatGet(db: &mut Sqlite3, regStat: i32, _iParam: i32, regOut: i32) {
    debug_assert!(regOut != regStat && regOut != regStat + 1);
    sqlite3VdbeAddFunctionCall(db, 0, regStat, regOut, 1, &statGetFuncdef(), 0);
}

/// Generate code to do an analysis of all indices associated with
/// a single table.
fn analyzeOneTable(
    db: &mut Sqlite3,
    pTab: &TableRef,
    pOnlyIdx: Option<&IndexRef>,
    iStatCur: i32,
    mut iMem: i32,
    mut iTab: i32,
) {
    let mut needTableCnt = true; // True to count the table
    let regNewRowid = iMem; // Rowid for the inserted record
    iMem += 1;
    let regStat = iMem; // Register to hold StatAccum object
    iMem += 1;
    let regChng = iMem; // Index of changed index field
    iMem += 1;
    let regRowid = iMem; // Rowid argument passed to stat_push()
    iMem += 1;
    let regTemp = iMem; // Temporary use register
    iMem += 1;
    let regTemp2 = iMem; // Second temporary use register
    iMem += 1;
    let regTabname = iMem; // Register containing table name
    iMem += 1;
    let regIdxname = iMem; // Register containing index name
    iMem += 1;
    let regStat1 = iMem; // Value for the stat column of sqlite_stat1
    iMem += 1;
    let regPrev = iMem; // MUST BE LAST (see below)

    crate::expr::sqlite3TouchRegister(db, iMem);
    if !crate::select::sqlite3GetVdbe(db) {
        return;
    }
    let (isOrdinary, zTabName, pSchema, tnum, hasRowid, aIndex) = {
        let t = pTab.borrow();
        (
            t.IsOrdinaryTable(),
            t.zName.clone(),
            t.pSchema,
            t.tnum,
            t.HasRowid(),
            t.pIndex.clone(),
        )
    };
    if !isOrdinary {
        // Do not gather statistics on views or virtual tables
        return;
    }
    if crate::func::sqlite3_strlike(Some(b"sqlite\\_%"), Some(&zTabName), b'\\' as u32) == 0 {
        // Do not gather statistics on system tables
        return;
    }
    let iDb = db.schema_index(pSchema);
    debug_assert!(iDb >= 0);
    let zDbSName = db.aDb[iDb as usize].zDbSName.clone();
    if crate::auth::sqlite3AuthCheck(
        db,
        SQLITE_ANALYZE,
        Some(&zTabName),
        None,
        zDbSName.as_deref(),
    ) != 0
    {
        return;
    }

    // Establish a read-lock on the table at the shared-cache level.
    // Open a read-only cursor on the table. Also allocate a cursor number
    // to use for scanning indexes (iIdxCur). No index cursor is opened at
    // this time though.
    sqlite3TableLock(db, iDb, tnum, 0, Some(&zTabName));
    let iTabCur = iTab;
    iTab += 1;
    let iIdxCur = iTab;
    iTab += 1;
    {
        let p = db.pp();
        p.nTab = p.nTab.max(iTab);
    }
    crate::insert::sqlite3OpenTable(db, iTabCur, iDb, pTab, OP_OpenRead as i32);
    sqlite3VdbeLoadString(db.v(), regTabname, &zTabName);

    for pIdx in aIndex.iter() {
        if let Some(only) = pOnlyIdx {
            if !Rc::ptr_eq(only, pIdx) {
                continue;
            }
        }
        let (
            hasPartWhere,
            isPk,
            nKeyCol,
            nColumn,
            zIdxNameOwn,
            uniqNotNull,
            isUnique,
            azColl,
            idxTnum,
        ) = {
            let x = pIdx.borrow();
            (
                x.pPartIdxWhere.is_some(),
                x.IsPrimaryKeyIndex(),
                x.nKeyCol as i32,
                x.nColumn as i32,
                x.zName.clone(),
                x.uniqNotNull,
                x.IsUniqueIndex(),
                x.azColl.clone(),
                x.tnum,
            )
        };
        if !hasPartWhere {
            needTableCnt = false;
        }
        let nCol: i32; // Number of columns in pIdx. "N"
        let zIdxName: ZStr; // Name of the index
        let nColTest: i32; // Number of columns to test for changes
        if !hasRowid && isPk {
            nCol = nKeyCol;
            zIdxName = zTabName.clone();
            nColTest = nCol - 1;
        } else {
            nCol = nColumn;
            zIdxName = zIdxNameOwn;
            nColTest = if uniqNotNull != 0 {
                nKeyCol - 1
            } else {
                nCol - 1
            };
        }

        // Populate the register containing the index name.
        sqlite3VdbeLoadString(db.v(), regIdxname, &zIdxName);

        // Pseudo-code for loop that calls stat_push():
        //
        //   regChng = 0
        //   Rewind csr
        //   if eof(csr){
        //      stat_init() with count = 0;
        //      goto end_of_scan;
        //   }
        //   count()
        //   stat_init()
        //   goto chng_addr_0;
        //
        //  next_row:
        //   regChng = 0
        //   if( idx(0) != regPrev(0) ) goto chng_addr_0
        //   regChng = 1
        //   if( idx(1) != regPrev(1) ) goto chng_addr_1
        //   ...
        //   regChng = N
        //   goto chng_addr_N
        //
        //  chng_addr_0:
        //   regPrev(0) = idx(0)
        //  chng_addr_1:
        //   regPrev(1) = idx(1)
        //  ...
        //
        //  endDistinctTest:
        //   regRowid = idx(rowid)
        //   stat_push(P, regChng, regRowid)
        //   Next csr
        //   if !eof(csr) goto next_row;
        //
        //  end_of_scan:

        // Make sure there are enough memory cells allocated to accommodate
        // the regPrev array and a trailing rowid (the rowid slot is required
        // when building a record to insert into the sample column of
        // the sqlite_stat4 table.
        crate::expr::sqlite3TouchRegister(db, regPrev + nColTest);

        // Open a read-only cursor on the index being analyzed.
        debug_assert!(iDb == db.schema_index(pIdx.borrow().pSchema));
        sqlite3VdbeAddOp3(db.v(), OP_OpenRead, iIdxCur, idxTnum as i32, iDb);
        sqlite3VdbeSetP4KeyInfo(db, pIdx);

        // Implementation of the following:
        //
        //   regChng = 0
        //   Rewind csr
        //   if eof(csr){
        //      stat_init() with count = 0;
        //      goto end_of_scan;
        //   }
        //   count()
        //   stat_init()
        //   goto chng_addr_0;
        debug_assert!(regTemp2 == regStat + 4);
        let nAnalysisLimit = db.nAnalysisLimit;
        sqlite3VdbeAddOp2(db.v(), OP_Integer, nAnalysisLimit, regTemp2);

        // Arguments to stat_init():
        //    (1) the number of columns in the index including the rowid
        //        (or for a WITHOUT ROWID table, the number of PK columns),
        //    (2) the number of columns in the key without the rowid/pk
        //    (3) estimated number of rows in the index.
        sqlite3VdbeAddOp2(db.v(), OP_Integer, nCol, regStat + 1);
        debug_assert!(regRowid == regStat + 2);
        sqlite3VdbeAddOp2(db.v(), OP_Integer, nKeyCol, regRowid);
        let noStat4 = db.OptimizationDisabled(SQLITE_Stat4) as i32;
        sqlite3VdbeAddOp3(db.v(), OP_Count, iIdxCur, regTemp, noStat4);
        sqlite3VdbeAddFunctionCall(db, 0, regStat + 1, regStat, 4, &statInitFuncdef(), 0);
        let mut addrGotoEnd = sqlite3VdbeAddOp1(db.v(), OP_Rewind, iIdxCur);

        sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regChng);
        let mut addrNextRow = sqlite3VdbeCurrentAddr(db.v());

        if nColTest > 0 {
            let endDistinctTest = sqlite3VdbeMakeLabel(db);
            // Array of jump instruction addresses
            let mut aGotoChng: Vec<i32> = vec![0; nColTest as usize];

            //  next_row:
            //   regChng = 0
            //   if( idx(0) != regPrev(0) ) goto chng_addr_0
            //   regChng = 1
            //   if( idx(1) != regPrev(1) ) goto chng_addr_1
            //   ...
            //   regChng = N
            //   goto endDistinctTest
            sqlite3VdbeAddOp0(db.v(), OP_Goto);
            addrNextRow = sqlite3VdbeCurrentAddr(db.v());
            if nColTest == 1 && nKeyCol == 1 && isUnique {
                // For a single-column UNIQUE index, once we have found a
                // non-NULL row, we know that all the rest will be distinct,
                // so skip subsequent distinctness tests.
                sqlite3VdbeAddOp2(db.v(), OP_NotNull, regPrev, endDistinctTest);
            }
            for i in 0..nColTest {
                let pColl = crate::callback::sqlite3LocateCollSeq(
                    db,
                    azColl.get(i as usize).map(|z| &z[..]),
                );
                sqlite3VdbeAddOp2(db.v(), OP_Integer, i, regChng);
                sqlite3VdbeAddOp3(db.v(), OP_Column, iIdxCur, i, regTemp);
                let p4 = match pColl {
                    Some(c) => P4::Coll(c),
                    None => P4::None,
                };
                aGotoChng[i as usize] = sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Ne,
                    regTemp,
                    0,
                    regPrev + i,
                    p4,
                    P4_COLLSEQ as i32,
                );
                sqlite3VdbeChangeP5(db.v(), SQLITE_NULLEQ);
            }
            sqlite3VdbeAddOp2(db.v(), OP_Integer, nColTest, regChng);
            sqlite3VdbeGoto(db.v(), endDistinctTest);

            //  chng_addr_0:
            //   regPrev(0) = idx(0)
            //  chng_addr_1:
            //   regPrev(1) = idx(1)
            //  ...
            sqlite3VdbeJumpHere(db.v(), addrNextRow - 1);
            for i in 0..nColTest {
                sqlite3VdbeJumpHere(db.v(), aGotoChng[i as usize]);
                sqlite3VdbeAddOp3(db.v(), OP_Column, iIdxCur, i, regPrev + i);
            }
            sqlite3VdbeResolveLabel(db.v(), endDistinctTest);
        }

        //  chng_addr_N:
        //   regRowid = idx(rowid)            // STAT4 only
        //   stat_push(P, regChng, regRowid)  // 3rd parameter STAT4 only
        //   Next csr
        //   if !eof(csr) goto next_row;
        debug_assert!(regChng == (regStat + 1));
        {
            sqlite3VdbeAddFunctionCall(db, 1, regStat, regTemp, 2, &statPushFuncdef(), 0);
            if db.nAnalysisLimit != 0 {
                let j1 = sqlite3VdbeAddOp1(db.v(), OP_IsNull, regTemp);
                let j2 = sqlite3VdbeAddOp1(db.v(), OP_If, regTemp);
                let j3 = sqlite3VdbeAddOp4Int(db.v(), OP_SeekGT, iIdxCur, 0, regPrev, 1);
                sqlite3VdbeJumpHere(db.v(), j1);
                sqlite3VdbeAddOp2(db.v(), OP_Next, iIdxCur, addrNextRow);
                sqlite3VdbeJumpHere(db.v(), j2);
                sqlite3VdbeJumpHere(db.v(), j3);
            } else {
                sqlite3VdbeAddOp2(db.v(), OP_Next, iIdxCur, addrNextRow);
            }
        }

        // Add the entry to the stat1 table.
        if hasPartWhere {
            // Partial indexes might get a zero-entry in sqlite_stat1.  But
            // an empty table is omitted from sqlite_stat1.
            sqlite3VdbeJumpHere(db.v(), addrGotoEnd);
            addrGotoEnd = 0;
        }
        callStatGet(db, regStat, STAT_GET_STAT1, regStat1);
        debug_assert!(b'B' == SQLITE_AFF_TEXT);
        sqlite3VdbeAddOp4(
            db.v(),
            OP_MakeRecord,
            regTabname,
            3,
            regTemp,
            P4::Z(b"BBB".to_vec()),
            0,
        );
        sqlite3VdbeAddOp2(db.v(), OP_NewRowid, iStatCur, regNewRowid);
        sqlite3VdbeAddOp3(db.v(), OP_Insert, iStatCur, regTemp, regNewRowid);
        sqlite3VdbeChangeP5(db.v(), OPFLAG_APPEND);

        // End of analysis
        if addrGotoEnd != 0 {
            sqlite3VdbeJumpHere(db.v(), addrGotoEnd);
        }
    }

    // Create a single sqlite_stat1 entry containing NULL as the index
    // name and the row count as the content.
    if pOnlyIdx.is_none() && needTableCnt {
        sqlite3VdbeAddOp2(db.v(), OP_Count, iTabCur, regStat1);
        let jZeroRows = sqlite3VdbeAddOp1(db.v(), OP_IfNot, regStat1);
        sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regIdxname);
        debug_assert!(b'B' == SQLITE_AFF_TEXT);
        sqlite3VdbeAddOp4(
            db.v(),
            OP_MakeRecord,
            regTabname,
            3,
            regTemp,
            P4::Z(b"BBB".to_vec()),
            0,
        );
        sqlite3VdbeAddOp2(db.v(), OP_NewRowid, iStatCur, regNewRowid);
        sqlite3VdbeAddOp3(db.v(), OP_Insert, iStatCur, regTemp, regNewRowid);
        sqlite3VdbeChangeP5(db.v(), OPFLAG_APPEND);
        sqlite3VdbeJumpHere(db.v(), jZeroRows);
    }
}

/// Generate code that will cause the most recent index analysis to
/// be loaded into internal hash tables where is can be used.
fn loadAnalysis(db: &mut Sqlite3, iDb: i32) {
    if crate::select::sqlite3GetVdbe(db) {
        sqlite3VdbeAddOp1(db.v(), OP_LoadAnalysis, iDb);
    }
}

/// Generate code that will do an analysis of an entire database
fn analyzeDatabase(db: &mut Sqlite3, iDb: i32) {
    sqlite3BeginWriteOperation(db, 0, iDb);
    let iStatCur = db.ppr().nTab;
    db.pp().nTab += 3;
    openStatTable(db, iDb, iStatCur, None, None);
    let iMem = db.ppr().nMem + 1;
    let iTab = db.ppr().nTab;
    let aTab: Vec<TableRef> = db.aDb[iDb as usize].pSchema.borrow().tblHash.values();
    for pTab in aTab.iter() {
        analyzeOneTable(db, pTab, None, iStatCur, iMem, iTab);
    }
    loadAnalysis(db, iDb);
}

/// Generate code that will do an analysis of a single table in
/// a database.  If pOnlyIdx is not NULL then it is a single index
/// in pTab that should be analyzed.
fn analyzeTable(db: &mut Sqlite3, pTab: &TableRef, pOnlyIdx: Option<&IndexRef>) {
    let iDb = db.schema_index(pTab.borrow().pSchema);
    sqlite3BeginWriteOperation(db, 0, iDb);
    let iStatCur = db.ppr().nTab;
    db.pp().nTab += 3;
    if let Some(idx) = pOnlyIdx {
        let z = idx.borrow().zName.clone();
        openStatTable(db, iDb, iStatCur, Some(&z), Some("idx"));
    } else {
        let z = pTab.borrow().zName.clone();
        openStatTable(db, iDb, iStatCur, Some(&z), Some("tbl"));
    }
    let iMem = db.ppr().nMem + 1;
    let iTab = db.ppr().nTab;
    analyzeOneTable(db, pTab, pOnlyIdx, iStatCur, iMem, iTab);
    loadAnalysis(db, iDb);
}

/// C: `void sqlite3Analyze(Parse *pParse, Token *pName1, Token *pName2)`
///
/// Generate code for the ANALYZE command.  The parser calls this routine
/// when it recognizes an ANALYZE command.
///
///        ANALYZE                            -- 1
///        ANALYZE  <database>                -- 2
///        ANALYZE  ?<database>.?<tablename>  -- 3
///
/// Form 1 causes all indices in all attached databases to be analyzed.
/// Form 2 analyzes all indices the single database named.
/// Form 3 analyzes all indices associated with the named table.
pub fn sqlite3Analyze(db: &mut Sqlite3, pName1: Option<&Token>, pName2: Option<&Token>) {
    // Read the database schema. If an error occurs, leave an error message
    // and code in pParse and return NULL.
    if SQLITE_OK != crate::prepare::sqlite3ReadSchema(db) {
        return;
    }

    debug_assert!(pName2.is_some() || pName1.is_none());
    match pName1 {
        None => {
            // Form 1:  Analyze everything
            let mut i = 0;
            while i < db.nDb() {
                if i != 1 {
                    // Do not analyze the TEMP database
                    analyzeDatabase(db, i);
                }
                i += 1;
            }
        }
        Some(pName1) => {
            let pName2 = pName2.expect("pName2");
            let mut iDb = -1;
            if pName2.n == 0 {
                iDb = sqlite3FindDb(db, pName1);
            }
            if pName2.n == 0 && iDb >= 0 {
                // Analyze the schema named as the argument
                analyzeDatabase(db, iDb);
            } else {
                // Form 3: Analyze the table or index named as an argument
                let mut pTableName = Token::default();
                iDb = sqlite3TwoPartName(db, pName1, pName2, &mut pTableName);
                if iDb >= 0 {
                    let zDb = if pName2.n != 0 {
                        db.aDb[iDb as usize].zDbSName.clone()
                    } else {
                        None
                    };
                    if let Some(z) = sqlite3NameFromToken(db, &pTableName) {
                        if let Some(pIdx) = sqlite3FindIndex(db, Some(&z), zDb.as_deref()) {
                            let pTab = pIdx.borrow().table();
                            analyzeTable(db, &pTab, Some(&pIdx));
                        } else if let Some(pTab) =
                            sqlite3LocateTable(db, 0, Some(&z), zDb.as_deref())
                        {
                            analyzeTable(db, &pTab, None);
                        }
                    }
                }
            }
        }
    }
    if db.nSqlExec == 0 && crate::select::sqlite3GetVdbe(db) {
        sqlite3VdbeAddOp0(db.v(), OP_Expire);
    }
}

/// The first argument points to a nul-terminated string containing a
/// list of space separated integers. Read the first nOut of these into
/// the array aLog[] (as LogEst values).  Handle the extra flags
/// ("unordered", "sz=N", "noskipscan") for the index described by the
/// last three arguments.
fn decodeIntArray(
    zIntArray: &[u8],
    nOut: usize,
    aLog: &mut [LogEst],
    bUnordered: &mut u8,
    noSkipScan: &mut u8,
    szIdxRow: &mut LogEst,
) {
    let z = zIntArray;
    let at = |i: usize| -> u8 { z.get(i).copied().unwrap_or(0) };
    let mut p = 0usize;
    let mut i = 0usize;
    while at(p) != 0 && i < nOut {
        let mut v: tRowcnt = 0;
        loop {
            let c = at(p);
            if !(b'0'..=b'9').contains(&c) {
                break;
            }
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as tRowcnt);
            p += 1;
        }
        if i < aLog.len() {
            aLog[i] = crate::util::sqlite3LogEst(v);
        }
        if at(p) == b' ' {
            p += 1;
        }
        i += 1;
    }
    *bUnordered = 0;
    *noSkipScan = 0;
    while at(p) != 0 {
        let rest = &z[p..];
        if crate::func::sqlite3_strglob(Some(b"unordered*"), Some(rest)) == 0 {
            *bUnordered = 1;
        } else if crate::func::sqlite3_strglob(Some(b"sz=[0-9]*"), Some(rest)) == 0 {
            let mut sz = crate::util::sqlite3Atoi(Some(&rest[3..]));
            if sz < 2 {
                sz = 2;
            }
            *szIdxRow = crate::util::sqlite3LogEst(sz as u64);
        } else if crate::func::sqlite3_strglob(Some(b"noskipscan*"), Some(rest)) == 0 {
            *noSkipScan = 1;
        }
        while at(p) != 0 && at(p) != b' ' {
            p += 1;
        }
        while at(p) == b' ' {
            p += 1;
        }
    }
}

/// This callback is invoked once for each index when reading the
/// sqlite_stat1 table.
///
///     argv[0] = name of the table
///     argv[1] = name of the index (might be NULL)
///     argv[2] = results of analysis - on integer for each column
///
/// Entries for which argv[1]==NULL simply record the number of rows in
/// the table.
///
/// Rust: an empty argv is the C `argv==0` of the empty-result callback
/// (PRAGMA empty_result_callbacks=ON).
fn analysisLoader(db: &mut Sqlite3, zDatabase: Option<&[u8]>, argv: &[Option<ZStr>]) -> i32 {
    debug_assert!(argv.is_empty() || argv.len() == 3);

    let (z0, z1, z2) = match (argv.first(), argv.get(1), argv.get(2)) {
        (Some(Some(a)), Some(b), Some(Some(c))) => (a, b, c),
        _ => return 0,
    };
    let pTable = match sqlite3FindTable(db, Some(z0), zDatabase) {
        Some(t) => t,
        None => return 0,
    };
    let pIndex: Option<IndexRef> = match z1 {
        None => None,
        Some(z1) => {
            if crate::util::sqlite3_stricmp(Some(z0), Some(z1)) == 0 {
                sqlite3PrimaryKeyIndex(&pTable)
            } else {
                sqlite3FindIndex(db, Some(z1), zDatabase)
            }
        }
    };
    let z = &z2[..];

    if let Some(pIndex) = pIndex {
        let mut x = pIndex.borrow_mut();
        let nCol = x.nKeyCol as usize + 1;
        x.bUnordered = 0;
        let mut aLog = std::mem::take(&mut x.aiRowLogEst);
        if aLog.len() < nCol {
            aLog.resize(nCol, 0);
        }
        let (mut bUnordered, mut noSkipScan, mut szIdxRow) =
            (x.bUnordered, x.noSkipScan, x.szIdxRow);
        decodeIntArray(
            z,
            nCol,
            &mut aLog,
            &mut bUnordered,
            &mut noSkipScan,
            &mut szIdxRow,
        );
        x.aiRowLogEst = aLog;
        x.bUnordered = bUnordered;
        x.noSkipScan = noSkipScan;
        x.szIdxRow = szIdxRow;
        x.hasStat1 = 1;
        if x.pPartIdxWhere.is_none() {
            let est = x.aiRowLogEst[0];
            drop(x);
            let mut t = pTable.borrow_mut();
            t.nRowLogEst = est;
            t.tabFlags |= TF_HasStat1;
        }
    } else {
        let mut t = pTable.borrow_mut();
        let mut szIdxRow = t.szTabRow;
        let (mut bU, mut nS) = (0u8, 0u8);
        let mut aLog = [t.nRowLogEst];
        decodeIntArray(z, 1, &mut aLog, &mut bU, &mut nS, &mut szIdxRow);
        t.nRowLogEst = aLog[0];
        t.szTabRow = szIdxRow;
        t.tabFlags |= TF_HasStat1;
    }

    0
}

/// C: `void sqlite3DeleteIndexSamples(sqlite3 *db, Index *pIdx)`
///
/// If the Index.aSample variable is not NULL, delete the aSample[] array
/// and its contents.  (No-op without STAT4.)
pub fn sqlite3DeleteIndexSamples(db: &mut Sqlite3, pIdx: &IndexRef) {
    let _ = (db, pIdx);
}

/// C: `int sqlite3AnalysisLoad(sqlite3 *db, int iDb)`
///
/// Load the content of the sqlite_stat1 and sqlite_stat4 tables. The
/// contents of sqlite_stat1 are used to populate the Index.aiRowEst[]
/// arrays. The contents of sqlite_stat4 are used to populate the
/// Index.aSample[] arrays.
///
/// If the sqlite_stat1 table is not present in the database, SQLITE_ERROR
/// is returned. In this case, even if SQLITE_ENABLE_STAT4 was defined
/// during compilation and the sqlite_stat4 table is present, no data is
/// read from it.
///
/// If SQLITE_ENABLE_STAT4 was defined during compilation and the
/// sqlite_stat4 table is not present in the database, SQLITE_ERROR is
/// returned. However, in this case, data is read from the sqlite_stat1
/// table (if it is present) before returning.
///
/// Rust: sqlite3_exec() cannot lend the connection to its callback, so the
/// rows of sqlite_stat1 are collected first and applied (in order) once
/// the query has finished.
pub fn sqlite3AnalysisLoad(db: &mut Sqlite3, iDb: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(iDb >= 0 && iDb < db.nDb());
    debug_assert!(db.aDb[iDb as usize].pBt.is_some());

    // Clear any prior statistics
    for pTab in db.aDb[iDb as usize].pSchema.borrow().tblHash.values() {
        pTab.borrow_mut().tabFlags &= !TF_HasStat1;
    }
    for pIdx in db.aDb[iDb as usize].pSchema.borrow().idxHash.values() {
        pIdx.borrow_mut().hasStat1 = 0;
    }

    // Load new statistics out of the sqlite_stat1 table
    let zDatabase = db.aDb[iDb as usize].zDbSName.clone();
    let pStat1 = sqlite3FindTable(db, Some(b"sqlite_stat1"), zDatabase.as_deref());
    if let Some(pStat1) = pStat1 {
        if pStat1.borrow().IsOrdinaryTable() {
            let zSql = mprintf!(
                db,
                "SELECT tbl,idx,stat FROM %Q.sqlite_stat1",
                zDatabase.as_deref()
            );
            let mut rows: Vec<Vec<Option<ZStr>>> = Vec::new();
            let mut cb = |argv: &[Option<&[u8]>], _cols: &[&[u8]]| -> i32 {
                rows.push(argv.iter().map(|a| a.map(|s| s.to_vec())).collect());
                0
            };
            let mut zErr: Option<ZStr> = None;
            rc = crate::legacy::sqlite3_exec(db, Some(&zSql), Some(&mut cb), &mut zErr);
            for row in rows.iter() {
                analysisLoader(db, zDatabase.as_deref(), row);
            }
        }
    }

    // Set appropriate defaults on all indexes not in the sqlite_stat1 table
    for pIdx in db.aDb[iDb as usize].pSchema.borrow().idxHash.values() {
        if pIdx.borrow().hasStat1 == 0 {
            sqlite3DefaultRowEst(&pIdx);
        }
    }

    // Load the statistics from the sqlite_stat4 table.  (No STAT4.)

    if rc == SQLITE_NOMEM {
        db.mallocFailed = 1;
    }
    rc
}
