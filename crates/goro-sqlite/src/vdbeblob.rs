//! Port of vdbeblob.c: incremental BLOB I/O.
//!
//! API shape: `sqlite3_blob_open()` returns a `Box<Blob>` (C
//! `sqlite3_blob*`, the Incrblob structure).  The handle holds the
//! `StmtId` of the small VDBE program that keeps the b-tree cursor open
//! (the statement lives in the connection's statement slots like any
//! prepared statement) and a reference to that b-tree cursor.  Every
//! function that needs the connection takes `db` explicitly; the handle
//! must be used with the connection that opened it.  `sqlite3_blob_close()`
//! finalizes the statement; the caller then drops the Box.

use crate::int::*;
use crate::util::{sqlite3Error, sqlite3ErrorWithMsg};
use crate::vdbeapi::{sqlite3_finalize, sqlite3_step, sqlite3ApiExit};

/// Valid sqlite3_blob* handles point to Incrblob structures.
pub struct Blob {
    /// Size of open blob, in bytes
    pub nByte: i32,
    /// Byte offset of blob in cursor data
    pub iOffset: i32,
    /// Table column this handle is open on
    pub iCol: u16,
    /// Cursor pointing at blob row
    pub pCsr: Option<BtCursorP>,
    /// Statement holding cursor open (None once invalidated)
    pub pStmt: Option<StmtId>,
    /// Database name
    pub zDb: ZStr,
    /// Table object
    pub pTab: Option<TableRef>,
}

/// This function is used by both blob_open() and blob_reopen().  It seeks
/// the b-tree cursor associated with blob handle p to point to row iRow.
/// If successful, SQLITE_OK is returned and subsequent calls to
/// sqlite3_blob_read() or sqlite3_blob_write() access the specified row.
///
/// If an error occurs, or if the specified row does not exist or does not
/// contain a value of type TEXT or BLOB in the column nominated when the
/// blob handle was opened, then an error code is returned and *pzErr may
/// be set to an error message.
///
/// If an error does occur, then the b-tree cursor is closed.  All
/// subsequent calls to sqlite3_blob_read(), blob_write() or blob_reopen()
/// will immediately return SQLITE_ABORT.
fn blobSeekToRow(db: &mut Sqlite3, p: &mut Blob, iRow: i64, pzErr: &mut Option<ZStr>) -> i32 {
    let mut rc;
    let mut zErr: Option<ZStr> = None;
    let id = match p.pStmt {
        Some(id) => id,
        None => return SQLITE_ABORT,
    };

    let mut v = match db.stmt_take(id) {
        Some(v) => v,
        None => return SQLITE_MISUSE,
    };
    /* Set the value of register r[1] in the SQL statement to integer iRow.
     ** This is done directly as a performance optimization
     */
    crate::vdbemem::sqlite3VdbeMemSetInt64(&mut v.aMem[1], iRow);

    /* If the statement has been run before (and is paused at the
     ** OP_ResultRow) then back it up to the point where it does the
     ** OP_NotExists.  This could have been down with an extra OP_Goto, but
     ** simply setting the program counter is faster. */
    if v.pc > 4 {
        v.pc = 4;
        debug_assert!(v.aOp[v.pc as usize].opcode == OP_NotExists);
        rc = crate::vdbe::sqlite3VdbeExec(db, &mut v);
        db.stmt_put(id, v);
    } else {
        db.stmt_put(id, v);
        rc = sqlite3_step(db, id);
    }
    if rc == SQLITE_ROW {
        let info = db.aVdbe[id.idx as usize].v.as_deref().and_then(|v| {
            let pC = v.apCsr.first()?.as_deref()?;
            debug_assert!(pC.eCurType == CURTYPE_BTREE);
            let iCol = p.iCol as usize;
            let ty = if pC.nHdrParsed as usize > iCol {
                pC.aType.get(iCol).copied().unwrap_or(0)
            } else {
                0
            };
            Some((
                ty,
                pC.aOffset.get(iCol).copied().unwrap_or(0),
                pC.pCursor.clone(),
            ))
        });
        let (ty, off, pCur) = info.unwrap_or((0, 0, None));
        if ty < 12 {
            let zType: &[u8] = if ty == 0 {
                b"null"
            } else if ty == 7 {
                b"real"
            } else {
                b"integer"
            };
            zErr = Some(mprintf!(db, "cannot open value of type %s", zType));
            rc = SQLITE_ERROR;
            sqlite3_finalize(db, id);
            p.pStmt = None;
        } else {
            p.iOffset = off as i32;
            p.nByte = crate::vdbeaux::sqlite3VdbeSerialTypeLen(ty) as i32;
            p.pCsr = pCur;
            if let Some(c) = &p.pCsr {
                crate::btree::sqlite3BtreeIncrblobCursor(c);
            }
        }
    }

    if rc == SQLITE_ROW {
        rc = SQLITE_OK;
    } else if let Some(id) = p.pStmt {
        rc = sqlite3_finalize(db, id);
        p.pStmt = None;
        if rc == SQLITE_OK {
            zErr = Some(mprintf!(db, "no such rowid: %lld", iRow));
            rc = SQLITE_ERROR;
        } else {
            let m = crate::mainapi::sqlite3_errmsg(db);
            zErr = Some(mprintf!(db, "%s", m.as_deref()));
        }
    }

    debug_assert!(rc != SQLITE_OK || zErr.is_none());
    debug_assert!(rc != SQLITE_ROW && rc != SQLITE_DONE);

    *pzErr = zErr;
    rc
}

/// The VDBE program of a blob handle.
///
/// This VDBE program seeks a btree cursor to the identified db/table/row
/// entry.  The reason for using a vdbe program instead of writing code to
/// use the b-tree layer directly is that the vdbe program will take
/// advantage of the various transaction, locking and error handling
/// infrastructure built into the vdbe.
///
/// After seeking the cursor, the vdbe executes an OP_ResultRow.  Code
/// external to the Vdbe then "borrows" the b-tree cursor and uses it to
/// implement the blob_read(), blob_write() and blob_bytes() functions.
///
/// The sqlite3_blob_close() function finalizes the vdbe program, which
/// closes the b-tree cursor and (possibly) commits the transaction.
const openBlob: [VdbeOpList; 6] = [
    VdbeOpList {
        opcode: OP_TableLock,
        p1: 0,
        p2: 0,
        p3: 0,
    }, /* 0: Acquire a read or write lock */
    VdbeOpList {
        opcode: OP_OpenRead,
        p1: 0,
        p2: 0,
        p3: 0,
    }, /* 1: Open a cursor */
    /* blobSeekToRow() will initialize r[1] to the desired rowid */
    VdbeOpList {
        opcode: OP_NotExists,
        p1: 0,
        p2: 5,
        p3: 1,
    }, /* 2: Seek the cursor to rowid=r[1] */
    VdbeOpList {
        opcode: OP_Column,
        p1: 0,
        p2: 0,
        p3: 1,
    }, /* 3  */
    VdbeOpList {
        opcode: OP_ResultRow,
        p1: 1,
        p2: 0,
        p3: 0,
    }, /* 4  */
    VdbeOpList {
        opcode: OP_Halt,
        p1: 0,
        p2: 0,
        p3: 0,
    }, /* 5  */
];

/// C: `int sqlite3_blob_open( sqlite3* db, const char *zDb, const char *zTable, const char *zColumn, sqlite_int64 iRow, int wrFlag, sqlite3_blob **ppBlob )`
///
/// Open a blob handle.
pub fn sqlite3_blob_open(
    db: &mut Sqlite3,
    zDb: Option<&[u8]>,
    zTable: Option<&[u8]>,
    zColumn: Option<&[u8]>,
    iRow: i64,
    wrFlag: i32,
    ppBlob: &mut Option<Box<crate::vdbeblob::Blob>>,
) -> i32 {
    let mut nAttempt = 0;
    let mut rc;
    let mut zErr: Option<ZStr>;

    *ppBlob = None;
    if zTable.is_none() || zColumn.is_none() {
        return SQLITE_MISUSE;
    }
    let wrFlag = (wrFlag != 0) as i32;

    let mut pBlob = Box::new(Blob {
        nByte: 0,
        iOffset: 0,
        iCol: 0,
        pCsr: None,
        pStmt: None,
        zDb: Vec::new(),
        pTab: None,
    });
    'blob_open_out: loop {
        crate::prepare::sqlite3ParseObjectInit(db);
        zErr = None;

        let mut pTab = crate::build::sqlite3LocateTable(db, 0, zTable, zDb);
        if let Some(t) = &pTab {
            let (isVirtual, hasRowid, hasGenerated, isView) = {
                let t = t.borrow();
                (
                    t.IsVirtual(),
                    t.HasRowid(),
                    (t.tabFlags & TF_HasGenerated) != 0,
                    t.IsView(),
                )
            };
            if isVirtual {
                pTab = None;
                errmsg!(db, "cannot open virtual table: %s", zTable);
            } else if !hasRowid {
                pTab = None;
                errmsg!(db, "cannot open table without rowid: %s", zTable);
            } else if hasGenerated {
                pTab = None;
                errmsg!(db, "cannot open table with generated columns: %s", zTable);
            } else if isView {
                pTab = None;
                errmsg!(db, "cannot open view: %s", zTable);
            }
        }
        let mut iDb = 0;
        let bFail = match &pTab {
            None => true,
            Some(t) => {
                let sid = t.borrow().pSchema;
                iDb = db.schema_index(sid);
                iDb == 1 && crate::build::sqlite3OpenTempDatabase(db) != 0
            }
        };
        if bFail {
            if let Some(m) = db.pp().zErrMsg.take() {
                zErr = Some(m);
            }
            rc = SQLITE_ERROR;
            break 'blob_open_out;
        }
        let pTab = pTab.unwrap();
        pBlob.pTab = Some(pTab.clone());
        pBlob.zDb = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();

        /* Now search pTab for the exact column. */
        let iCol = crate::select::sqlite3ColumnIndex(&pTab, zColumn);
        if iCol < 0 {
            zErr = Some(mprintf!(db, "no such column: \"%s\"", zColumn));
            rc = SQLITE_ERROR;
            break 'blob_open_out;
        }

        /* If the value is being opened for writing, check that the column
         ** is not indexed, and that it is not part of a foreign key.
         */
        if wrFlag != 0 {
            let mut zFault: Option<&[u8]> = None;
            let t = pTab.borrow();
            if db.flags & SQLITE_ForeignKeys != 0 {
                /* Check that the column is not part of an FK child key
                 ** definition.  It is not necessary to check if it is part of
                 ** a parent key, as parent key columns must be indexed.  The
                 ** check below will pick up this case.  */
                debug_assert!(t.IsOrdinaryTable());
                for pFKey in t.u.tab.pFKey.iter() {
                    let fk = pFKey.borrow();
                    for j in 0..fk.nCol as usize {
                        if fk.aCol.get(j).map(|c| c.iFrom) == Some(iCol) {
                            zFault = Some(b"foreign key");
                        }
                    }
                }
            }
            for pIdx in t.pIndex.iter() {
                let idx = pIdx.borrow();
                for j in 0..idx.nKeyCol as usize {
                    /* FIXME: Be smarter about indexes that use expressions */
                    let c = idx.aiColumn.get(j).copied().unwrap_or(0);
                    if c as i32 == iCol || c == XN_EXPR {
                        zFault = Some(b"indexed");
                    }
                }
            }
            drop(t);
            if let Some(f) = zFault {
                zErr = Some(mprintf!(db, "cannot open %s column for writing", f));
                rc = SQLITE_ERROR;
                break 'blob_open_out;
            }
        }

        let mut v = crate::vdbeaux::sqlite3VdbeCreate(db);
        {
            let (cookie, generation) = {
                let s = &db.aDb[iDb as usize].pSchema;
                (s.schema_cookie, s.iGeneration)
            };
            let (tnum, nCol) = {
                let t = pTab.borrow();
                (t.tnum, t.nCol as i32)
            };
            crate::vdbeaux::sqlite3VdbeAddOp4Int(
                &mut v,
                OP_Transaction,
                iDb,
                wrFlag,
                cookie,
                generation,
            );
            crate::vdbeaux::sqlite3VdbeChangeP5(&mut v, 1);
            debug_assert!(crate::vdbeaux::sqlite3VdbeCurrentAddr(&mut v) == 2);
            let addr = crate::vdbeaux::sqlite3VdbeAddOpList(&mut v, &openBlob, 0) as usize;

            /* Make sure a mutex is held on the table to be accessed */
            crate::vdbeaux::sqlite3VdbeUsesBtree(&mut v, iDb);

            /* Configure the OP_TableLock instruction (SQLITE_OMIT_SHARED_CACHE) */
            v.aOp[addr].opcode = OP_Noop;

            /* Remove either the OP_OpenWrite or OpenRead. Set the P2
             ** parameter of the other to pTab->tnum.  */
            if wrFlag != 0 {
                v.aOp[addr + 1].opcode = OP_OpenWrite;
            }
            v.aOp[addr + 1].p2 = tnum as i32;
            v.aOp[addr + 1].p3 = iDb;

            /* Configure the number of columns. Configure the cursor to think
             ** that the table has one more column than it really does. An
             ** OP_Column to retrieve this imaginary column will always return
             ** an SQL NULL. This is useful because it means we can invoke
             ** OP_Column to fill in the vdbe cursors type and offset cache
             ** without causing any IO.
             */
            v.aOp[addr + 1].p4type = P4_INT32;
            v.aOp[addr + 1].p4 = P4::I(nCol + 1);
            v.aOp[addr + 3].p2 = nCol;

            {
                let pp = db.pp();
                pp.nVar = 0;
                pp.nMem = 1;
                pp.nTab = 1;
            }
            crate::vdbeaux::sqlite3VdbeMakeReady(db, &mut v);
        }
        pBlob.pStmt = Some(db.stmt_new(v));

        pBlob.iCol = iCol as u16;
        rc = blobSeekToRow(db, &mut pBlob, iRow, &mut zErr);
        nAttempt += 1;
        if nAttempt >= SQLITE_MAX_SCHEMA_RETRY || rc != SQLITE_SCHEMA {
            break 'blob_open_out;
        }
        crate::prepare::sqlite3ParseObjectReset(db);
    }

    /* blob_open_out: */
    if rc == SQLITE_OK && db.mallocFailed == 0 {
        *ppBlob = Some(pBlob);
    } else if let Some(id) = pBlob.pStmt.take() {
        if let Some(mut v) = db.stmt_take(id) {
            crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v);
        }
        db.stmt_remove(id);
    }
    match &zErr {
        Some(z) => sqlite3ErrorWithMsg(db, rc, Some(b"%s"), &[crate::printf::PfArg::from(z)]),
        None => sqlite3ErrorWithMsg(db, rc, None, &[]),
    }
    crate::prepare::sqlite3ParseObjectReset(db);
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_blob_close(sqlite3_blob *pBlob)`
///
/// Close a blob handle that was previously created using
/// sqlite3_blob_open().  (The caller drops the Box afterwards.)
pub fn sqlite3_blob_close(db: &mut Sqlite3, pBlob: &mut crate::vdbeblob::Blob) -> i32 {
    pBlob.pCsr = None;
    match pBlob.pStmt.take() {
        Some(id) => sqlite3_finalize(db, id),
        /* sqlite3_finalize(NULL) is a harmless no-op */
        None => SQLITE_OK,
    }
}

/// Perform a read or write operation on a blob.
fn blobReadWrite(
    db: &mut Sqlite3,
    p: &mut Blob,
    z: &mut [u8],
    n: i32,
    iOffset: i32,
    bWrite: bool,
) -> i32 {
    let mut rc;

    if n < 0 || iOffset < 0 || (iOffset as i64 + n as i64) > p.nByte as i64 || n as usize > z.len()
    {
        /* Request is out of range. Return a transient error. */
        rc = SQLITE_ERROR;
    } else if p.pStmt.is_none() || p.pCsr.is_none() {
        /* If there is no statement handle, then the blob-handle has already
         ** been invalidated. Return SQLITE_ABORT in this case.
         */
        rc = SQLITE_ABORT;
    } else {
        /* Call either BtreeData() or BtreePutData(). If SQLITE_ABORT is
         ** returned, clean-up the statement handle.
         */
        let pCsr = p.pCsr.clone().unwrap();
        let off = (iOffset + p.iOffset) as u32;
        let buf = &mut z[..n as usize];
        rc = if bWrite {
            crate::btree::sqlite3BtreePutData(db, &pCsr, off, n as u32, buf)
        } else {
            crate::btree::sqlite3BtreePayloadChecked(&pCsr, off, n as u32, buf)
        };

        let id = p.pStmt.unwrap();
        if rc == SQLITE_ABORT {
            if let Some(mut v) = db.stmt_take(id) {
                crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v);
            }
            db.stmt_remove(id);
            p.pStmt = None;
        } else if db.stmt_valid(id) {
            if let Some(v) = db.aVdbe[id.idx as usize].v.as_deref_mut() {
                v.rc = rc;
            }
        }
    }
    sqlite3Error(db, rc);
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_blob_read(sqlite3_blob *pBlob, void *z, int n, int iOffset)`
///
/// Read data from a blob handle.  Rust: takes `db`.
pub fn sqlite3_blob_read(
    db: &mut Sqlite3,
    pBlob: &mut crate::vdbeblob::Blob,
    z: &mut [u8],
    n: i32,
    iOffset: i32,
) -> i32 {
    blobReadWrite(db, pBlob, z, n, iOffset, false)
}

/// C: `int sqlite3_blob_write(sqlite3_blob *pBlob, const void *z, int n, int iOffset)`
///
/// Write data to a blob handle.  Rust: takes `db`.
pub fn sqlite3_blob_write(
    db: &mut Sqlite3,
    pBlob: &mut crate::vdbeblob::Blob,
    z: &[u8],
    n: i32,
    iOffset: i32,
) -> i32 {
    let mut buf = z.to_vec();
    blobReadWrite(db, pBlob, &mut buf, n, iOffset, true)
}

/// C: `int sqlite3_blob_bytes(sqlite3_blob *pBlob)`
///
/// Query a blob handle for the size of the data.
pub fn sqlite3_blob_bytes(pBlob: &mut crate::vdbeblob::Blob) -> i32 {
    if pBlob.pStmt.is_some() {
        pBlob.nByte
    } else {
        0
    }
}

/// C: `int sqlite3_blob_reopen(sqlite3_blob *pBlob, sqlite3_int64 iRow)`
///
/// Move an existing blob handle to point to a different row of the same
/// database table.
///
/// If an error occurs, or if the specified row does not exist or does not
/// contain a blob or text value, then an error code is returned and the
/// database handle error code and message set.  If this happens, then all
/// subsequent calls to sqlite3_blob_xxx() functions (except blob_close())
/// immediately return SQLITE_ABORT.
pub fn sqlite3_blob_reopen(db: &mut Sqlite3, pBlob: &mut crate::vdbeblob::Blob, iRow: i64) -> i32 {
    let mut rc;
    match pBlob.pStmt {
        None => {
            /* If there is no statement handle, then the blob-handle has
             ** already been invalidated. Return SQLITE_ABORT in this case.
             */
            rc = SQLITE_ABORT;
        }
        Some(id) => {
            if db.stmt_valid(id) {
                if let Some(v) = db.aVdbe[id.idx as usize].v.as_deref_mut() {
                    v.rc = SQLITE_OK;
                }
            }
            let mut zErr: Option<ZStr> = None;
            rc = blobSeekToRow(db, pBlob, iRow, &mut zErr);
            if rc != SQLITE_OK {
                match &zErr {
                    Some(z) => {
                        sqlite3ErrorWithMsg(db, rc, Some(b"%s"), &[crate::printf::PfArg::from(z)])
                    }
                    None => sqlite3ErrorWithMsg(db, rc, None, &[]),
                }
            }
            debug_assert!(rc != SQLITE_SCHEMA);
        }
    }

    rc = sqlite3ApiExit(db, rc);
    debug_assert!(rc == SQLITE_OK || pBlob.pStmt.is_none());
    rc
}
