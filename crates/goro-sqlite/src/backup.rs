//! Port of backup.c: the online backup API (sqlite3_backup_init/step/
//! finish/remaining/pagecount) and sqlite3BtreeCopyFile() (used by
//! VACUUM).
//!
//! Rust API (connections cannot be shared as raw pointers):
//!
//! * `sqlite3_backup_init(pDestDb, zDestDb, Some(pSrcDb), zSrcDb)` returns
//!   a `BackupP` handle (`None` on error, with the error left in pDestDb).
//!   Pass `None` as pSrcDb when the application named the same connection
//!   twice: that reproduces the C "source and destination must be
//!   distinct" error.
//! * `sqlite3_backup_step(pDestDb, pSrcDb, &p, nPage)` and
//!   `sqlite3_backup_finish(pDestDb, pSrcDb, p)` take both connections
//!   again (they must be the same two connections as for init).  PHP's
//!   SQLite3::backup(destDb, srcName, destName) is
//!   init + step(-1) + finish with the two connections.
//! * `sqlite3_backup_remaining(&p)` / `sqlite3_backup_pagecount(&p)`.
//!
//! Page-change notifications: while a backup is in progress (after its
//! first step) it is registered in the list returned by
//! `sqlite3PagerBackupPtr()` of the *source* pager (C pPager->pBackup).
//! The pager calls `sqlite3BackupUpdate(&list, pgno, data)` whenever it
//! writes a page of the source database and `sqlite3BackupRestart(&list)`
//! when its cache is reset because another connection changed the file;
//! it must clone the list and release its own borrow before doing so.
//! The update path only touches the destination btree/pager (the source
//! page size is cached in the Backup object), never the source Btree.

use crate::btree::*;
use crate::int::*;
use crate::pager::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Handle on a backup operation (C `sqlite3_backup*`).
pub type BackupP = Rc<RefCell<Backup>>;

/// Structure allocated for each backup operation.
pub struct Backup {
    /// C `pDestDb!=0`: false when the object is used internally by
    /// sqlite3BtreeCopyFile() (no destination connection).
    pub hasDestDb: bool,
    /// Name of the destination database ("main", ...).
    pub zDestDb: ZStr,
    /// Destination b-tree file
    pub pDest: Option<BtreeP>,
    /// Original schema cookie in destination
    pub iDestSchema: u32,
    /// True once a write-transaction is open on pDest
    pub bDestLocked: i32,
    /// Page number of the next source page to copy
    pub iNext: Pgno,
    /// Source b-tree file
    pub pSrc: BtreeP,
    /// Backup process error code
    pub rc: i32,
    /// Number of pages left to copy (set by every backup_step())
    pub nRemaining: Pgno,
    /// Total number of pages to copy (set by every backup_step())
    pub nPagecount: Pgno,
    /// True once backup has been registered with pager
    pub isAttached: i32,
    /// Page size of the source database (cached by backup_step() for
    /// sqlite3BackupUpdate()).
    pub nSrcPgsz: i32,
}

// ---------------------------------------------------------------------------
// Integration points with btree.c internals.
// ---------------------------------------------------------------------------

/// C `p->nBackup += delta` (Btree.nBackup, read by sqlite3BtreeIsInBackup()).
///
/// FIXME(integrator): btree.rs's Btree is still opaque in the skeleton; make
/// this update the Btree::nBackup field.
fn btree_nbackup_add(p: &BtreeP, delta: i32) {
    let _ = (p, delta);
    todo!("Btree::nBackup += delta")
}

/// C `pTo->pBt->btsFlags &= ~BTS_PAGESIZE_FIXED`.
///
/// FIXME(integrator): BtShared is still opaque in the skeleton.
fn btree_clear_pagesize_fixed(p: &BtreeP) {
    let _ = p;
    todo!("pBt->btsFlags &= ~BTS_PAGESIZE_FIXED")
}

/// PENDING_BYTE_PAGE(pBt)
fn pending_byte_page(p: &BtreeP) -> Pgno {
    let pgsz = sqlite3BtreeGetPageSize(p) as i64;
    ((crate::global::sqlite3PendingByte / pgsz) + 1) as Pgno
}

/// The error of a findBtree() failure: (code, message) to store in the
/// error connection.
type FindErr = (i32, ZStr);

/// Return a pointer corresponding to database zDb (i.e. "main", "temp")
/// in connection handle pDb. If such a database cannot be found, return
/// the error to write into the error connection.
///
/// If the "temp" database is requested, it may need to be opened by this
/// function.
fn find_btree_in(pDb: &mut Sqlite3, zDb: Option<&[u8]>) -> Result<Option<BtreeP>, FindErr> {
    let i = crate::build::sqlite3FindDbName(pDb, zDb);
    if i == 1 {
        let mut err: Option<FindErr> = None;
        crate::prepare::sqlite3ParseObjectInit(pDb);
        if crate::build::sqlite3OpenTempDatabase(pDb) != 0 {
            let p = pDb.ppr();
            err = Some((p.rc, p.zErrMsg.clone().unwrap_or_default()));
        }
        crate::prepare::sqlite3ParseObjectReset(pDb);
        if let Some(e) = err {
            return Err(e);
        }
    }

    if i < 0 {
        let z = mprintf!(pDb, "unknown database %s", zDb);
        return Err((SQLITE_ERROR, z));
    }

    Ok(pDb.aDb[i as usize].pBt.clone())
}

/// C `findBtree(pErrorDb, pDb, zDb)`; pDb==None means pDb is pErrorDb.
fn findBtree(
    pErrorDb: &mut Sqlite3,
    pDb: Option<&mut Sqlite3>,
    zDb: Option<&[u8]>,
) -> Option<BtreeP> {
    let r = match pDb {
        Some(d) => find_btree_in(d, zDb),
        None => find_btree_in(pErrorDb, zDb),
    };
    match r {
        Ok(p) => p,
        Err((rc, msg)) => {
            crate::util::sqlite3ErrorWithMsg(
                pErrorDb,
                rc,
                Some(b"%s"),
                &[crate::printf::PfArg::from(&msg)],
            );
            None
        }
    }
}

/// Attempt to set the page size of the destination to match the page size
/// of the source.
fn setDestPgsz(pDest: &BtreeP, pSrc: &BtreeP) -> i32 {
    sqlite3BtreeSetPageSize(pDest, sqlite3BtreeGetPageSize(pSrc), 0, 0)
}

/// Check that there is no open read-transaction on the b-tree passed as
/// the second argument. If there is not, return SQLITE_OK. Otherwise, if
/// there is an open read-transaction, return SQLITE_ERROR and leave an
/// error message in database handle db.
fn checkReadTransaction(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    if sqlite3BtreeTxnState(db, p) != SQLITE_TXN_NONE {
        crate::util::sqlite3ErrorWithMsg(
            db,
            SQLITE_ERROR,
            Some(b"destination database is in use"),
            &[],
        );
        return SQLITE_ERROR;
    }
    SQLITE_OK
}

/// C: `sqlite3_backup * sqlite3_backup_init( sqlite3* pDestDb, const char *zDestDb, sqlite3* pSrcDb, const char *zSrcDb )`
///
/// Create an sqlite3_backup process to copy the contents of zSrcDb from
/// connection handle pSrcDb to zDestDb in pDestDb. If successful, return
/// a pointer to the new sqlite3_backup object.
///
/// If an error occurs, NULL is returned and an error code and error
/// message stored in database handle pDestDb.
///
/// Rust: `pSrcDb==None` means that the source connection is pDestDb itself.
pub fn sqlite3_backup_init(
    pDestDb: &mut Sqlite3,
    zDestDb: Option<&[u8]>,
    pSrcDb: Option<&mut Sqlite3>,
    zSrcDb: Option<&[u8]>,
) -> Option<BackupP> {
    let pSrcDb = match pSrcDb {
        None => {
            crate::util::sqlite3ErrorWithMsg(
                pDestDb,
                SQLITE_ERROR,
                Some(b"source and destination must be distinct"),
                &[],
            );
            return None;
        }
        Some(s) => s,
    };

    // Do not store the pointer to the destination b-tree at this point.
    // This is because there is nothing preventing it from being detached
    // or otherwise freed before the first call to sqlite3_backup_step()
    // on this object. The source b-tree does not have this problem, as
    // incrementing Btree.nBackup (see below) effectively locks the object.
    let pDest = findBtree(pDestDb, None, zDestDb);
    let pSrc = findBtree(pDestDb, Some(pSrcDb), zSrcDb);

    let (pSrc, pDest) = match (pSrc, pDest) {
        (Some(s), Some(d)) => (s, d),
        _ => return None,
    };
    if checkReadTransaction(pDestDb, &pDest) != SQLITE_OK {
        // There is a transaction open on the destination database. The
        // error has already been written into the pDestDb handle.
        return None;
    }

    let zDest = zDestDb.unwrap_or(b"");
    let n = zDest.iter().position(|&c| c == 0).unwrap_or(zDest.len());
    let p = Backup {
        hasDestDb: true,
        zDestDb: zDest[..n].to_vec(),
        pDest: None,
        iDestSchema: 0,
        bDestLocked: 0,
        iNext: 1,
        pSrc: pSrc.clone(),
        rc: SQLITE_OK,
        nRemaining: 0,
        nPagecount: 0,
        isAttached: 0,
        nSrcPgsz: 0,
    };
    btree_nbackup_add(&pSrc, 1);
    Some(Rc::new(RefCell::new(p)))
}

/// Argument rc is an SQLite error code. Return true if this error is
/// considered fatal if encountered during a backup operation. All errors
/// are considered fatal except for SQLITE_BUSY and SQLITE_LOCKED.
fn isFatalError(rc: i32) -> bool {
    rc != SQLITE_OK && rc != SQLITE_BUSY && rc != SQLITE_LOCKED
}

/// Parameter zSrcData points to a buffer containing the data for
/// page iSrcPg from the source database. Copy this data into the
/// destination database.
///
/// Rust: `pSrc` is only used when `bUpdate==0` (to read its page count);
/// `nSrcPgsz` is the source page size.
fn backupOnePage(
    pDest: &BtreeP,
    pSrc: Option<&BtreeP>,
    nSrcPgsz: i32,
    iSrcPg: Pgno,
    zSrcData: &[u8],
    bUpdate: i32,
) -> i32 {
    let pDestPager = sqlite3BtreePager(pDest);
    let nDestPgsz = sqlite3BtreeGetPageSize(pDest);
    let nCopy = nSrcPgsz.min(nDestPgsz) as usize;
    let iEnd = iSrcPg as i64 * nSrcPgsz as i64;
    let mut rc = SQLITE_OK;
    let iPendingDest = pending_byte_page(pDest);

    // This loop runs once for each destination page spanned by the source
    // page. For each iteration, variable iOff is set to the byte offset
    // of the destination page.
    let mut iOff = iEnd - nSrcPgsz as i64;
    while rc == SQLITE_OK && iOff < iEnd {
        let iDest = (iOff / nDestPgsz as i64) as Pgno + 1;
        if iDest != iPendingDest {
            let mut pDestPg: Option<DbPage> = None;
            rc = {
                let mut pg = pDestPager.borrow_mut();
                sqlite3PagerGet(&mut pg, iDest, &mut pDestPg, 0)
            };
            if rc == SQLITE_OK {
                rc = sqlite3PagerWrite(pDestPg.as_ref().expect("page"));
            }
            if rc == SQLITE_OK {
                let nLast = if iOff == 0 && bUpdate == 0 {
                    pSrc.map(sqlite3BtreeLastPage)
                } else {
                    None
                };
                let pg = pDestPg.as_ref().expect("page");
                let mut h = pg.borrow_mut();
                let zIn = (iOff % nSrcPgsz as i64) as usize;
                let zOut = (iOff % nDestPgsz as i64) as usize;

                // Copy the data from the source page into the destination
                // page.  Then clear the Btree layer MemPage.isInit flag. Both
                // this module and the pager code use this trick (clearing the
                // first byte of the page 'extra' space to invalidate the
                // Btree layers cached parse of the page).
                //
                // Rust: the whole MemPage is reset to its default state,
                // which has isInit==0 (and forces the btree to re-read the
                // page header).
                h.aData[zOut..zOut + nCopy].copy_from_slice(&zSrcData[zIn..zIn + nCopy]);
                h.pExtra = MemPage::default();
                if let Some(n) = nLast {
                    crate::util::sqlite3Put4byte(&mut h.aData[zOut + 28..], n);
                }
            }
            if let Some(pg) = pDestPg.as_ref() {
                sqlite3PagerUnref(pg);
            }
        }
        iOff += nDestPgsz as i64;
    }

    rc
}

/// If pFile is currently larger than iSize bytes, then truncate it to
/// exactly iSize bytes. If pFile is not larger than iSize bytes, then
/// this function is a no-op.
///
/// Return SQLITE_OK if everything is successful, or an SQLite error
/// code if an error occurs.
fn backupTruncateFile(pFile: &mut crate::os::SqliteFile, iSize: i64) -> i32 {
    let mut iCurrent = 0i64;
    let mut rc = crate::os::sqlite3OsFileSize(pFile, &mut iCurrent);
    if rc == SQLITE_OK && iCurrent > iSize {
        rc = crate::os::sqlite3OsTruncate(pFile, iSize);
    }
    rc
}

/// Register this backup object with the associated source pager for
/// callbacks when pages are changed or the cache invalidated.
fn attachBackupObject(p: &BackupP) {
    let pSrc = p.borrow().pSrc.clone();
    let pPager = sqlite3BtreePager(&pSrc);
    {
        let mut pg = pPager.borrow_mut();
        sqlite3PagerBackupPtr(&mut pg).insert(0, Rc::downgrade(p));
    }
    p.borrow_mut().isAttached = 1;
}

/// C: `int sqlite3_backup_step(sqlite3_backup *p, int nPage)`
///
/// Copy nPage pages from the source b-tree to the destination.
pub fn sqlite3_backup_step(
    pDestDb: &mut Sqlite3,
    pSrcDb: &mut Sqlite3,
    p: &BackupP,
    nPage: i32,
) -> i32 {
    backup_step(Some(pDestDb), pSrcDb, p, nPage)
}

/// sqlite3_backup_step() with an optional destination connection (None:
/// the internal use by sqlite3BtreeCopyFile(), where both b-trees belong
/// to pSrcDb).
fn backup_step(
    mut pDestDb: Option<&mut Sqlite3>,
    pSrcDb: &mut Sqlite3,
    pp: &BackupP,
    nPage: i32,
) -> i32 {
    // The connection that owns the destination b-tree.
    macro_rules! ddb {
        () => {
            match pDestDb.as_deref_mut() {
                Some(d) => d,
                None => &mut *pSrcDb,
            }
        };
    }
    let hasDestDb = pp.borrow().hasDestDb;
    let pSrc = pp.borrow().pSrc.clone();

    let mut rc = pp.borrow().rc;
    if !isFatalError(rc) {
        let pSrcPager = sqlite3BtreePager(&pSrc); // Source pager
        let mut pDestPager: Option<PagerP> = None; // Dest pager
        let mut bCloseTrans = false; // True if src db requires unlocking
        let mut destMode = 0; // Destination journal mode
        let mut pgszSrc = 0; // Source page size
        let mut pgszDest = 0; // Destination page size

        // If the source pager is currently in a write-transaction, return
        // SQLITE_BUSY immediately.
        if hasDestDb && sqlite3BtreeTxnState(pSrcDb, &pSrc) == SQLITE_TXN_WRITE {
            rc = SQLITE_BUSY;
        } else {
            rc = SQLITE_OK;
        }

        // If there is no open read-transaction on the source database, open
        // one now. If a transaction is opened here, then it will be closed
        // before this function exits.
        if rc == SQLITE_OK && SQLITE_TXN_NONE == sqlite3BtreeTxnState(pSrcDb, &pSrc) {
            let mut dummy = 0;
            rc = sqlite3BtreeBeginTrans(pSrcDb, &pSrc, 0, &mut dummy);
            bCloseTrans = true;
        }

        // Locate the destination btree and pager.
        let mut pDest = pp.borrow().pDest.clone();
        if pDest.is_none() {
            let zDestDb = pp.borrow().zDestDb.clone();
            if let Some(d) = pDestDb.as_deref_mut() {
                pDest = findBtree(d, None, Some(&zDestDb));
            }
        }
        match pDest.as_ref() {
            None => rc = SQLITE_ERROR,
            Some(d) => pDestPager = Some(sqlite3BtreePager(d)),
        }

        // If the destination database has not yet been locked (i.e. if this
        // is the first call to backup_step() for the current backup
        // operation), try to set its page size to the same as the source
        // database. This is especially important on ZipVFS systems, as in
        // that case it is not possible to create a database file that uses
        // one page size by writing to it with another.
        let bDestLocked = pp.borrow().bDestLocked;
        if bDestLocked == 0
            && rc == SQLITE_OK
            && setDestPgsz(pDest.as_ref().unwrap(), &pSrc) == SQLITE_NOMEM
        {
            rc = SQLITE_NOMEM;
        }

        // Lock the destination database, if it is not locked already.
        if SQLITE_OK == rc && bDestLocked == 0 {
            let mut iDestSchema = 0i32;
            rc = sqlite3BtreeBeginTrans(ddb!(), pDest.as_ref().unwrap(), 2, &mut iDestSchema);
            if rc == SQLITE_OK {
                let mut p = pp.borrow_mut();
                p.iDestSchema = iDestSchema as u32;
                p.bDestLocked = 1;
                p.pDest = pDest.clone();
            }
        }

        // Do not allow backup if the destination database is in WAL mode
        // and the page sizes are different between source and destination
        if rc == SQLITE_OK {
            let pD = pp.borrow().pDest.clone().unwrap();
            pgszSrc = sqlite3BtreeGetPageSize(&pSrc);
            pgszDest = sqlite3BtreeGetPageSize(&pD);
            let isMemdb;
            {
                let pagerD = sqlite3BtreePager(&pD);
                let mut pg = pagerD.borrow_mut();
                destMode = sqlite3PagerGetJournalMode(&mut pg);
                isMemdb = sqlite3PagerIsMemdb(&mut pg) != 0;
            }
            if (destMode == PAGER_JOURNALMODE_WAL || isMemdb) && pgszSrc != pgszDest {
                rc = SQLITE_READONLY;
            }
        }
        pp.borrow_mut().nSrcPgsz = sqlite3BtreeGetPageSize(&pSrc);

        // Now that there is a read-lock on the source database, query the
        // source pager for the number of pages in the database.
        let mut nSrcPage = sqlite3BtreeLastPage(&pSrc) as i32;
        debug_assert!(nSrcPage >= 0);
        let iPendingSrc = pending_byte_page(&pSrc);
        let mut ii = 0;
        while (nPage < 0 || ii < nPage) && pp.borrow().iNext <= nSrcPage as Pgno && rc == 0 {
            let iSrcPg = pp.borrow().iNext; // Source page number
            if iSrcPg != iPendingSrc {
                let mut pSrcPg: Option<DbPage> = None; // Source page object
                rc = {
                    let mut pg = pSrcPager.borrow_mut();
                    sqlite3PagerGet(&mut pg, iSrcPg, &mut pSrcPg, PAGER_GET_READONLY)
                };
                if rc == SQLITE_OK {
                    let pg = pSrcPg.as_ref().expect("page");
                    let data = pg.borrow().aData.clone();
                    let pD = pp.borrow().pDest.clone().unwrap();
                    let nSrcPgsz = pp.borrow().nSrcPgsz;
                    rc = backupOnePage(&pD, Some(&pSrc), nSrcPgsz, iSrcPg, &data, 0);
                    sqlite3PagerUnref(pg);
                }
            }
            pp.borrow_mut().iNext += 1;
            ii += 1;
        }
        if rc == SQLITE_OK {
            let (iNext, isAttached) = {
                let mut p = pp.borrow_mut();
                p.nPagecount = nSrcPage as Pgno;
                p.nRemaining = (nSrcPage as Pgno + 1).wrapping_sub(p.iNext);
                (p.iNext, p.isAttached)
            };
            if iNext > nSrcPage as Pgno {
                rc = SQLITE_DONE;
            } else if isAttached == 0 {
                attachBackupObject(pp);
            }
        }

        // Update the schema version field in the destination database. This
        // is to make sure that the schema-version really does change in
        // the case where the source and destination databases have the
        // same schema version.
        if rc == SQLITE_DONE {
            let pD = pp.borrow().pDest.clone().unwrap();
            let pDestPager = pDestPager.clone().unwrap();
            if nSrcPage == 0 {
                rc = sqlite3BtreeNewDb(&pD);
                nSrcPage = 1;
            }
            if rc == SQLITE_OK || rc == SQLITE_DONE {
                let iDestSchema = pp.borrow().iDestSchema;
                rc = sqlite3BtreeUpdateMeta(&pD, 1, iDestSchema.wrapping_add(1));
            }
            if rc == SQLITE_OK {
                if let Some(d) = pDestDb.as_deref_mut() {
                    crate::build::sqlite3ResetAllSchemasOfConnection(d);
                }
                if destMode == PAGER_JOURNALMODE_WAL {
                    rc = sqlite3BtreeSetVersion(&pD, 2);
                }
            }
            if rc == SQLITE_OK {
                // Set nDestTruncate to the final number of pages in the
                // destination database. The complication here is that the
                // destination page size may be different to the source page
                // size.
                //
                // If the source page size is smaller than the destination
                // page size, round up. In this case the call to
                // sqlite3OsTruncate() below will fix the size of the file.
                // However it is important to call sqlite3PagerTruncateImage()
                // here so that any pages in the destination file that lie
                // beyond the nDestTruncate page mark are journalled by
                // PagerCommitPhaseOne() before they are destroyed by the
                // file truncation.
                let iPendingDest = pending_byte_page(&pD);
                let mut nDestTruncate: i32;
                if pgszSrc < pgszDest {
                    let ratio = pgszDest / pgszSrc;
                    nDestTruncate = (nSrcPage + ratio - 1) / ratio;
                    if nDestTruncate == iPendingDest as i32 {
                        nDestTruncate -= 1;
                    }
                } else {
                    nDestTruncate = nSrcPage * (pgszSrc / pgszDest);
                }
                debug_assert!(nDestTruncate > 0);

                if pgszSrc < pgszDest {
                    // If the source page-size is smaller than the destination
                    // page-size, two extra things may need to happen:
                    //
                    //   * The destination may need to be truncated, and
                    //
                    //   * Data stored on the pages immediately following the
                    //     pending-byte page in the source database may need to
                    //     be copied into the destination database.
                    let iSize = pgszSrc as i64 * nSrcPage as i64;

                    // This block ensures that all data required to recreate
                    // the original database has been stored in the journal
                    // for pDestPager and the journal synced to disk. So at
                    // this point we may safely modify the database file in
                    // any way, knowing that if a power failure occurs, the
                    // original database will be reconstructed from the
                    // journal file.
                    let mut nDstPage = 0i32;
                    {
                        let mut pg = pDestPager.borrow_mut();
                        sqlite3PagerPagecount(&mut pg, &mut nDstPage);
                    }
                    let mut iPg = nDestTruncate as Pgno;
                    while rc == SQLITE_OK && iPg <= nDstPage as Pgno {
                        if iPg != iPendingDest {
                            let mut pPg: Option<DbPage> = None;
                            rc = {
                                let mut pg = pDestPager.borrow_mut();
                                sqlite3PagerGet(&mut pg, iPg, &mut pPg, 0)
                            };
                            if rc == SQLITE_OK {
                                let pg = pPg.as_ref().expect("page");
                                rc = sqlite3PagerWrite(pg);
                                sqlite3PagerUnref(pg);
                            }
                        }
                        iPg += 1;
                    }
                    if rc == SQLITE_OK {
                        let mut pg = pDestPager.borrow_mut();
                        rc = sqlite3PagerCommitPhaseOne(&mut pg, None, 1);
                    }

                    // Write the extra pages and truncate the database file as
                    // required
                    let iEnd = (crate::global::sqlite3PendingByte + pgszDest as i64).min(iSize);
                    let mut iOff = crate::global::sqlite3PendingByte + pgszSrc as i64;
                    while rc == SQLITE_OK && iOff < iEnd {
                        let mut pSrcPg: Option<DbPage> = None;
                        let iSrcPg = ((iOff / pgszSrc as i64) + 1) as Pgno;
                        rc = {
                            let mut pg = pSrcPager.borrow_mut();
                            sqlite3PagerGet(&mut pg, iSrcPg, &mut pSrcPg, 0)
                        };
                        if rc == SQLITE_OK {
                            let zData = pSrcPg.as_ref().unwrap().borrow().aData.clone();
                            let mut pg = pDestPager.borrow_mut();
                            let f = sqlite3PagerFile(&mut pg);
                            rc = crate::os::sqlite3OsWrite(f, &zData, pgszSrc, iOff);
                        }
                        if let Some(pg) = pSrcPg.as_ref() {
                            sqlite3PagerUnref(pg);
                        }
                        iOff += pgszSrc as i64;
                    }
                    if rc == SQLITE_OK {
                        let mut pg = pDestPager.borrow_mut();
                        let f = sqlite3PagerFile(&mut pg);
                        rc = backupTruncateFile(f, iSize);
                    }

                    // Sync the database file to disk.
                    if rc == SQLITE_OK {
                        let mut pg = pDestPager.borrow_mut();
                        rc = sqlite3PagerSync(&mut pg, None);
                    }
                } else {
                    let mut pg = pDestPager.borrow_mut();
                    sqlite3PagerTruncateImage(&mut pg, nDestTruncate as Pgno);
                    rc = sqlite3PagerCommitPhaseOne(&mut pg, None, 0);
                }

                // Finish committing the transaction to the destination
                // database.
                if SQLITE_OK == rc {
                    rc = sqlite3BtreeCommitPhaseTwo(&pD, 0);
                    if rc == SQLITE_OK {
                        rc = SQLITE_DONE;
                    }
                }
            }
        }

        // If bCloseTrans is true, then this function opened a read
        // transaction on the source database. Close the read transaction
        // here. There is no need to check the return values of the btree
        // methods here, as "committing" a read-only transaction cannot
        // fail.
        if bCloseTrans {
            sqlite3BtreeCommitPhaseOne(&pSrc, None);
            sqlite3BtreeCommitPhaseTwo(&pSrc, 0);
        }

        if rc == SQLITE_IOERR_NOMEM {
            rc = SQLITE_NOMEM;
        }
        pp.borrow_mut().rc = rc;
    }
    rc
}

/// C: `int sqlite3_backup_finish(sqlite3_backup *p)`
///
/// Release all resources associated with an sqlite3_backup* handle.
pub fn sqlite3_backup_finish(pDestDb: &mut Sqlite3, pSrcDb: &mut Sqlite3, p: BackupP) -> i32 {
    backup_finish(Some(pDestDb), pSrcDb, &p)
}

fn backup_finish(pDestDb: Option<&mut Sqlite3>, pSrcDb: &mut Sqlite3, pp: &BackupP) -> i32 {
    let (hasDestDb, pSrc, isAttached, pDest, prc) = {
        let p = pp.borrow();
        (
            p.hasDestDb,
            p.pSrc.clone(),
            p.isAttached,
            p.pDest.clone(),
            p.rc,
        )
    };

    // Detach this backup from the source pager.
    if hasDestDb {
        btree_nbackup_add(&pSrc, -1);
    }
    if isAttached != 0 {
        let pPager = sqlite3BtreePager(&pSrc);
        let mut pg = pPager.borrow_mut();
        let list = sqlite3PagerBackupPtr(&mut pg);
        if let Some(i) = list
            .iter()
            .position(|b| std::ptr::eq(b.as_ptr(), Rc::as_ptr(pp)))
        {
            list.remove(i);
        }
    }
    pp.borrow_mut().isAttached = 0;

    // If a transaction is still open on the Btree, roll it back.
    if let Some(d) = pDest.as_ref() {
        sqlite3BtreeRollback(d, SQLITE_OK, 0);
    }

    // Set the error code of the destination database handle.
    let rc = if prc == SQLITE_DONE { SQLITE_OK } else { prc };
    if let Some(d) = pDestDb {
        crate::util::sqlite3Error(d, rc);

        // Exit the mutexes and free the backup context structure.
        crate::mainapi::sqlite3LeaveMutexAndCloseZombie(d);
    }
    crate::mainapi::sqlite3LeaveMutexAndCloseZombie(pSrcDb);
    rc
}

/// C: `int sqlite3_backup_remaining(sqlite3_backup *p)`
///
/// Return the number of pages still to be backed up as of the most recent
/// call to sqlite3_backup_step().
pub fn sqlite3_backup_remaining(p: &BackupP) -> i32 {
    p.borrow().nRemaining as i32
}

/// C: `int sqlite3_backup_pagecount(sqlite3_backup *p)`
///
/// Return the total number of pages in the source database as of the most
/// recent call to sqlite3_backup_step().
pub fn sqlite3_backup_pagecount(p: &BackupP) -> i32 {
    p.borrow().nPagecount as i32
}

/// C: `void sqlite3BackupUpdate(sqlite3_backup *pBackup, Pgno iPage, const u8 *aData)`
///
/// This function is called after the contents of page iPage of the
/// source database have been modified. If page iPage has already been
/// copied into the destination database, then the data written to the
/// destination is now invalidated. The destination copy of iPage needs
/// to be updated with the new data before the backup operation is
/// complete.
///
/// Rust: `pBackup` is the source pager's backup list (C Pager.pBackup,
/// a `Vec<Weak<..>>`), upgraded by the pager before the call.
pub fn sqlite3BackupUpdate(pBackup: &[BackupP], iPage: Pgno, aData: &[u8]) {
    for p in pBackup.iter() {
        let (rc, iNext, pDest, nSrcPgsz) = {
            let b = p.borrow();
            (b.rc, b.iNext, b.pDest.clone(), b.nSrcPgsz)
        };
        if !isFatalError(rc) && iPage < iNext {
            // The backup process p has already copied page iPage. But now it
            // has been modified by a transaction on the source pager. Copy
            // the new data into the backup.
            if let Some(pDest) = pDest {
                let rc = backupOnePage(&pDest, None, nSrcPgsz, iPage, aData, 1);
                debug_assert!(rc != SQLITE_BUSY && rc != SQLITE_LOCKED);
                if rc != SQLITE_OK {
                    p.borrow_mut().rc = rc;
                }
            }
        }
    }
}

/// C: `void sqlite3BackupRestart(sqlite3_backup *pBackup)`
///
/// Restart the backup process. This is called when the pager layer
/// detects that the database has been modified by an external database
/// connection. In this case there is no way of knowing which of the
/// pages that have been copied into the destination database are still
/// valid and which are not, so the entire process needs to be restarted.
pub fn sqlite3BackupRestart(pBackup: &[BackupP]) {
    for p in pBackup.iter() {
        p.borrow_mut().iNext = 1;
    }
}

/// C: `int sqlite3BtreeCopyFile(Btree *pTo, Btree *pFrom)`
///
/// Copy the complete content of pBtFrom into pBtTo.  A transaction
/// must be active for both files.
///
/// The size of file pTo may be reduced by this operation. If anything
/// goes wrong, the transaction on pTo is rolled back. If successful, the
/// transaction is committed before returning.
///
/// Rust: `db` is the connection owning both b-trees.
pub fn sqlite3BtreeCopyFile(db: &mut Sqlite3, pTo: &BtreeP, pFrom: &BtreeP) -> i32 {
    let mut rc;
    debug_assert!(sqlite3BtreeTxnState(db, pTo) == SQLITE_TXN_WRITE);
    {
        let nByte = sqlite3BtreeGetPageSize(pFrom) as i64 * sqlite3BtreeLastPage(pFrom) as i64;
        let pPager = sqlite3BtreePager(pTo);
        let mut pg = pPager.borrow_mut();
        let pFd = sqlite3PagerFile(&mut pg);
        if crate::os::isOpen(pFd) {
            let mut arg = nByte;
            rc = crate::os::sqlite3OsFileControl(
                pFd,
                SQLITE_FCNTL_OVERWRITE,
                crate::os::FcntlArg::I64(&mut arg),
            );
            if rc == SQLITE_NOTFOUND {
                rc = SQLITE_OK;
            }
            if rc != 0 {
                return rc;
            }
        }
    }

    // Set up an sqlite3_backup object. sqlite3_backup.pDestDb must be set
    // to 0. This is used by the implementations of sqlite3_backup_step()
    // and sqlite3_backup_finish() to detect that they are being called
    // from this function, not directly by the user.
    let b = Rc::new(RefCell::new(Backup {
        hasDestDb: false,
        zDestDb: Vec::new(),
        pDest: Some(pTo.clone()),
        iDestSchema: 0,
        bDestLocked: 0,
        iNext: 1,
        pSrc: pFrom.clone(),
        rc: SQLITE_OK,
        nRemaining: 0,
        nPagecount: 0,
        isAttached: 0,
        nSrcPgsz: 0,
    }));

    // 0x7FFFFFFF is the hard limit for the number of pages in a database
    // file. By passing this as the number of pages to copy to
    // sqlite3_backup_step(), we can guarantee that the copy finishes
    // within a single call (unless an error occurs).
    backup_step(None, db, &b, 0x7FFFFFFF);
    debug_assert!(b.borrow().rc != SQLITE_OK);

    rc = backup_finish(None, db, &b);
    if rc == SQLITE_OK {
        btree_clear_pagesize_fixed(pTo);
    } else {
        let pDest = b.borrow().pDest.clone().unwrap();
        let pPager = sqlite3BtreePager(&pDest);
        let mut pg = pPager.borrow_mut();
        sqlite3PagerClearCache(&mut pg);
    }

    debug_assert!(sqlite3BtreeTxnState(db, pTo) != SQLITE_TXN_WRITE);
    rc
}
