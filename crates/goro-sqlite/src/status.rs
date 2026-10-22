//! Port of status.c.
//!
//! This module implements the sqlite3_status() interface and related
//! functionality.
//!
//! Rust: there is no lookaside allocator and no memory accounting, so the
//! lookaside and memory-size counters are always zero.  The global status
//! counters are kept (they are thread-local, the port has no shared
//! global state between threads).

use crate::int::*;
use std::cell::RefCell;

/// Variables in which to record status information.
#[derive(Default, Clone, Copy)]
struct sqlite3StatType {
    /// Current value
    nowValue: [i64; 10],
    /// Maximum value
    mxValue: [i64; 10],
}

thread_local! {
    static sqlite3Stat: RefCell<sqlite3StatType> = RefCell::new(sqlite3StatType::default());
}

/// C: `sqlite3_int64 sqlite3StatusValue(int op)`
///
/// Return the current value of a status parameter.
pub fn sqlite3StatusValue(op: i32) -> i64 {
    debug_assert!(op >= 0 && op < 10);
    sqlite3Stat.with(|s| s.borrow().nowValue[op as usize])
}

/// C: `void sqlite3StatusUp(int op, int N)`
///
/// Add N to the value of a status record.
pub fn sqlite3StatusUp(op: i32, N: i32) {
    debug_assert!(op >= 0 && op < 10);
    sqlite3Stat.with(|s| {
        let mut s = s.borrow_mut();
        let op = op as usize;
        s.nowValue[op] += N as i64;
        if s.nowValue[op] > s.mxValue[op] {
            s.mxValue[op] = s.nowValue[op];
        }
    })
}

/// C: `void sqlite3StatusDown(int op, int N)`
pub fn sqlite3StatusDown(op: i32, N: i32) {
    debug_assert!(N >= 0);
    debug_assert!(op >= 0 && op < 10);
    sqlite3Stat.with(|s| s.borrow_mut().nowValue[op as usize] -= N as i64)
}

/// C: `void sqlite3StatusHighwater(int op, int X)`
///
/// Adjust the highwater mark if necessary.
pub fn sqlite3StatusHighwater(op: i32, X: i32) {
    debug_assert!(X >= 0);
    let newValue = X as i64;
    debug_assert!(op >= 0 && op < 10);
    debug_assert!(
        op == SQLITE_STATUS_MALLOC_SIZE
            || op == SQLITE_STATUS_PAGECACHE_SIZE
            || op == SQLITE_STATUS_PARSER_STACK
    );
    sqlite3Stat.with(|s| {
        let mut s = s.borrow_mut();
        if newValue > s.mxValue[op as usize] {
            s.mxValue[op as usize] = newValue;
        }
    })
}

/// C: `int sqlite3_status64( int op, sqlite3_int64 *pCurrent, sqlite3_int64 *pHighwater, int resetFlag )`
///
/// Query status information.
pub fn sqlite3_status64(op: i32, pCurrent: &mut i64, pHighwater: &mut i64, resetFlag: i32) -> i32 {
    if op < 0 || op >= 10 {
        return SQLITE_MISUSE;
    }
    sqlite3Stat.with(|s| {
        let mut s = s.borrow_mut();
        let op = op as usize;
        *pCurrent = s.nowValue[op];
        *pHighwater = s.mxValue[op];
        if resetFlag != 0 {
            s.mxValue[op] = s.nowValue[op];
        }
    });
    SQLITE_OK
}

/// C: `int sqlite3_status(int op, int *pCurrent, int *pHighwater, int resetFlag)`
pub fn sqlite3_status(op: i32, pCurrent: &mut i32, pHighwater: &mut i32, resetFlag: i32) -> i32 {
    let mut iCur: i64 = 0;
    let mut iHwtr: i64 = 0;
    let rc = sqlite3_status64(op, &mut iCur, &mut iHwtr, resetFlag);
    if rc == 0 {
        *pCurrent = iCur as i32;
        *pHighwater = iHwtr as i32;
    }
    rc
}

/// C: `int sqlite3LookasideUsed(sqlite3 *db, int *pHighwater)`
///
/// Return the number of LookasideSlot elements on the linked list.
/// Rust: there is no lookaside memory; always 0.
pub fn sqlite3LookasideUsed(db: &mut Sqlite3, pHighwater: &mut i32) -> i32 {
    let _ = db;
    *pHighwater = 0;
    0
}

/// C: `int sqlite3_db_status64( sqlite3 *db, int op, sqlite3_int64 *pCurrent, sqlite3_int64 *pHighwtr, int resetFlag )`
///
/// Query status information for a single database connection
pub fn sqlite3_db_status64(
    db: &mut Sqlite3,
    op: i32,
    pCurrent: &mut i64,
    pHighwtr: &mut i64,
    resetFlag: i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut op = op;
    match op {
        SQLITE_DBSTATUS_LOOKASIDE_USED => {
            let mut H = 0;
            *pCurrent = sqlite3LookasideUsed(db, &mut H) as i64;
            *pHighwtr = H as i64;
        }

        SQLITE_DBSTATUS_LOOKASIDE_HIT
        | SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE
        | SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL => {
            // Rust: no lookaside, the anStat[] counters are always zero.
            *pCurrent = 0;
            *pHighwtr = 0;
        }

        // Return an approximation for the amount of memory currently used
        // by all pagers associated with the given database connection.  The
        // highwater mark is meaningless and is returned as zero.
        SQLITE_DBSTATUS_CACHE_USED_SHARED | SQLITE_DBSTATUS_CACHE_USED => {
            let mut totalUsed: i64 = 0;
            for i in 0..db.aDb.len() {
                if let Some(pBt) = db.aDb[i].pBt.clone() {
                    let pPager = crate::btree::sqlite3BtreePager(&pBt);
                    let mut nByte = crate::pager::sqlite3PagerMemUsed(&mut pPager.borrow_mut());
                    if op == SQLITE_DBSTATUS_CACHE_USED_SHARED {
                        nByte /= crate::btree::sqlite3BtreeConnectionCount(&pBt);
                    }
                    totalUsed += nByte as i64;
                }
            }
            *pCurrent = totalUsed;
            *pHighwtr = 0;
        }

        // *pCurrent gets an accurate estimate of the amount of memory used
        // to store the schema for all databases (main, temp, and any ATTACHed
        // databases.  *pHighwater is set to zero.
        // Rust: memory is not accounted; report zero.
        SQLITE_DBSTATUS_SCHEMA_USED => {
            *pHighwtr = 0;
            *pCurrent = 0;
        }

        // *pCurrent gets an accurate estimate of the amount of memory used
        // to store all prepared statements.
        // *pHighwater is set to zero.
        // Rust: memory is not accounted; report zero.
        SQLITE_DBSTATUS_STMT_USED => {
            *pHighwtr = 0;
            *pCurrent = 0;
        }

        // Set *pCurrent to the total cache hits or misses encountered by all
        // pagers the database handle is connected to. *pHighwater is always
        // set to zero.
        SQLITE_DBSTATUS_CACHE_SPILL
        | SQLITE_DBSTATUS_CACHE_HIT
        | SQLITE_DBSTATUS_CACHE_MISS
        | SQLITE_DBSTATUS_CACHE_WRITE => {
            if op == SQLITE_DBSTATUS_CACHE_SPILL {
                op = SQLITE_DBSTATUS_CACHE_WRITE + 1;
            }
            let mut nRet: u64 = 0;
            for i in 0..db.aDb.len() {
                if let Some(pBt) = db.aDb[i].pBt.clone() {
                    let pPager = crate::btree::sqlite3BtreePager(&pBt);
                    crate::pager::sqlite3PagerCacheStat(
                        &mut pPager.borrow_mut(),
                        op,
                        resetFlag,
                        &mut nRet,
                    );
                }
            }
            *pHighwtr = 0;
            *pCurrent = nRet as i64;
        }

        // Set *pCurrent to the number of bytes that the db database connection
        // has spilled to the filesystem in temporary files that could have been
        // stored in memory, had sufficient memory been available.
        // The *pHighwater is always set to zero.
        SQLITE_DBSTATUS_TEMPBUF_SPILL => {
            let mut nRet: u64 = 0;
            if let Some(pBt) = db.aDb[1].pBt.clone() {
                let pPager = crate::btree::sqlite3BtreePager(&pBt);
                crate::pager::sqlite3PagerCacheStat(
                    &mut pPager.borrow_mut(),
                    SQLITE_DBSTATUS_CACHE_WRITE,
                    resetFlag,
                    &mut nRet,
                );
                nRet = nRet.wrapping_mul(crate::btree::sqlite3BtreeGetPageSize(&pBt) as u64);
            }
            nRet = nRet.wrapping_add(db.nSpill);
            if resetFlag != 0 {
                db.nSpill = 0;
            }
            *pHighwtr = 0;
            *pCurrent = nRet as i64;
        }

        // Set *pCurrent to non-zero if there are unresolved deferred foreign
        // key constraints.  Set *pCurrent to zero if all foreign key constraints
        // have been satisfied.  The *pHighwtr is always set to zero.
        SQLITE_DBSTATUS_DEFERRED_FKS => {
            *pHighwtr = 0;
            *pCurrent = (db.nDeferredImmCons > 0 || db.nDeferredCons > 0) as i64;
        }

        _ => {
            rc = SQLITE_ERROR;
        }
    }
    rc
}

/// C: `int sqlite3_db_status( sqlite3 *db, int op, int *pCurrent, int *pHighwtr, int resetFlag )`
pub fn sqlite3_db_status(
    db: &mut Sqlite3,
    op: i32,
    pCurrent: &mut i32,
    pHighwtr: &mut i32,
    resetFlag: i32,
) -> i32 {
    let mut C: i64 = 0;
    let mut H: i64 = 0;
    let rc = sqlite3_db_status64(db, op, &mut C, &mut H, resetFlag);
    if rc == 0 {
        *pCurrent = (C & 0x7fffffff) as i32;
        *pHighwtr = (H & 0x7fffffff) as i32;
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_counters() {
        sqlite3StatusUp(SQLITE_STATUS_MALLOC_COUNT, 5);
        sqlite3StatusDown(SQLITE_STATUS_MALLOC_COUNT, 2);
        let (mut c, mut h) = (0i32, 0i32);
        assert_eq!(
            sqlite3_status(SQLITE_STATUS_MALLOC_COUNT, &mut c, &mut h, 1),
            SQLITE_OK
        );
        assert_eq!((c, h), (3, 5));
        assert_eq!(
            sqlite3_status(SQLITE_STATUS_MALLOC_COUNT, &mut c, &mut h, 0),
            SQLITE_OK
        );
        assert_eq!((c, h), (3, 3));
        assert_eq!(sqlite3_status(10, &mut c, &mut h, 0), SQLITE_MISUSE);
    }
}
