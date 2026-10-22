//! Port of vdbeaux.c: code used for creating, destroying, and populating
//! a VDBE (or an "sqlite3_stmt" as it is known to the outside world).
//!
//! Rust-port notes (see also PORTING.md):
//! * the program builder takes `v: &mut Vdbe` (use `db.v()`); the label
//!   counter is `Parse::nLabel`, the label table is `Vdbe::aLabel`.
//! * sqlite3VdbeCreate() only builds the Vdbe; the caller
//!   (sqlite3GetVdbe) stores it in `db.pp().pVdbe`, and prepare.c registers
//!   the finished statement in `db.aVdbe`.
//! * functions that take both `db` and a Vdbe that normally lives in the
//!   current parse (sqlite3VdbeMakeReady) must be given the Vdbe taken out
//!   of `db.pp().pVdbe` (and put back afterwards).
//! * prepared statements live in `db.aVdbe` slots; functions that operate
//!   on a running statement (Halt, Reset, ...) get it taken out of its slot.

#![allow(unused_variables)]

use crate::int::*;
use crate::printf::PfArg;
use crate::vdbe::VdbeFrame;
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Small helpers (C macros of sqliteInt.h / vdbeInt.h)
// ---------------------------------------------------------------------------

/// Byte `i` of `a`, or 0 past its end (C code may read into the padding
/// that follows a record buffer; corrupt records must not panic here).
#[inline]
fn at(a: &[u8], i: usize) -> u8 {
    a.get(i).copied().unwrap_or(0)
}

/// sqlite3GetVarint(): decode a varint at `a[off..]` (bounds-safe).
/// Returns (value, number of bytes read).
fn getVarint(a: &[u8], off: usize) -> (u64, usize) {
    let mut v: u64 = 0;
    for i in 0..8 {
        let b = at(a, off + i);
        v = (v << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            return (v, i + 1);
        }
    }
    let b = at(a, off + 8);
    v = (v << 8) | b as u64;
    (v, 9)
}

/// getVarint32(): decode a 32-bit varint at `a[off..]`.  Values that do
/// not fit in 32 bits read as 0xffffffff (as sqlite3GetVarint32()).
#[inline]
fn getVarint32(a: &[u8], off: usize) -> (u32, usize) {
    let b = at(a, off);
    if b < 0x80 {
        return (b as u32, 1);
    }
    let (v, n) = getVarint(a, off);
    if v > u32::MAX as u64 {
        (0xffffffff, n)
    } else {
        (v as u32, n)
    }
}

/// sqlite3VarintLen()
#[inline]
fn varintLen(mut v: u64) -> u32 {
    let mut i = 1;
    loop {
        v >>= 7;
        if v == 0 {
            break;
        }
        i += 1;
    }
    i
}

/// sqlite3Get8byte() (bounds-safe)
#[inline]
fn get8byte(a: &[u8], off: usize) -> u64 {
    let mut x: u64 = 0;
    for i in 0..8 {
        x = (x << 8) | at(a, off + i) as u64;
    }
    x
}

/// Big-endian two's-complement integer of `n` bytes (ONE_BYTE_INT,
/// TWO_BYTE_INT, THREE_BYTE_INT, FOUR_BYTE_INT, the 6-byte and 8-byte
/// forms).
#[inline]
fn beInt(a: &[u8], off: usize, n: usize) -> i64 {
    let mut x: i64 = (at(a, off) as i8) as i64;
    for i in 1..n {
        x = (x << 8) | at(a, off + i) as i64;
    }
    x
}

/// sqlite3Strlen30() on a byte string: length up to the first NUL.
#[inline]
fn strlenNul(z: &[u8]) -> usize {
    z.iter().position(|&c| c == 0).unwrap_or(z.len())
}

/// IsNaN() on the bits of a double.
#[inline]
fn isNaN64(x: u64) -> bool {
    const EXP754: u64 = 0x7ff << 52;
    const MAN754: u64 = (1u64 << 52) - 1;
    (x & EXP754) == EXP754 && (x & MAN754) != 0
}

/// sqlite3VtabInSync(db): there is no virtual-table transaction tracking
/// yet, so this is always false.
#[inline]
fn sqlite3VtabInSync(_db: &Sqlite3) -> bool {
    false
}

/// sqlite3BtreeTxnState() on db.aDb[i].pBt (SQLITE_TXN_NONE for no btree).
fn txnState(db: &mut Sqlite3, i: usize) -> i32 {
    match db.aDb[i].pBt.clone() {
        Some(pBt) => crate::btree::sqlite3BtreeTxnState(db, &pBt),
        None => SQLITE_TXN_NONE,
    }
}

/// A new Mem with the given flags (initMemArray()).
#[inline]
fn newMem(flags: u16) -> Mem {
    Mem {
        flags,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// C: `Vdbe * sqlite3VdbeCreate(Parse *pParse)`
///
/// Rust: returns the new Vdbe; the caller (sqlite3GetVdbe) stores it in
/// `db.pp().pVdbe`.  The statement is registered in `db.aVdbe` by prepare
/// once compilation succeeds.
pub fn sqlite3VdbeCreate(db: &mut Sqlite3) -> Box<Vdbe> {
    let mut p = Box::new(Vdbe::default());
    p.iResultRow = -1;
    p.eVdbeState = VDBE_INIT_STATE;
    debug_assert!(db.ppr().nLabel == 0);
    sqlite3VdbeAddOp2(&mut p, OP_Init, 0, 1);
    p
}

/// C: `Parse * sqlite3VdbeParser(Vdbe *p)`
/// Rust: the parse of a Vdbe under construction is `db.pp()`.
pub fn sqlite3VdbeParser(p: &mut Vdbe) -> () {}

/// C: `void sqlite3VdbeError(Vdbe *p, const char *zFormat, ...)`
pub fn sqlite3VdbeError(db: &mut Sqlite3, p: &mut Vdbe, zFormat: Option<&[u8]>, args: &[PfArg]) {
    let z = crate::printf::sqlite3VMPrintf(db, zFormat.unwrap_or(b""), args);
    p.zErrMsg = Some(z);
}

/// C: `void sqlite3VdbeSetSql(Vdbe *p, const char *z, int n, u8 prepFlags)`
pub fn sqlite3VdbeSetSql(p: &mut Vdbe, z: &[u8], prepFlags: u8) {
    p.prepFlags = prepFlags;
    if (prepFlags as i32 & SQLITE_PREPARE_SAVESQL) == 0 {
        p.expmask = 0;
        p.smimask = 0;
    }
    debug_assert!(p.zSql.is_none());
    p.zSql = Some(z.to_vec());
}

/// C: `void sqlite3VdbeAddDblquoteStr(sqlite3 *db, Vdbe *p, const char *z)`
/// SQLITE_ENABLE_NORMALIZE is not compiled in: no-op.
pub fn sqlite3VdbeAddDblquoteStr(db: &mut Sqlite3, p: &mut Vdbe, z: Option<&[u8]>) {}

/// C: `int sqlite3VdbeUsesDoubleQuotedString( Vdbe *pVdbe, const char *zId )`
/// SQLITE_ENABLE_NORMALIZE is not compiled in: always 0.
pub fn sqlite3VdbeUsesDoubleQuotedString(pVdbe: &mut Vdbe, zId: Option<&[u8]>) -> i32 {
    0
}

/// C: `void sqlite3VdbeSwap(Vdbe *pA, Vdbe *pB)`
///
/// Swap byte-code between two VDBE structures.  This happens after pB was
/// previously run and returned SQLITE_SCHEMA.  The statement was then
/// reprepared in pA.  This routine transfers the new bytecode in pA over to
/// pB so that pB can be run again.  The old pB byte code is moved back to
/// pA so that it will be cleaned up when pA is finalized.  (The slot
/// positions in db.aVdbe play the role of the C pVNext/ppVPrev links and
/// are not affected.)
pub fn sqlite3VdbeSwap(db: &mut Sqlite3, pA: &mut Vdbe, pB: &mut Vdbe) {
    std::mem::swap(pA, pB);
    std::mem::swap(&mut pA.zSql, &mut pB.zSql);
    pB.expmask |= pA.expmask;
    pB.smimask |= pA.smimask;
    pB.prepFlags = pA.prepFlags;
    pB.aCounter = pA.aCounter;
    pB.aCounter[SQLITE_STMTSTATUS_REPREPARE as usize] =
        pB.aCounter[SQLITE_STMTSTATUS_REPREPARE as usize].wrapping_add(1);
}

// ---------------------------------------------------------------------------
// Adding opcodes
// ---------------------------------------------------------------------------

/// C: `int sqlite3VdbeAddOp0(Vdbe *p, int op)`
pub fn sqlite3VdbeAddOp0(p: &mut Vdbe, op: u8) -> i32 {
    sqlite3VdbeAddOp3(p, op, 0, 0, 0)
}

/// C: `int sqlite3VdbeAddOp1(Vdbe *p, int op, int p1)`
pub fn sqlite3VdbeAddOp1(p: &mut Vdbe, op: u8, p1: i32) -> i32 {
    sqlite3VdbeAddOp3(p, op, p1, 0, 0)
}

/// C: `int sqlite3VdbeAddOp2(Vdbe *p, int op, int p1, int p2)`
pub fn sqlite3VdbeAddOp2(p: &mut Vdbe, op: u8, p1: i32, p2: i32) -> i32 {
    sqlite3VdbeAddOp3(p, op, p1, p2, 0)
}

/// C: `int sqlite3VdbeAddOp3(Vdbe *p, int op, int p1, int p2, int p3)`
///
/// Add a new instruction to the list of instructions current in the VDBE.
/// Return the address of the new instruction.
pub fn sqlite3VdbeAddOp3(p: &mut Vdbe, op: u8, p1: i32, p2: i32, p3: i32) -> i32 {
    let i = p.aOp.len() as i32;
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    debug_assert!(op < 0xff);
    p.aOp.push(VdbeOp {
        opcode: op,
        p4type: P4_NOTUSED,
        p5: 0,
        p1,
        p2,
        p3,
        p4: P4::None,
        zComment: None,
    });
    i
}

/// C: `int sqlite3VdbeAddOp4Int( Vdbe *p, int op, int p1, int p2, int p3, int p4 )`
pub fn sqlite3VdbeAddOp4Int(p: &mut Vdbe, op: u8, p1: i32, p2: i32, p3: i32, p4: i32) -> i32 {
    let i = p.aOp.len() as i32;
    p.aOp.push(VdbeOp {
        opcode: op,
        p4type: P4_INT32,
        p5: 0,
        p1,
        p2,
        p3,
        p4: P4::I(p4),
        zComment: None,
    });
    i
}

/// C: `int sqlite3VdbeAddInt64(Vdbe *p, int iDest, i64 iVal)`
/// Generate an opcode that loads a 64-bit integer into register iDest.
pub fn sqlite3VdbeAddInt64(p: &mut Vdbe, iDest: i32, iVal: i64) -> i32 {
    sqlite3VdbeAddOp3(
        p,
        OP_Int64,
        iVal as u64 as u32 as i32,
        iDest,
        ((iVal as u64) >> 32) as u32 as i32,
    )
}

/// C: `int sqlite3VdbeAddDouble(Vdbe *p, int iDest, double rVal)`
/// Generate an opcode that loads a 64-bit floating point value into
/// register iDest.
pub fn sqlite3VdbeAddDouble(p: &mut Vdbe, iDest: i32, rVal: f64) -> i32 {
    let iVal = rVal.to_bits();
    sqlite3VdbeAddOp3(
        p,
        OP_Real,
        iVal as u32 as i32,
        iDest,
        (iVal >> 32) as u32 as i32,
    )
}

/// C: `int sqlite3VdbeGoto(Vdbe *p, int iDest)`
/// Generate code for an unconditional jump to instruction iDest.
pub fn sqlite3VdbeGoto(p: &mut Vdbe, iDest: i32) -> i32 {
    sqlite3VdbeAddOp3(p, OP_Goto, 0, iDest, 0)
}

/// C: `int sqlite3VdbeLoadString(Vdbe *p, int iDest, const char *zStr)`
/// Generate code to cause the string zStr to be loaded into register iDest.
pub fn sqlite3VdbeLoadString(p: &mut Vdbe, iDest: i32, zStr: &[u8]) -> i32 {
    sqlite3VdbeAddOp4(p, OP_String8, 0, iDest, 0, P4::Z(zStr.to_vec()), 0)
}

/// C: `void sqlite3VdbeMultiLoad(Vdbe *p, int iDest, const char *zTypes, ...)`
///
/// Generate code that initializes multiple registers to string or integer
/// constants.  The registers begin with iDest and increase consecutively.
/// One register is initialized for each character in zTypes[].  For each
/// "s" character in zTypes[], the register is a string if the argument is
/// not NULL (`PfArg::Str(None)`), or OP_Null if the value is a null
/// pointer.  For each "i" character in zTypes[], the register is
/// initialized to an integer.
///
/// If the input string does not end with "X" then an OP_ResultRow
/// instruction is generated for the values inserted.
pub fn sqlite3VdbeMultiLoad(p: &mut Vdbe, iDest: i32, zTypes: &[u8], args: &[PfArg]) {
    let mut ai = args.iter();
    let mut i = 0usize;
    while i < zTypes.len() && zTypes[i] != 0 {
        let c = zTypes[i];
        if c == b's' {
            let z: Option<Vec<u8>> = match ai.next() {
                Some(PfArg::Str(s)) => s.map(|x| x.to_vec()),
                Some(PfArg::Tok(t)) => Some(t.text().to_vec()),
                _ => None,
            };
            match z {
                None => {
                    sqlite3VdbeAddOp4(p, OP_Null, 0, iDest + i as i32, 0, P4::None, 0);
                }
                Some(z) => {
                    sqlite3VdbeAddOp4(p, OP_String8, 0, iDest + i as i32, 0, P4::Z(z), 0);
                }
            }
        } else if c == b'i' {
            let v = match ai.next() {
                Some(PfArg::Int(v)) => *v as i32,
                Some(PfArg::Dbl(v)) => *v as i32,
                _ => 0,
            };
            sqlite3VdbeAddOp2(p, OP_Integer, v, iDest + i as i32);
        } else {
            return; /* skip_op_resultrow */
        }
        i += 1;
    }
    sqlite3VdbeAddOp2(p, OP_ResultRow, iDest, i as i32);
}

/// C: `int sqlite3VdbeAddOp4( Vdbe *p, int op, int p1, int p2, int p3, const char *zP4, int p4type )`
/// `p4type` is the C P4_* code (P4_DYNAMIC/P4_STATIC for strings; 0 means
/// "copy the string", see sqlite3VdbeChangeP4()).
pub fn sqlite3VdbeAddOp4(
    p: &mut Vdbe,
    op: u8,
    p1: i32,
    p2: i32,
    p3: i32,
    p4: P4,
    p4type: i32,
) -> i32 {
    let addr = sqlite3VdbeAddOp3(p, op, p1, p2, p3);
    sqlite3VdbeChangeP4(p, addr, p4, p4type);
    addr
}

/// C: `int sqlite3VdbeAddFunctionCall( Parse *pParse, int p1, int p2, int p3, int nArg, const FuncDef *pFunc, int eCallCtx )`
///
/// Add an OP_Function or OP_PureFunc opcode.
///
/// The eCallCtx argument is information (typically taken from Expr.op2)
/// that describes the calling context of the function.  0 means a general
/// function call.  NC_IsCheck means called by a check constraint,
/// NC_IdxExpr means called as part of an index expression.  NC_PartIdx
/// means in the WHERE clause of a partial index.  NC_GenCol means called
/// while computing a generated column value.  0 is the usual case.
///
/// Rust: the P4 is `P4::FuncCtx(pFunc, nArg)`; the C context's iOp is the
/// address of the opcode itself.
pub fn sqlite3VdbeAddFunctionCall(
    db: &mut Sqlite3,
    p1: i32,
    p2: i32,
    p3: i32,
    nArg: i32,
    pFunc: &FuncDefRef,
    eCallCtx: i32,
) -> i32 {
    let v = db.v();
    let addr = sqlite3VdbeAddOp4(
        v,
        if eCallCtx != 0 {
            OP_PureFunc
        } else {
            OP_Function
        },
        p1,
        p2,
        p3,
        P4::FuncCtx(pFunc.clone(), nArg as u16),
        P4_FUNCCTX as i32,
    );
    sqlite3VdbeChangeP5(v, (eCallCtx & NC_SelfRef) as u16);
    crate::build::sqlite3MayAbort(db);
    addr
}

/// C: `int sqlite3VdbeExplainParent(Parse *pParse)`
///
/// Return the address of the current EXPLAIN QUERY PLAN baseline.
/// 0 means "none".
pub fn sqlite3VdbeExplainParent(db: &mut Sqlite3) -> i32 {
    let addrExplain = db.ppr().addrExplain;
    if addrExplain == 0 {
        return 0;
    }
    let v = db.v();
    match v.aOp.get(addrExplain as usize) {
        Some(op) => op.p2,
        None => 0,
    }
}

/// C: `void sqlite3ExplainBreakpoint(const char *z1, const char *z2)`
/// Debugging aid (SQLITE_DEBUG only): no-op.
pub fn sqlite3ExplainBreakpoint(z1: Option<&[u8]>, z2: Option<&[u8]>) {}

/// C: `int sqlite3VdbeExplain(Parse *pParse, u8 bPush, const char *zFmt, ...)`
///
/// Add a new OP_Explain opcode.
///
/// If the bPush flag is true, then make this opcode the parent for
/// subsequent Explains until sqlite3VdbeExplainPop() is called.
/// Returns the address of the OP_Explain (0 if none was added).
pub fn sqlite3VdbeExplain(db: &mut Sqlite3, bPush: u8, zFmt: &[u8], args: &[PfArg]) -> i32 {
    let mut addr = 0;
    /* Always include the OP_Explain opcodes if SQLITE_DEBUG is defined.
     ** But omit them (for performance) during production builds */
    if db.ppr().explain == 2 {
        let zMsg = crate::printf::sqlite3VMPrintf(db, zFmt, args);
        let addrExplain = db.ppr().addrExplain;
        let v = db.v();
        let iThis = v.aOp.len() as i32;
        addr = sqlite3VdbeAddOp4(
            v,
            OP_Explain,
            iThis,
            addrExplain,
            0,
            P4::Z(zMsg),
            P4_DYNAMIC as i32,
        );
        if bPush != 0 {
            db.pp().addrExplain = iThis;
        }
        /* sqlite3VdbeScanStatus(v, iThis, -1, -1, 0, 0): not compiled in */
    }
    addr
}

/// C: `void sqlite3VdbeExplainPop(Parse *pParse)`
/// Pop the EXPLAIN QUERY PLAN stack one level.
pub fn sqlite3VdbeExplainPop(db: &mut Sqlite3) {
    let a = sqlite3VdbeExplainParent(db);
    db.pp().addrExplain = a;
}

/// C: `void sqlite3VdbeAddParseSchemaOp(Vdbe *p, int iDb, char *zWhere, u16 p5)`
///
/// Add an OP_ParseSchema opcode.  This routine is broken out from
/// sqlite3VdbeAddOp4() since it needs to also needs to mark all btrees as
/// having been used.
///
/// zWhere is a WHERE clause that defines which entries of the schema to
/// reparse.  If zWhere==0 (None), that means all entries.  p5 is a mask of
/// INITFLAG_* values for the parse.
///
/// Rust: takes `db` (operates on `db.v()`) because it needs db->nDb and
/// sqlite3MayAbort(); zWhere is an Option (NULL for ALTER TABLE).
pub fn sqlite3VdbeAddParseSchemaOp(db: &mut Sqlite3, iDb: i32, zWhere: Option<ZStr>, p5: u16) {
    debug_assert!(p5 == 0 || zWhere.is_none());
    let nDb = db.nDb();
    let p4 = match zWhere {
        Some(z) => P4::Z(z),
        None => P4::None,
    };
    let p = db.v();
    sqlite3VdbeAddOp4(p, OP_ParseSchema, iDb, 0, 0, p4, P4_DYNAMIC as i32);
    sqlite3VdbeChangeP5(p, p5);
    for j in 0..nDb {
        sqlite3VdbeUsesBtree(p, j);
    }
    crate::build::sqlite3MayAbort(db);
}

/// C: `void sqlite3VdbeEndCoroutine(Vdbe *v, int regYield)`
///
/// Insert the end of a co-routine.
/// Rust: takes `db` (operates on `db.v()`) because it must clear the
/// temporary register cache of the parse.
pub fn sqlite3VdbeEndCoroutine(db: &mut Sqlite3, regYield: i32) {
    sqlite3VdbeAddOp1(db.v(), OP_EndCoroutine, regYield);

    /* Clear the temporary register cache, thereby ensuring that each
     ** co-routine has its own independent set of registers, because co-routines
     ** might expect their registers to be preserved across an OP_Yield, and
     ** that could cause problems if two or more co-routines are using the same
     ** temporary register.
     */
    let pParse = db.pp();
    pParse.nTempReg = 0;
    pParse.nRangeReg = 0;
}

/// C: `int sqlite3VdbeMakeLabel(Parse *pParse)`
///
/// Create a new symbolic label for an instruction that has yet to be
/// coded.  The symbolic label is really just a negative number.  The label
/// can be used as the P2 value of an operation.  Later, when the label is
/// resolved to a specific address, the VDBE will scan through its
/// operation list and change all values of P2 which match the label into
/// the resolved address.
pub fn sqlite3VdbeMakeLabel(db: &mut Sqlite3) -> i32 {
    let p = db.pp();
    p.nLabel -= 1;
    p.nLabel
}

/// C: `void sqlite3VdbeResolveLabel(Vdbe *v, int x)`
///
/// Resolve label "x" to be the address of the next instruction to be
/// inserted.  The parameter "x" must have been obtained from a prior call
/// to sqlite3VdbeMakeLabel().
///
/// Rust: the C code calls sqlite3ProgressCheck() each time the label table
/// grows past a multiple of 100 entries; that check needs the parse and is
/// not done here.
pub fn sqlite3VdbeResolveLabel(v: &mut Vdbe, x: i32) {
    let j = ADDR(x);
    debug_assert!(v.eVdbeState == VDBE_INIT_STATE);
    debug_assert!(j >= 0);
    let j = j as usize;
    if v.aLabel.len() <= j {
        v.aLabel.resize(j + 1, -1);
    }
    debug_assert!(v.aLabel[j] == -1); /* Labels may only be resolved once */
    v.aLabel[j] = v.aOp.len() as i32;
}

/// C: `void sqlite3VdbeRunOnlyOnce(Vdbe *p)`
/// Mark the VDBE as one that can only be run one time.
pub fn sqlite3VdbeRunOnlyOnce(p: &mut Vdbe) {
    sqlite3VdbeAddOp2(p, OP_Expire, 1, 1);
}

/// C: `void sqlite3VdbeReusable(Vdbe *p)`
/// Mark the VDBE as one that can be run multiple times.
pub fn sqlite3VdbeReusable(p: &mut Vdbe) {
    let mut i = 1;
    while i < p.aOp.len() {
        if p.aOp[i].opcode == OP_Expire {
            p.aOp[1].opcode = OP_Noop;
            break;
        }
        i += 1;
    }
}

/// Iterate through all opcodes of a Vdbe main program and of each of the
/// sub-programs (triggers) it may invoke directly or indirectly (C
/// VdbeOpIter / opIterNext()).  Calls `f` on each opcode until it returns
/// false.
fn opIterAll(v: &Vdbe, mut f: impl FnMut(&VdbeOp) -> bool) {
    let mut apSub: Vec<SubProgramRef> = Vec::new();
    let mut visit = |aOp: &[VdbeOp], apSub: &mut Vec<SubProgramRef>| -> bool {
        for op in aOp {
            if let P4::Program(prg) = &op.p4 {
                if op.p4type == P4_SUBPROGRAM && !apSub.iter().any(|s| Rc::ptr_eq(s, prg)) {
                    apSub.push(prg.clone());
                }
            }
            if !f(op) {
                return false;
            }
        }
        true
    };
    if !visit(&v.aOp, &mut apSub) {
        return;
    }
    let mut iSub = 0;
    while iSub < apSub.len() {
        let s = apSub[iSub].clone();
        if !visit(&s.borrow().aOp, &mut apSub) {
            return;
        }
        iSub += 1;
    }
}

/// C: `int sqlite3VdbeAssertMayAbort(Vdbe *v, int mayAbort)`
///
/// Check if the program stored in the VM associated with pParse may throw
/// an ABORT exception (causing the statement, but not entire transaction to
/// be rolled back).  Then check that the value of Parse.mayAbort is true if
/// an ABORT may be thrown, or false otherwise.  Return true if it does
/// match, or false otherwise.  (SQLITE_DEBUG only in C.)
pub fn sqlite3VdbeAssertMayAbort(v: &mut Vdbe, mayAbort: i32) -> i32 {
    let mut hasAbort = 0;
    let mut hasFkCounter = 0;
    let mut hasCreateTable = 0;
    let mut hasCreateIndex = 0;
    let mut hasInitCoroutine = 0;
    opIterAll(v, |pOp| {
        let opcode = pOp.opcode;
        if opcode == OP_Destroy
            || opcode == OP_VUpdate
            || opcode == OP_VRename
            || opcode == OP_VDestroy
            || opcode == OP_VCreate
            || opcode == OP_ParseSchema
            || opcode == OP_Function
            || opcode == OP_PureFunc
            || ((opcode == OP_Halt || opcode == OP_HaltIfNull)
                && (pOp.p1 != SQLITE_OK && pOp.p2 == OE_Abort as i32))
            || (opcode == OP_MustBeInt && pOp.p2 == 0)
        {
            hasAbort = 1;
            return false;
        }
        if opcode == OP_CreateBtree && pOp.p3 == BTREE_INTKEY {
            hasCreateTable = 1;
        }
        if mayAbort != 0 {
            if opcode == OP_CreateBtree && pOp.p3 == BTREE_BLOBKEY {
                hasCreateIndex = 1;
            }
            if opcode == OP_Clear {
                hasCreateIndex = 1;
            }
        }
        if opcode == OP_InitCoroutine {
            hasInitCoroutine = 1;
        }
        if opcode == OP_FkCounter && pOp.p1 == 0 && pOp.p2 == 1 {
            hasFkCounter = 1;
        }
        true
    });
    (hasAbort == mayAbort
        || hasFkCounter != 0
        || (hasCreateTable != 0 && hasInitCoroutine != 0)
        || hasCreateIndex != 0) as i32
}

/// C: `void sqlite3VdbeIncrWriteCounter(Vdbe *p, VdbeCursor *pC)`
/// SQLITE_DEBUG only (Vdbe.nWrite does not exist in release builds): no-op.
pub fn sqlite3VdbeIncrWriteCounter(p: &mut Vdbe, pC: &mut VdbeCursor) {}

/// C: `void sqlite3VdbeAssertAbortable(Vdbe *p)`
/// SQLITE_DEBUG only: no-op.
pub fn sqlite3VdbeAssertAbortable(p: &mut Vdbe) {}

/// resolveP2Values()
///
/// This routine is called after all opcodes have been inserted.  It loops
/// through all the opcodes and fixes up some details.
///
/// (1) For each jump instruction with a negative P2 value (a label)
///     resolve the P2 value to an actual address.
///
/// (2) Compute the maximum number of arguments used by the xUpdate/xFilter
///     methods of any virtual table and store that value in *pMaxVtabArgs.
///
/// (3) Update the Vdbe.readOnly and Vdbe.bIsReader flags to accurately
///     indicate what the prepared statement actually does.
///
/// (5) Reclaim the memory allocated for storing labels.
///
/// Rust: the caller resets Parse::nLabel.
fn resolveP2Values(p: &mut Vdbe, pMaxVtabArgs: &mut i32) {
    let mut nMaxVtabArgs = *pMaxVtabArgs;
    p.readOnly = 1;
    p.bIsReader = 0;
    debug_assert!(!p.aOp.is_empty() && p.aOp[0].opcode == OP_Init);
    let mut i = p.aOp.len();
    while i > 0 {
        i -= 1;
        let opcode = p.aOp[i].opcode;
        /* Only JUMP opcodes and the short list of special opcodes in the switch
         ** below need to be considered.  The mkopcodeh.tcl generator script groups
         ** all these opcodes together near the front of the opcode list.  Skip
         ** any opcode that does not need processing by virtual of the fact that
         ** it is larger than SQLITE_MX_JUMP_OPCODE, as a performance optimization.
         */
        if (opcode as i32) <= SQLITE_MX_JUMP_OPCODE {
            let mut doDefault = false;
            match opcode {
                OP_Transaction => {
                    if p.aOp[i].p2 != 0 {
                        p.readOnly = 0;
                    }
                    p.bIsReader = 1;
                }
                OP_AutoCommit | OP_Savepoint => {
                    p.bIsReader = 1;
                }
                OP_Checkpoint | OP_Vacuum | OP_JournalMode => {
                    p.readOnly = 0;
                    p.bIsReader = 1;
                }
                OP_Init => {
                    debug_assert!(p.aOp[i].p2 >= 0);
                    break;
                }
                OP_VUpdate => {
                    if p.aOp[i].p2 > nMaxVtabArgs {
                        nMaxVtabArgs = p.aOp[i].p2;
                    }
                }
                OP_VFilter => {
                    /* The instruction immediately prior to VFilter will be an
                     ** OP_Integer that sets the "argc" value for the VFilter.  See
                     ** the code where OP_VFilter is generated at tag-20250207a. */
                    debug_assert!(i >= 3);
                    debug_assert!(p.aOp[i - 1].opcode == OP_Integer);
                    let n = if i > 0 { p.aOp[i - 1].p1 } else { 0 };
                    if n > nMaxVtabArgs {
                        nMaxVtabArgs = n;
                    }
                    doDefault = true;
                }
                _ => {
                    doDefault = true;
                }
            }
            if doDefault {
                let p2 = p.aOp[i].p2;
                if p2 < 0 {
                    /* The mkopcodeh.tcl script has so arranged things that the only
                     ** non-jump opcodes less than SQLITE_MX_JUMP_CODE are guaranteed to
                     ** have non-negative values for P2. */
                    debug_assert!(
                        (crate::global::sqlite3OpcodeProperty[opcode as usize] & OPFLG_JUMP) != 0
                    );
                    let j = ADDR(p2) as usize;
                    debug_assert!(j < p.aLabel.len());
                    p.aOp[i].p2 = p.aLabel.get(j).copied().unwrap_or(-1);
                }
            }
        }
    }
    p.aLabel = Vec::new();
    *pMaxVtabArgs = nMaxVtabArgs;
}

/// C: `void sqlite3VdbeNoJumpsOutsideSubrtn( Vdbe *v, int iFirst, int iLast, int iRetReg )`
/// SQLITE_DEBUG only: no-op.
pub fn sqlite3VdbeNoJumpsOutsideSubrtn(v: &mut Vdbe, iFirst: i32, iLast: i32, iRetReg: i32) {}

/// C: `int sqlite3VdbeCurrentAddr(Vdbe *p)`
/// Return the address of the next instruction to be inserted.
pub fn sqlite3VdbeCurrentAddr(p: &mut Vdbe) -> i32 {
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    p.aOp.len() as i32
}

/// C: `void sqlite3VdbeVerifyNoMallocRequired(Vdbe *p, int N)`
/// SQLITE_DEBUG only: no-op.
pub fn sqlite3VdbeVerifyNoMallocRequired(p: &mut Vdbe, N: i32) {}

/// C: `void sqlite3VdbeVerifyNoResultRow(Vdbe *p)`
/// Verify that the VM does not contain an OP_ResultRow opcode.
pub fn sqlite3VdbeVerifyNoResultRow(p: &mut Vdbe) {
    for op in &p.aOp {
        debug_assert!(op.opcode != OP_ResultRow);
    }
}

/// C: `void sqlite3VdbeVerifyAbortable(Vdbe *p, int onError)`
/// SQLITE_DEBUG only (it generates OP_Abortable): no-op.
pub fn sqlite3VdbeVerifyAbortable(p: &mut Vdbe, onError: i32) {}

/// C: `VdbeOp * sqlite3VdbeTakeOpArray(Vdbe *p, int *pnOp, int *pnMaxArg)`
///
/// Returns the array of opcodes associated with the Vdbe (resolving the
/// labels first).  *pnMaxArg is set to the larger of its current value and
/// the number of entries in the Vdbe.apArg[] array required to execute the
/// returned program.  The number of opcodes is the length of the returned
/// Vec.
pub fn sqlite3VdbeTakeOpArray(p: &mut Vdbe, pnMaxArg: &mut i32) -> Vec<VdbeOp> {
    /* Check that sqlite3VdbeUsesBtree() was not called on this VM */
    debug_assert!(p.btreeMask == 0);
    resolveP2Values(p, pnMaxArg);
    std::mem::take(&mut p.aOp)
}

/// C: `VdbeOp * sqlite3VdbeAddOpList( Vdbe *p, int nOp, VdbeOpList const *aOp, int iLineno )`
///
/// Add a whole list of operations to the operation stack.  Return the
/// address of the first operation inserted.
///
/// Non-zero P2 arguments to jump instructions are automatically adjusted
/// so that the jump target is relative to the first operation inserted.
pub fn sqlite3VdbeAddOpList(p: &mut Vdbe, aOp: &[VdbeOpList], iLineno: i32) -> i32 {
    debug_assert!(!aOp.is_empty());
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    let nOp0 = p.aOp.len() as i32;
    for a in aOp {
        let mut p2 = a.p2 as i32;
        debug_assert!(a.p2 >= 0);
        if (crate::global::sqlite3OpcodeProperty[a.opcode as usize] & OPFLG_JUMP) != 0 && a.p2 > 0 {
            p2 += nOp0;
        }
        p.aOp.push(VdbeOp {
            opcode: a.opcode,
            p4type: P4_NOTUSED,
            p5: 0,
            p1: a.p1 as i32,
            p2,
            p3: a.p3 as i32,
            p4: P4::None,
            zComment: None,
        });
    }
    nOp0
}

/// C: `void sqlite3VdbeScanStatus( Vdbe *p, int addrExplain, int addrLoop, int addrVisit, LogEst nEst, const char *zName )`
/// SQLITE_ENABLE_STMT_SCANSTATUS is not compiled in: no-op.
pub fn sqlite3VdbeScanStatus(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    addrExplain: i32,
    addrLoop: i32,
    addrVisit: i32,
    nEst: LogEst,
    zName: Option<&[u8]>,
) {
}

/// C: `void sqlite3VdbeScanStatusRange( Vdbe *p, int addrExplain, int addrStart, int addrEnd )`
/// SQLITE_ENABLE_STMT_SCANSTATUS is not compiled in: no-op.
pub fn sqlite3VdbeScanStatusRange(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    addrExplain: i32,
    addrStart: i32,
    addrEnd: i32,
) {
}

/// C: `void sqlite3VdbeScanStatusCounters( Vdbe *p, int addrExplain, int addrLoop, int addrVisit )`
/// SQLITE_ENABLE_STMT_SCANSTATUS is not compiled in: no-op.
pub fn sqlite3VdbeScanStatusCounters(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    addrExplain: i32,
    addrLoop: i32,
    addrVisit: i32,
) {
}

/// C: `void sqlite3VdbeChangeOpcode(Vdbe *p, int addr, u8 iNewOpcode)`
pub fn sqlite3VdbeChangeOpcode(p: &mut Vdbe, addr: i32, iNewOpcode: u8) {
    debug_assert!(addr >= 0);
    sqlite3VdbeGetOp(p, addr).opcode = iNewOpcode;
}

/// C: `void sqlite3VdbeChangeP1(Vdbe *p, int addr, int val)`
pub fn sqlite3VdbeChangeP1(p: &mut Vdbe, addr: i32, val: i32) {
    debug_assert!(addr >= 0);
    sqlite3VdbeGetOp(p, addr).p1 = val;
}

/// C: `void sqlite3VdbeChangeP2(Vdbe *p, int addr, int val)`
pub fn sqlite3VdbeChangeP2(p: &mut Vdbe, addr: i32, val: i32) {
    debug_assert!(addr >= 0);
    sqlite3VdbeGetOp(p, addr).p2 = val;
}

/// C: `void sqlite3VdbeChangeP3(Vdbe *p, int addr, int val)`
pub fn sqlite3VdbeChangeP3(p: &mut Vdbe, addr: i32, val: i32) {
    debug_assert!(addr >= 0);
    sqlite3VdbeGetOp(p, addr).p3 = val;
}

/// C: `void sqlite3VdbeChangeP5(Vdbe *p, u16 p5)`
/// Change the P5 operand of the most recently added opcode.
pub fn sqlite3VdbeChangeP5(p: &mut Vdbe, p5: u16) {
    debug_assert!(!p.aOp.is_empty());
    if let Some(op) = p.aOp.last_mut() {
        op.p5 = p5;
    }
}

/// C: `void sqlite3VdbeTypeofColumn(Vdbe *p, int iDest)`
///
/// If the previous opcode is an OP_Column that delivers results into
/// register iDest, then add the OPFLAG_TYPEOFARG flag to that opcode.
pub fn sqlite3VdbeTypeofColumn(p: &mut Vdbe, iDest: i32) {
    if let Some(pOp) = p.aOp.last_mut() {
        if pOp.p3 == iDest && pOp.opcode == OP_Column {
            pOp.p5 |= OPFLAG_TYPEOFARG;
        }
    }
}

/// C: `void sqlite3VdbeJumpHere(Vdbe *p, int addr)`
///
/// Change the P2 operand of instruction addr so that it points to the
/// address of the next instruction to be coded.
pub fn sqlite3VdbeJumpHere(p: &mut Vdbe, addr: i32) {
    let n = p.aOp.len() as i32;
    sqlite3VdbeChangeP2(p, addr, n);
}

/// C: `void sqlite3VdbeJumpHereOrPopInst(Vdbe *p, int addr)`
///
/// Change the P2 operand of the jump instruction at addr so that the jump
/// lands on the next opcode.  Or if the jump instruction was the previous
/// opcode (and is thus a no-op) then simply back up the next instruction
/// counter by one slot so that the jump is overwritten by the next
/// inserted opcode.
pub fn sqlite3VdbeJumpHereOrPopInst(p: &mut Vdbe, addr: i32) {
    if addr == p.aOp.len() as i32 - 1 {
        debug_assert!(
            p.aOp[addr as usize].opcode == OP_Once
                || p.aOp[addr as usize].opcode == OP_If
                || p.aOp[addr as usize].opcode == OP_FkIfZero
        );
        debug_assert!(p.aOp[addr as usize].p4type == 0);
        p.aOp.pop();
    } else {
        let n = p.aOp.len() as i32;
        sqlite3VdbeChangeP2(p, addr, n);
    }
}

/// Delete a P4 value if necessary (freeP4()).
///
/// Reference-counted P4 values are released by dropping them; a
/// P4_TABLEREF also gives back its Table.nTabRef reference through
/// sqlite3DeleteTable() when `db` is available.
fn freeP4(db: Option<&mut Sqlite3>, p4type: i8, p4: P4) {
    if p4type == P4_TABLEREF {
        if let (Some(db), P4::Tab(pTab)) = (db, &p4) {
            crate::build::sqlite3DeleteTable(db, pTab);
        }
    }
    drop(p4);
}

/// Free the space allocated for aOp and any p4 values allocated for the
/// opcodes contained within (vdbeFreeOpArray()).
fn vdbeFreeOpArray(db: &mut Sqlite3, aOp: Vec<VdbeOp>) {
    for op in aOp.into_iter().rev() {
        if op.p4type <= P4_FREE_IF_LE {
            freeP4(Some(db), op.p4type, op.p4);
        }
    }
}

/// C: `void sqlite3VdbeLinkSubProgram(Vdbe *pVdbe, SubProgram *p)`
///
/// Link the SubProgram object passed as the second argument into the list
/// at Vdbe.pProgram.  This list is used to delete all sub-program objects
/// when the VM is no longer required.
pub fn sqlite3VdbeLinkSubProgram(pVdbe: &mut Vdbe, p: &SubProgramRef) {
    /* C pushes on the front of a linked list */
    pVdbe.pProgram.insert(0, p.clone());
}

/// C: `int sqlite3VdbeHasSubProgram(Vdbe *pVdbe)`
/// Return true if the given Vdbe has any SubPrograms.
pub fn sqlite3VdbeHasSubProgram(pVdbe: &mut Vdbe) -> i32 {
    (!pVdbe.pProgram.is_empty()) as i32
}

/// C: `int sqlite3VdbeChangeToNoop(Vdbe *p, int addr)`
/// Change the opcode at addr into OP_Noop.
pub fn sqlite3VdbeChangeToNoop(p: &mut Vdbe, addr: i32) -> i32 {
    debug_assert!(addr >= 0 && (addr as usize) < p.aOp.len());
    let pOp = &mut p.aOp[addr as usize];
    let p4 = std::mem::take(&mut pOp.p4);
    let p4type = pOp.p4type;
    pOp.p4type = P4_NOTUSED;
    pOp.opcode = OP_Noop;
    freeP4(None, p4type, p4);
    1
}

/// C: `int sqlite3VdbeDeletePriorOpcode(Vdbe *p, u8 op)`
///
/// If the last opcode is "op" and it is not a jump destination, then
/// remove it.  Return true if and only if an opcode was removed.
pub fn sqlite3VdbeDeletePriorOpcode(p: &mut Vdbe, op: u8) -> i32 {
    if !p.aOp.is_empty() && p.aOp[p.aOp.len() - 1].opcode == op {
        let a = p.aOp.len() as i32 - 1;
        sqlite3VdbeChangeToNoop(p, a)
    } else {
        0
    }
}

/// C: `void sqlite3VdbeReleaseRegisters( Parse *pParse, int iFirst, int N, u32 mask, int bUndefine )`
/// SQLITE_DEBUG only (generates OP_ReleaseReg): no-op in release builds.
pub fn sqlite3VdbeReleaseRegisters(
    db: &mut Sqlite3,
    iFirst: i32,
    N: i32,
    mask: u32,
    bUndefine: i32,
) {
}

/// vdbeChangeP4Full()
fn vdbeChangeP4Full(p: &mut Vdbe, addr: usize, p4: P4, n: i32) {
    {
        let pOp = &mut p.aOp[addr];
        if pOp.p4type != 0 {
            debug_assert!(pOp.p4type > P4_FREE_IF_LE);
            pOp.p4type = 0;
            pOp.p4 = P4::None;
        }
    }
    if n < 0 {
        sqlite3VdbeChangeP4(p, addr as i32, p4, n);
    } else {
        let pOp = &mut p.aOp[addr];
        pOp.p4 = match p4 {
            P4::Z(mut z) => {
                let len = if n == 0 { strlenNul(&z) } else { n as usize };
                z.truncate(len);
                P4::Z(z)
            }
            other => other,
        };
        pOp.p4type = P4_DYNAMIC;
    }
}

/// C: `void sqlite3VdbeChangeP4(Vdbe *p, int addr, const char *zP4, int n)`
///
/// Change the value of the P4 operand for a specific instruction.
///
/// If n>=0 then the P4 operand is a string that is copied (n==0: up to the
/// first NUL; n>0: n bytes) and the type is P4_DYNAMIC.
///
/// Other values of n (P4_STATIC, P4_COLLSEQ etc.) give the C P4 type of
/// the value, which is stored as is.  For strings pass `P4::Z(bytes)` and
/// n = P4_DYNAMIC (or P4_STATIC).
///
/// If addr<0 then change P4 on the most recently inserted instruction.
pub fn sqlite3VdbeChangeP4(p: &mut Vdbe, addr: i32, p4: P4, n: i32) {
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    if p.aOp.is_empty() {
        return;
    }
    debug_assert!(addr < p.aOp.len() as i32);
    let addr = if addr < 0 {
        p.aOp.len() - 1
    } else {
        addr as usize
    };
    if n >= 0 || p.aOp[addr].p4type != 0 {
        vdbeChangeP4Full(p, addr, p4, n);
        return;
    }
    let pOp = &mut p.aOp[addr];
    if n == P4_INT32 as i32 {
        pOp.p4 = match p4 {
            P4::I(i) => P4::I(i),
            P4::None => P4::I(0),
            other => other,
        };
        pOp.p4type = P4_INT32;
    } else if !matches!(p4, P4::None) {
        debug_assert!(n < 0);
        pOp.p4 = p4;
        pOp.p4type = n as i8;
    }
}

/// C: `void sqlite3VdbeAppendP4(Vdbe *p, void *pP4, int n)`
///
/// Change the P4 operand of the most recently coded instruction to the
/// value defined by the arguments.  This is a high-speed version of
/// sqlite3VdbeChangeP4().
///
/// The P4 operand must not have been previously defined.  And the new P4
/// must not be P4_INT32.  Use sqlite3VdbeChangeP4() in either of those
/// cases.
pub fn sqlite3VdbeAppendP4(p: &mut Vdbe, p4: P4, n: i32) {
    debug_assert!(n != P4_INT32 as i32 && n != P4_VTAB as i32);
    debug_assert!(n <= 0);
    debug_assert!(!p.aOp.is_empty());
    if let Some(pOp) = p.aOp.last_mut() {
        debug_assert!(pOp.p4type == P4_NOTUSED);
        pOp.p4type = n as i8;
        pOp.p4 = p4;
    }
}

/// C: `void sqlite3VdbeSetP4KeyInfo(Parse *pParse, Index *pIdx)`
///
/// Set the P4 on the most recently added opcode to the KeyInfo for the
/// index given.
pub fn sqlite3VdbeSetP4KeyInfo(db: &mut Sqlite3, pIdx: &IndexRef) {
    let pKeyInfo = crate::build::sqlite3KeyInfoOfIndex(db, pIdx);
    if let Some(k) = pKeyInfo {
        sqlite3VdbeAppendP4(db.v(), P4::KeyInfo(k), P4_KEYINFO as i32);
    }
}

/// C: `void sqlite3VdbeComment(Vdbe *p, const char *zFormat, ...)`
/// SQLITE_ENABLE_EXPLAIN_COMMENTS is not compiled in: no-op.
pub fn sqlite3VdbeComment(p: &mut Vdbe, zFormat: &[u8], args: &[PfArg]) {}

/// C: `void sqlite3VdbeNoopComment(Vdbe *p, const char *zFormat, ...)`
/// SQLITE_ENABLE_EXPLAIN_COMMENTS is not compiled in: no-op (in C the
/// VdbeNoopComment() macro expands to nothing, no OP_Noop is added).
pub fn sqlite3VdbeNoopComment(p: &mut Vdbe, zFormat: &[u8], args: &[PfArg]) {}

/// C: `void sqlite3VdbeSetLineNumber(Vdbe *v, int iLine)`
/// SQLITE_VDBE_COVERAGE is not compiled in: no-op.
pub fn sqlite3VdbeSetLineNumber(v: &mut Vdbe, iLine: i32) {}

/// C: `VdbeOp * sqlite3VdbeGetOp(Vdbe *p, int addr)`
/// Return the opcode for a given address.  The address must be
/// non-negative.
pub fn sqlite3VdbeGetOp(p: &mut Vdbe, addr: i32) -> &mut VdbeOp {
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    debug_assert!(addr >= 0 && (addr as usize) < p.aOp.len());
    &mut p.aOp[addr as usize]
}

/// C: `VdbeOp * sqlite3VdbeGetLastOp(Vdbe *p)`
/// Return the most recently added opcode.
pub fn sqlite3VdbeGetLastOp(p: &mut Vdbe) -> Option<&mut VdbeOp> {
    p.aOp.last_mut()
}

/// C: `char * sqlite3VdbeDisplayComment( sqlite3 *db, const Op *pOp, const char *zP4 )`
/// SQLITE_ENABLE_EXPLAIN_COMMENTS is not compiled in: always None (the
/// "comment" column of EXPLAIN is NULL).
pub fn sqlite3VdbeDisplayComment(
    db: &mut Sqlite3,
    pOp: &VdbeOp,
    zP4: Option<&[u8]>,
) -> Option<ZStr> {
    None
}

/// Format with sqlite3_mprintf() (no internal conversions needed).
#[inline]
fn fmt(z: &[u8], args: &[PfArg]) -> Vec<u8> {
    crate::printf::sqlite3_mprintf(z, args)
}

/// C: `char * sqlite3VdbeDisplayP4(sqlite3 *db, Op *pOp)`
///
/// Compute a string that describes the P4 parameter for an opcode.
/// Returns None (SQL NULL in the EXPLAIN output) when there is nothing to
/// display (a NULL P4 string).
pub fn sqlite3VdbeDisplayP4(pOp: &VdbeOp) -> Option<ZStr> {
    let mut x: Vec<u8> = Vec::new();
    let mut zP4: Option<Vec<u8>> = None;
    match pOp.p4type {
        P4_KEYINFO => {
            if let P4::KeyInfo(k) = &pOp.p4 {
                let pKeyInfo = k.borrow();
                x.extend(fmt(b"k(%d", &[PfArg::Int(pKeyInfo.nKeyField as i64)]));
                for j in 0..pKeyInfo.nKeyField as usize {
                    let pColl = pKeyInfo.aColl.get(j).and_then(|c| c.clone());
                    let mut zColl: &[u8] = match &pColl {
                        Some(c) => &c.zName,
                        None => b"",
                    };
                    if zColl == b"BINARY" {
                        zColl = b"B";
                    }
                    let sf = pKeyInfo.aSortFlags.get(j).copied().unwrap_or(0);
                    x.push(b',');
                    if sf & KEYINFO_ORDER_DESC != 0 {
                        x.push(b'-');
                    }
                    if sf & KEYINFO_ORDER_BIGNULL != 0 {
                        x.extend_from_slice(b"N.");
                    }
                    x.extend_from_slice(&zColl[..strlenNul(zColl)]);
                }
                x.push(b')');
            }
        }
        P4_COLLSEQ => {
            if let P4::Coll(pColl) = &pOp.p4 {
                const encnames: [&[u8]; 4] = [b"?", b"8", b"16LE", b"16BE"];
                let e = encnames[(pColl.enc as usize) & 3];
                x.extend(fmt(
                    b"%.18s-%s",
                    &[PfArg::Str(Some(&pColl.zName)), PfArg::Str(Some(e))],
                ));
            }
        }
        P4_FUNCDEF => {
            if let P4::Func(pDef) = &pOp.p4 {
                x.extend(fmt(
                    b"%s(%d)",
                    &[PfArg::Str(Some(&pDef.zName)), PfArg::Int(pDef.nArg as i64)],
                ));
            }
        }
        P4_FUNCCTX => {
            if let P4::FuncCtx(pDef, _) = &pOp.p4 {
                x.extend(fmt(
                    b"%s(%d)",
                    &[PfArg::Str(Some(&pDef.zName)), PfArg::Int(pDef.nArg as i64)],
                ));
            }
        }
        P4_INT32 => {
            let i = match &pOp.p4 {
                P4::I(i) => *i,
                _ => 0,
            };
            x.extend(fmt(b"%d", &[PfArg::Int(i as i64)]));
        }
        P4_MEM => {
            if let P4::Mem(pMem) = &pOp.p4 {
                if pMem.flags & MEM_Str != 0 {
                    zP4 = Some(pMem.bytes().to_vec());
                } else if pMem.flags & (MEM_Int | MEM_IntReal) != 0 {
                    x.extend(fmt(b"%lld", &[PfArg::Int(pMem.u.i)]));
                } else if pMem.flags & MEM_Real != 0 {
                    x.extend(fmt(b"%.16g", &[PfArg::Dbl(pMem.u.r)]));
                } else if pMem.flags & MEM_Null != 0 {
                    zP4 = Some(b"NULL".to_vec());
                } else {
                    debug_assert!(pMem.flags & MEM_Blob != 0);
                    zP4 = Some(b"(blob)".to_vec());
                }
            }
        }
        P4_VTAB => {
            /* "vtab:%p": virtual tables are not ported yet */
            x.extend_from_slice(b"vtab:0");
        }
        P4_INTARRAY => {
            if let P4::Ai(ai) = &pOp.p4 {
                /* The first element of an INTARRAY is always the count of the
                 ** number of elements to follow */
                let n = ai.first().copied().unwrap_or(0) as usize;
                let n = n.min(ai.len().saturating_sub(1));
                for i in 1..=n {
                    x.push(if i == 1 { b'[' } else { b',' });
                    x.extend(fmt(b"%u", &[PfArg::Int(ai[i] as i64)]));
                }
                x.push(b']');
            }
        }
        P4_SUBPROGRAM => {
            zP4 = Some(b"program".to_vec());
        }
        P4_TABLE => {
            if let P4::Tab(t) = &pOp.p4 {
                zP4 = Some(t.borrow().zName.clone());
            }
        }
        P4_INDEX => {
            if let P4::Idx(i) = &pOp.p4 {
                zP4 = Some(i.borrow().zName.clone());
            }
        }
        P4_SUBRTNSIG => {
            if let P4::SubrtnSig(pSig) = &pOp.p4 {
                x.extend(fmt(
                    b"subrtnsig:%d,%s",
                    &[
                        PfArg::Int(pSig.selId as i64),
                        PfArg::Str(pSig.zAff.as_deref()),
                    ],
                ));
            }
        }
        _ => {
            if let P4::Z(z) = &pOp.p4 {
                zP4 = Some(z.clone());
            }
        }
    }
    match zP4 {
        Some(z) => {
            x.extend_from_slice(&z[..strlenNul(&z)]);
            Some(x)
        }
        None => {
            /* sqlite3StrAccumFinish() returns NULL when nothing was ever
             ** appended: that only happens for a NULL P4 string (the default
             ** case); every other P4 type always appends something. */
            if x.is_empty() { None } else { Some(x) }
        }
    }
}

/// C: `void sqlite3VdbeUsesBtree(Vdbe *p, int i)`
///
/// Declare to the Vdbe that the BTree object at db->aDb[i] is used.
/// (Without shared cache no btree is sharable, so lockMask stays empty.)
pub fn sqlite3VdbeUsesBtree(p: &mut Vdbe, i: i32) {
    debug_assert!(i >= 0 && i < (std::mem::size_of::<yDbMask>() * 8) as i32);
    p.btreeMask |= (1 as yDbMask) << i;
}

/// C: `void sqlite3VdbeEnter(Vdbe *p)`
/// No shared cache: lockMask is always empty, nothing to lock.
pub fn sqlite3VdbeEnter(db: &mut Sqlite3, p: &mut Vdbe) {
    if p.lockMask == 0 {
        return; /* The common case */
    }
}

/// C: `void sqlite3VdbeLeave(Vdbe *p)`
/// No shared cache: nothing to unlock.
pub fn sqlite3VdbeLeave(p: &mut Vdbe) {
    if p.lockMask == 0 {
        return; /* The common case */
    }
}

/// C: `void sqlite3VdbePrintOp(FILE *pOut, int pc, VdbeOp *pOp)`
/// Print a single opcode to stdout (debugging only).
pub fn sqlite3VdbePrintOp(pOut: (), pc: i32, pOp: &mut VdbeOp) {
    let zP4 = sqlite3VdbeDisplayP4(pOp).unwrap_or_default();
    let line = fmt(
        b"%4d %-13s %4d %4d %4d %-13s %.2X %s\n",
        &[
            PfArg::Int(pc as i64),
            PfArg::Str(Some(
                crate::global::sqlite3OpcodeName(pOp.opcode as i32).as_bytes(),
            )),
            PfArg::Int(pOp.p1 as i64),
            PfArg::Int(pOp.p2 as i64),
            PfArg::Int(pOp.p3 as i64),
            PfArg::Str(Some(&zP4)),
            PfArg::Int(pOp.p5 as i64),
            PfArg::Str(Some(b"")),
        ],
    );
    print!("{}", String::from_utf8_lossy(&line));
}

/// Release auxiliary memory held in an array of Mem elements
/// (releaseMemArray()).
///
/// After this routine returns, all Mem elements in the array will still be
/// valid.  Those Mem elements that were not holding auxiliary resources
/// will be unchanged.  Mem elements which had something freed will be set
/// to MEM_Undefined.  (A non-empty `z` buffer plays the role of the C
/// szMalloc.)
fn releaseMemArray(a: &mut [Mem]) {
    for p in a.iter_mut() {
        if p.flags & (MEM_Agg | MEM_Dyn) != 0 {
            crate::vdbemem::sqlite3VdbeMemRelease(p);
            p.flags = MEM_Undefined;
        } else if p.z.capacity() > 0 || p.pPtr.is_some() {
            p.z = Vec::new();
            p.pPtr = None;
            p.flags = MEM_Undefined;
        }
    }
}

/// C: `int sqlite3VdbeFrameIsValid(VdbeFrame *pFrame)`
/// (SQLITE_DEBUG only in C; frames are always valid here.)
pub fn sqlite3VdbeFrameIsValid(pFrame: &mut VdbeFrame) -> i32 {
    1
}

/// C: `void sqlite3VdbeFrameMemDel(void *pArg)`
///
/// In C this is the destructor of the register holding a VdbeFrame, which
/// queues the frame on Vdbe.pDelFrame.  Frames are not stored in registers
/// in this port (see VdbeFrame), so this is a no-op.
pub fn sqlite3VdbeFrameMemDel(pArg: &mut [u8]) {}

/// C: `int sqlite3VdbeNextOpcode( Vdbe *p, Mem *pSub, int eMode, int *piPc, int *piAddr, Op **paOp )`
///
/// Locate the next opcode to be displayed in EXPLAIN or EXPLAIN QUERY PLAN
/// output.
///
/// Return SQLITE_ROW on success.  Return SQLITE_DONE if there are no more
/// opcodes to be displayed.
///
/// Rust: `apSub` (C pSub) is the list of sub-programs seen so far (None
/// when sub-programs are not listed); `*paOp` receives the sub-program
/// whose aOp holds the opcode, or None for the main program (p.aOp).
pub fn sqlite3VdbeNextOpcode(
    p: &mut Vdbe,
    mut apSub: Option<&mut Vec<SubProgramRef>>,
    eMode: i32,
    piPc: &mut i32,
    piAddr: &mut i32,
    paOp: &mut Option<SubProgramRef>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut i: i32;
    let mut aOp: Option<SubProgramRef> = None;

    /* When the number of output rows reaches nRow, that means the
     ** listing has finished and sqlite3_step() should return SQLITE_DONE.
     ** nRow is the sum of the number of rows in the main program, plus
     ** the sum of the number of rows in all trigger subprograms encountered
     ** so far.  The nRow value will increase as new trigger subprograms are
     ** encountered, but p->pc will eventually catch up to nRow.
     */
    let mut nRow = p.aOp.len() as i32;
    if let Some(subs) = apSub.as_deref() {
        for s in subs.iter() {
            nRow += s.borrow().aOp.len() as i32;
        }
    }
    let mut iPc = *piPc;
    loop {
        /* Loop exits via break */
        i = iPc;
        iPc += 1;
        if i >= nRow {
            p.rc = SQLITE_OK;
            rc = SQLITE_DONE;
            break;
        }
        let holder: SubProgramRef;
        let guard;
        let op: &VdbeOp;
        if (i as usize) < p.aOp.len() {
            /* The rowid is small enough that we are still in the
             ** main program. */
            aOp = None;
            op = &p.aOp[i as usize];
        } else {
            /* We are currently listing subprograms.  Figure out which one and
             ** pick up the appropriate opcode. */
            i -= p.aOp.len() as i32;
            let subs = apSub.as_deref().expect("sub-program list");
            let mut j = 0usize;
            while i >= subs[j].borrow().aOp.len() as i32 {
                i -= subs[j].borrow().aOp.len() as i32;
                j += 1;
            }
            holder = subs[j].clone();
            aOp = Some(holder.clone());
            guard = holder.borrow();
            op = &guard.aOp[i as usize];
        }

        /* When an OP_Program opcode is encounter (the only opcode that has
         ** a P4_SUBPROGRAM argument), expand the size of the array of subprograms
         ** kept in p->aMem[9].z to hold the new program - assuming this subprogram
         ** has not already been seen.
         */
        if let Some(subs) = apSub.as_deref_mut() {
            if op.p4type == P4_SUBPROGRAM {
                if let P4::Program(prg) = &op.p4 {
                    if !subs.iter().any(|s| Rc::ptr_eq(s, prg)) {
                        nRow += prg.borrow().aOp.len() as i32;
                        subs.push(prg.clone());
                    }
                }
            }
        }
        if eMode == 0 {
            break;
        }
        debug_assert!(eMode == 1);
        if op.opcode == OP_Explain {
            break;
        }
        if op.opcode == OP_Init && iPc > 1 {
            break;
        }
    }
    *piPc = iPc;
    *piAddr = i;
    *paOp = aOp;
    rc
}

/// C: `void sqlite3VdbeFrameDelete(VdbeFrame *p)`
///
/// Delete a VdbeFrame object and its contents: close the cursors and
/// release the registers it holds, and delete its auxdata.
pub fn sqlite3VdbeFrameDelete(db: &mut Sqlite3, p: &mut VdbeFrame) {
    let mut csrs = std::mem::take(&mut p.apChildCsr);
    csrs.append(&mut p.apCsr);
    for c in csrs.into_iter().flatten() {
        let mut c = c;
        freeCursorNN(db, &mut c);
    }
    releaseMemArray(&mut p.aChildMem);
    releaseMemArray(&mut p.aMem);
    p.aChildMem = Vec::new();
    p.aMem = Vec::new();
    sqlite3VdbeDeleteAuxData(db, &mut p.pAuxData, -1, 0);
}

/// The list of sub-programs kept in aMem[9] while listing EXPLAIN output
/// (C stores an array of SubProgram pointers in the blob of that register).
type SubList = RefCell<Vec<SubProgramRef>>;

/// C: `int sqlite3VdbeList( Vdbe *p )`
///
/// Give a listing of the program in the virtual machine.
///
/// The interface is the same as sqlite3VdbeExec().  But instead of running
/// the code, it invokes the callback once for each instruction.  This
/// feature is used to implement "EXPLAIN".
///
/// When p->explain==1, each instruction is listed.  When p->explain==2,
/// only OP_Explain instructions are listed and these are shown in a
/// different format.  p->explain==2 is used to implement EXPLAIN QUERY
/// PLAN.  2018-04-24:  In p->explain==2 mode, the OP_Init opcodes of
/// triggers are also shown, so that the boundaries between the main
/// program and each trigger are clear.
///
/// When p->explain==1, first the main program is listed, then each of the
/// trigger subprograms are listed one by one.
///
/// Rust: the result row is aMem[1..=8] (p.iResultRow = 1).
pub fn sqlite3VdbeList(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut rc;
    let bListSubprogs = p.explain == 1 || (db.flags & SQLITE_TriggerEQP) != 0;

    debug_assert!(p.explain != 0);
    debug_assert!(p.eVdbeState == VDBE_RUN_STATE);
    debug_assert!(p.rc == SQLITE_OK || p.rc == SQLITE_BUSY || p.rc == SQLITE_NOMEM);

    /* Even though this opcode does not use dynamic strings for
     ** the result, result columns may become dynamic if the user calls
     ** sqlite3_column_text16(), causing a translation to UTF-16 encoding.
     */
    if p.aMem.len() >= 9 {
        releaseMemArray(&mut p.aMem[1..9]);
    }

    if p.rc == SQLITE_NOMEM {
        /* This happens if a malloc() inside a call to sqlite3_column_text() or
         ** sqlite3_column_text16() failed.  */
        return SQLITE_ERROR;
    }

    /* The first 8 memory cells are used for the result set.  So we will
     ** commandeer the 9th cell to use as storage for an array of pointers
     ** to trigger subprograms.  The VDBE is guaranteed to have at least 9
     ** cells.  */
    let pSub: Option<Rc<SubList>> = if bListSubprogs {
        debug_assert!(p.nMem > 9);
        let m = &mut p.aMem[9];
        let existing = if m.flags & MEM_Blob != 0 {
            m.pPtr.clone().and_then(|a| a.downcast::<SubList>().ok())
        } else {
            None
        };
        match existing {
            Some(s) => Some(s),
            None => {
                let s: Rc<SubList> = Rc::new(RefCell::new(Vec::new()));
                m.pPtr = Some(s.clone());
                Some(s)
            }
        }
    } else {
        None
    };

    /* Figure out which opcode is next to display */
    let mut i = 0;
    let mut aOp: Option<SubProgramRef> = None;
    let mut pc = p.pc;
    {
        let mut guard = pSub.as_ref().map(|s| s.borrow_mut());
        rc = sqlite3VdbeNextOpcode(
            p,
            guard.as_deref_mut(),
            (p.explain == 2) as i32,
            &mut pc,
            &mut i,
            &mut aOp,
        );
    }
    p.pc = pc;
    if let Some(s) = &pSub {
        /* MemSetTypeFlag(pSub, MEM_Blob); pSub->n = nSub*sizeof(SubProgram*) */
        let nSub = s.borrow().len();
        if nSub > 0 {
            let m = &mut p.aMem[9];
            m.MemSetTypeFlag(MEM_Blob);
            m.n = (nSub * 8) as i32;
            m.z.resize(m.n as usize, 0);
        }
    }

    if rc == SQLITE_OK {
        let pOp: VdbeOp = match &aOp {
            None => p.aOp[i as usize].clone(),
            Some(s) => s.borrow().aOp[i as usize].clone(),
        };
        if db.isInterrupted.get() != 0 {
            p.rc = SQLITE_INTERRUPT;
            rc = SQLITE_ERROR;
            let z = crate::mainapi::sqlite3ErrStr(p.rc).unwrap_or_default();
            p.zErrMsg = Some(z);
        } else {
            let zP4 = sqlite3VdbeDisplayP4(&pOp);
            let aMem = &mut p.aMem;
            if p.explain == 2 {
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[1], pOp.p1 as i64);
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[2], pOp.p2 as i64);
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[3], pOp.p3 as i64);
                let n = zP4.as_ref().map(|z| z.len() as i64).unwrap_or(0);
                crate::vdbemem::sqlite3VdbeMemSetStr(
                    db,
                    &mut aMem[4],
                    zP4.as_deref(),
                    n,
                    SQLITE_UTF8,
                );
                debug_assert!(p.nResColumn == 4);
            } else {
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[1], i as i64);
                let zName = crate::global::sqlite3OpcodeName(pOp.opcode as i32).as_bytes();
                crate::vdbemem::sqlite3VdbeMemSetStr(
                    db,
                    &mut aMem[2],
                    Some(zName),
                    zName.len() as i64,
                    SQLITE_UTF8,
                );
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[3], pOp.p1 as i64);
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[4], pOp.p2 as i64);
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[5], pOp.p3 as i64);
                /* pMem+5 for p4 is done last */
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aMem[7], pOp.p5 as i64);
                crate::vdbemem::sqlite3VdbeMemSetNull(&mut aMem[8]);
                let n = zP4.as_ref().map(|z| z.len() as i64).unwrap_or(0);
                crate::vdbemem::sqlite3VdbeMemSetStr(
                    db,
                    &mut aMem[6],
                    zP4.as_deref(),
                    n,
                    SQLITE_UTF8,
                );
                debug_assert!(p.nResColumn == 8);
            }
            p.iResultRow = 1;
            if db.mallocFailed != 0 {
                p.rc = SQLITE_NOMEM;
                rc = SQLITE_ERROR;
            } else {
                p.rc = SQLITE_OK;
                rc = SQLITE_ROW;
            }
        }
    }
    rc
}

/// C: `void sqlite3VdbePrintSql(Vdbe *p)`
/// Print the SQL that was used to generate a VDBE program (debugging).
pub fn sqlite3VdbePrintSql(p: &mut Vdbe) {
    let mut z: Option<Vec<u8>> = None;
    if let Some(s) = &p.zSql {
        z = Some(s.clone());
    } else if let Some(pOp) = p.aOp.first() {
        if pOp.opcode == OP_Init {
            if let P4::Z(s) = &pOp.p4 {
                let k = s
                    .iter()
                    .position(|&c| !crate::global::sqlite3Isspace(c))
                    .unwrap_or(s.len());
                z = Some(s[k..].to_vec());
            }
        }
    }
    if let Some(z) = z {
        println!("SQL: [{}]", String::from_utf8_lossy(&z));
    }
}

/// C: `void sqlite3VdbeIOTraceSql(Vdbe *p)`
/// SQLITE_ENABLE_IOTRACE is not compiled in: no-op.
pub fn sqlite3VdbeIOTraceSql(p: &mut Vdbe) {}

/// C: `void sqlite3VdbeRewind(Vdbe *p)`
///
/// Rewind the VDBE back to the beginning in preparation for running it.
pub fn sqlite3VdbeRewind(db: &mut Sqlite3, p: &mut Vdbe) {
    debug_assert!(
        p.eVdbeState == VDBE_INIT_STATE
            || p.eVdbeState == VDBE_READY_STATE
            || p.eVdbeState == VDBE_HALT_STATE
    );

    /* There should be at least one opcode.
     */
    debug_assert!(!p.aOp.is_empty());

    p.eVdbeState = VDBE_READY_STATE;

    p.pc = -1;
    p.rc = SQLITE_OK;
    p.errorAction = OE_Abort;
    p.nChange = 0;
    p.cacheCtr = 1;
    p.minWriteFileFormat = 255;
    p.iStatement = 0;
    p.nFkConstraint = 0;
    /* Rust: the main program's OP_Once flags (C uses the OP_Init P1 counter,
     ** which effectively resets them on each run). */
    for b in p.aOnce.iter_mut() {
        *b = 0;
    }
}

/// C: `void sqlite3VdbeMakeReady( Vdbe *p, Parse *pParse )`
///
/// Prepare a virtual machine for execution for the first time after
/// creating the virtual machine.  This involves things such as allocating
/// registers and initializing the program counter.  After the VDBE has be
/// prepped, it can be executed by one or more calls to sqlite3VdbeExec().
///
/// This function may be called exactly once on each virtual machine.
/// After this routine is called the VM has been "packaged" and is ready to
/// run.  After this routine is called, further calls to sqlite3VdbeAddOp()
/// functions are prohibited.  This routine disconnects the Vdbe from the
/// Parse object that helped generate it so that the the Vdbe becomes an
/// independent entity and the Parse object can be destroyed.
///
/// Rust: the Parse is the current parse (`db.pp()`); `p` is its Vdbe,
/// taken out of `db.pp().pVdbe` by the caller for the duration of the call.
pub fn sqlite3VdbeMakeReady(db: &mut Sqlite3, p: &mut Vdbe) {
    debug_assert!(!p.aOp.is_empty());
    debug_assert!(p.eVdbeState == VDBE_INIT_STATE);
    let (nVar, mut nMem, nCursor, mut nArg, explain, usesStmtJournal) = {
        let pParse = db.pp();
        p.pVList = std::mem::take(&mut pParse.pVList);
        (
            pParse.nVar as i32,
            pParse.nMem,
            pParse.nTab,
            pParse.nMaxArg,
            pParse.explain,
            (pParse.isMultiWrite != 0 && pParse.mayAbort != 0) as u8,
        )
    };

    /* Each cursor uses a memory cell.  The first cursor (cursor 0) can
     ** use aMem[0] which is not otherwise used by the VDBE program.  Allocate
     ** space at the end of aMem[] for cursors 1 and greater.
     ** See also: allocateCursor().
     */
    nMem += nCursor;
    if nCursor == 0 && nMem > 0 {
        nMem += 1; /* Space for aMem[0] even if not used */
    }

    resolveP2Values(p, &mut nArg);
    db.pp().nLabel = 0;
    p.usesStmtJournal = usesStmtJournal;
    if explain != 0 {
        if nMem < 10 {
            nMem = 10;
        }
        p.explain = explain;
        p.nResColumn = (12 - 4 * explain as i32) as u16;
    }
    p.expired = 0;

    /* Memory for registers, parameters, cursor, etc. */
    p.nCursor = nCursor;
    p.nVar = nVar as ynVar;
    p.aVar = (0..nVar).map(|_| newMem(MEM_Null)).collect();
    p.nMem = nMem;
    p.aMem = (0..nMem).map(|_| newMem(MEM_Undefined)).collect();
    p.apCsr = (0..nCursor).map(|_| None).collect();
    p.aOnce = vec![0u8; (p.aOp.len() + 7) / 8];
    sqlite3VdbeRewind(db, p);
}

/// C: `void sqlite3VdbeFreeCursor(Vdbe *p, VdbeCursor *pCx)`
///
/// Close a VDBE cursor and release all the resources that cursor happens
/// to hold.
/// Rust: takes `db` and the cursor by value (None is the C NULL pointer):
/// `let c = p.apCsr[i].take(); sqlite3VdbeFreeCursor(db, p, c);`
pub fn sqlite3VdbeFreeCursor(db: &mut Sqlite3, p: &mut Vdbe, pCx: Option<Box<VdbeCursor>>) {
    if let Some(mut c) = pCx {
        sqlite3VdbeFreeCursorNN(db, p, &mut c);
    }
}

/// freeCursorWithCache() + sqlite3VdbeFreeCursorNN() without the Vdbe.
fn freeCursorNN(db: &mut Sqlite3, pCx: &mut VdbeCursor) {
    if pCx.colCache != 0 {
        /* freeCursorWithCache() */
        pCx.colCache = 0;
        pCx.pCache = None; /* releases pCValue */
    }
    match pCx.eCurType {
        CURTYPE_SORTER => {
            crate::vdbesort::sqlite3VdbeSorterClose(db, pCx);
        }
        CURTYPE_BTREE => {
            debug_assert!(pCx.pCursor.is_some());
            if let Some(c) = pCx.pCursor.take() {
                crate::btree::sqlite3BtreeCloseCursor(&c);
            }
        }
        CURTYPE_VTAB => { /* Virtual tables are not ported yet (C: pVtab->nRef--; xClose) */ }
        _ => {}
    }
}

/// C: `void sqlite3VdbeFreeCursorNN(Vdbe *p, VdbeCursor *pCx)`
pub fn sqlite3VdbeFreeCursorNN(db: &mut Sqlite3, p: &mut Vdbe, pCx: &mut VdbeCursor) {
    freeCursorNN(db, pCx);
}

/// Close all cursors in the current frame (closeCursorsInFrame()).
fn closeCursorsInFrame(db: &mut Sqlite3, p: &mut Vdbe) {
    for i in 0..p.apCsr.len() {
        if let Some(mut pC) = p.apCsr[i].take() {
            freeCursorNN(db, &mut pC);
        }
    }
}

/// C: `int sqlite3VdbeFrameRestore(VdbeFrame *pFrame)`
///
/// Copy the values stored in the VdbeFrame structure to its Vdbe.  This is
/// used, for example, when a trigger sub-program is halted to restore
/// control to the main program.
///
/// Rust: takes the Vdbe (C pFrame->v) explicitly; the frame must have been
/// taken off `v.pFrame` by the caller.  The child's registers and cursors
/// are left in `pFrame.aChildMem/apChildCsr` (dispose of the frame with
/// sqlite3VdbeFrameDelete()).  Returns the program counter of the parent.
pub fn sqlite3VdbeFrameRestore(db: &mut Sqlite3, v: &mut Vdbe, pFrame: &mut VdbeFrame) -> i32 {
    closeCursorsInFrame(db, v);
    v.aOp = std::mem::take(&mut pFrame.aOp);
    let parentMem = std::mem::take(&mut pFrame.aMem);
    let childMem = std::mem::replace(&mut v.aMem, parentMem);
    pFrame.aChildMem.extend(childMem);
    v.nMem = pFrame.nMem;
    let parentCsr = std::mem::take(&mut pFrame.apCsr);
    let childCsr = std::mem::replace(&mut v.apCsr, parentCsr);
    pFrame.apChildCsr.extend(childCsr);
    v.nCursor = pFrame.nCursor;
    db.lastRowid = pFrame.lastRowid;
    v.nChange = pFrame.nChange;
    db.nChange = pFrame.nDbChange;
    sqlite3VdbeDeleteAuxData(db, &mut v.pAuxData, -1, 0);
    v.pAuxData = std::mem::take(&mut pFrame.pAuxData);
    pFrame.pc
}

/// Close all cursors (closeAllCursors()).
///
/// Also release any dynamic memory held by the VM in the Vdbe.aMem memory
/// cell array, and delete every frame of a sub-program still running.
fn closeAllCursors(db: &mut Sqlite3, p: &mut Vdbe) {
    if let Some(mut pFrame) = p.pFrame.take() {
        /* Walk to the outermost frame, keeping the intermediate ones (they
         ** hold the registers and cursors of intermediate trigger levels). */
        let mut chain: Vec<Box<VdbeFrame>> = Vec::new();
        while let Some(parent) = pFrame.pParent.take() {
            chain.push(pFrame);
            pFrame = parent;
        }
        sqlite3VdbeFrameRestore(db, p, &mut pFrame);
        p.nFrame = 0;
        sqlite3VdbeFrameDelete(db, &mut pFrame);
        for mut f in chain {
            sqlite3VdbeFrameDelete(db, &mut f);
        }
    }
    debug_assert!(p.nFrame == 0);
    closeCursorsInFrame(db, p);
    releaseMemArray(&mut p.aMem);

    /* Delete any auxdata allocations made by the VM */
    if !p.pAuxData.is_empty() {
        sqlite3VdbeDeleteAuxData(db, &mut p.pAuxData, -1, 0);
    }
    debug_assert!(p.pAuxData.is_empty());
}

/// C: `void sqlite3VdbeSetNumCols(Vdbe *p, int nResColumn)`
///
/// Set the number of result columns that will be returned by this SQL
/// statement.  This is now set at compile time, rather than during
/// execution of the vdbe program so that sqlite3_column_count() can be
/// called on an SQL statement before sqlite3_step().
pub fn sqlite3VdbeSetNumCols(p: &mut Vdbe, nResColumn: i32) {
    if p.nResAlloc != 0 {
        releaseMemArray(&mut p.aColName);
    }
    let n = (nResColumn * COLNAME_N) as usize;
    p.nResColumn = nResColumn as u16;
    p.nResAlloc = nResColumn as u16;
    p.aColName = (0..n).map(|_| newMem(MEM_Null)).collect();
}

/// C: `int sqlite3VdbeSetColName( Vdbe *p, int idx, int var, const char *zName, void (*xDel)(void*) )`
///
/// Set the name of the idx'th column to be returned by the SQL statement.
/// var is COLNAME_NAME, COLNAME_DECLTYPE, ...  This call must be made
/// after a call to sqlite3VdbeSetNumCols().  The layout of aColName is the
/// C one: aColName[idx + var*nResAlloc].
pub fn sqlite3VdbeSetColName(p: &mut Vdbe, idx: i32, var: i32, zName: &[u8]) -> i32 {
    debug_assert!(idx < p.nResAlloc as i32);
    debug_assert!(var < COLNAME_N);
    let k = (idx + var * p.nResAlloc as i32) as usize;
    let pColName = &mut p.aColName[k];
    /* sqlite3VdbeMemSetText(pColName, zName, -1, xDel) */
    let n = strlenNul(zName);
    pColName.z = zName[..n].to_vec();
    pColName.n = n as i32;
    pColName.enc = SQLITE_UTF8;
    pColName.flags = MEM_Str | MEM_Term;
    SQLITE_OK
}

/// vdbeCommit()
///
/// A read or write transaction may or may not be active on database
/// handle db.  If a transaction is active, commit it.  If there is a
/// write-transaction spanning more than one database file, this routine
/// takes care of the super-journal trickery.
fn vdbeCommit(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut nTrans = 0; /* Number of databases with an active write-transaction
     ** that are candidates for a two-phase commit using a
     ** super-journal */
    let mut rc;
    let mut needXcommit = 0;

    /* Before doing anything else, call the xSync() callback for any
     ** virtual module tables written in this transaction. This has to
     ** be done before determining whether a super-journal file is
     ** required, as an xSync() callback may add an attached database
     ** to the transaction.
     */
    rc = crate::vtab::sqlite3VtabSync(db, p);

    /* This loop determines (a) if the commit hook should be invoked and
     ** (b) how many database files have open write transactions, not
     ** including the temp database. (b) is important because if more than
     ** one database file has an open write transaction, a super-journal
     ** file is required for an atomic commit.
     */
    let mut i = 0usize;
    while rc == SQLITE_OK && i < db.aDb.len() {
        if txnState(db, i) == SQLITE_TXN_WRITE {
            /* Whether or not a database might need a super-journal depends upon
             ** its journal mode (among other things).  This matrix determines which
             ** journal modes use a super-journal and which do not */
            const aMJNeeded: [u8; 6] = [
                1, /* DELETE   */
                1, /* PERSIST   */
                0, /* OFF       */
                1, /* TRUNCATE  */
                0, /* MEMORY    */
                0, /* WAL       */
            ];
            needXcommit = 1;
            let pBt = db.aDb[i].pBt.clone().unwrap();
            let pPager = crate::btree::sqlite3BtreePager(&pBt);
            let jm = crate::pager::sqlite3PagerGetJournalMode(&mut pPager.borrow_mut());
            let isMem = crate::pager::sqlite3PagerIsMemdb(&mut pPager.borrow_mut());
            if db.aDb[i].safety_level as i32 != PAGER_SYNCHRONOUS_OFF
                && aMJNeeded.get(jm as usize).copied().unwrap_or(0) != 0
                && isMem == 0
            {
                debug_assert!(i != 1);
                nTrans += 1;
            }
            rc = crate::pager::sqlite3PagerExclusiveLock(&mut pPager.borrow_mut());
        }
        i += 1;
    }
    if rc != SQLITE_OK {
        return rc;
    }

    /* If there are any write-transactions at all, invoke the commit hook */
    if needXcommit != 0 {
        if let Some(cb) = db.xCommitCallback.clone() {
            rc = cb();
            if rc != 0 {
                return SQLITE_CONSTRAINT_COMMITHOOK;
            }
        }
    }

    /* The simple case - no more than one database file (not counting the
     ** TEMP database) has a transaction active.   There is no need for the
     ** super-journal.
     **
     ** If the return value of sqlite3BtreeGetFilename() is a zero length
     ** string, it means the main database is :memory: or a temp file.  In
     ** that case we do not support atomic multi-file commits, so use the
     ** simple case then too.
     */
    let zMainFile: Vec<u8> = match db.aDb[0].pBt.clone() {
        Some(b) => crate::btree::sqlite3BtreeGetFilename(&b).unwrap_or_default(),
        None => Vec::new(),
    };
    let zMainFile = zMainFile[..strlenNul(&zMainFile)].to_vec();
    if zMainFile.is_empty() || nTrans <= 1 {
        if needXcommit != 0 {
            let mut i = 0usize;
            while rc == SQLITE_OK && i < db.aDb.len() {
                if txnState(db, i) >= SQLITE_TXN_WRITE {
                    let pBt = db.aDb[i].pBt.clone().unwrap();
                    rc = crate::btree::sqlite3BtreeCommitPhaseOne(&pBt, None);
                }
                i += 1;
            }
        }

        /* Do the commit only if all databases successfully complete phase 1.
         ** If one of the BtreeCommitPhaseOne() calls fails, this indicates an
         ** IO error while deleting or truncating a journal file. It is unlikely,
         ** but could happen. In this case abandon processing and return the error.
         */
        let mut i = 0usize;
        while rc == SQLITE_OK && i < db.aDb.len() {
            let txn = txnState(db, i);
            if txn != SQLITE_TXN_NONE {
                debug_assert!(needXcommit != 0 || txn == SQLITE_TXN_READ);
                let pBt = db.aDb[i].pBt.clone().unwrap();
                rc = crate::btree::sqlite3BtreeCommitPhaseTwo(&pBt, 0);
            }
            i += 1;
        }
        if rc == SQLITE_OK {
            crate::vtab::sqlite3VtabCommit(db);
        }
    }
    /* The complex case - There is a multi-file write-transaction active.
     ** This requires a super-journal file to ensure the transaction is
     ** committed atomically.
     */
    else {
        let pVfs = db.pVfs.clone();
        let mut zSuper: Vec<u8>;
        let mut pSuperJrnl: Option<Box<crate::os::SqliteFile>> = None;
        let mut offset: i64 = 0;
        let mut res: i32 = 0;
        let mut retryCount = 0;

        /* Select a super-journal file name */
        loop {
            zSuper = zMainFile.clone();
            if retryCount != 0 {
                if retryCount > 100 {
                    crate::printf::sqlite3_log(
                        SQLITE_FULL,
                        b"MJ delete: %s",
                        &[PfArg::Str(Some(&zSuper))],
                    );
                    crate::os::sqlite3OsDelete(&pVfs, &zSuper, 0);
                    break;
                } else if retryCount == 1 {
                    crate::printf::sqlite3_log(
                        SQLITE_FULL,
                        b"MJ collide: %s",
                        &[PfArg::Str(Some(&zSuper))],
                    );
                }
            }
            retryCount += 1;
            let mut rnd = [0u8; 4];
            crate::random::sqlite3_randomness(4, &mut rnd);
            let iRandom = u32::from_ne_bytes(rnd);
            zSuper.extend(crate::printf::sqlite3_snprintf(
                13,
                b"-mj%06X9%02X",
                &[
                    PfArg::Int(((iRandom >> 8) & 0xffffff) as i64),
                    PfArg::Int((iRandom & 0xff) as i64),
                ],
            ));
            /* The antipenultimate character of the super-journal name must
             ** be "9" to avoid name collisions when using 8+3 filenames. */
            debug_assert!(zSuper[zSuper.len() - 3] == b'9');
            /* sqlite3FileSuffix3(zMainFile, zSuper): 8+3 names not enabled */
            rc = crate::os::sqlite3OsAccess(
                &pVfs,
                &zSuper,
                SQLITE_ACCESS_EXISTS,
                &mut res,
            );
            if !(rc == SQLITE_OK && res != 0) {
                break;
            }
        }
        if rc == SQLITE_OK {
            /* Open the super-journal. */
            let mut outFlags = 0;
            rc = crate::os::sqlite3OsOpenMalloc(
                &pVfs,
                Some(&zSuper),
                &mut pSuperJrnl,
                SQLITE_OPEN_READWRITE
                    | SQLITE_OPEN_CREATE
                    | SQLITE_OPEN_EXCLUSIVE
                    | SQLITE_OPEN_SUPER_JOURNAL,
                Some(&mut outFlags),
            );
        }
        if rc != SQLITE_OK {
            return rc;
        }
        let mut pSuperJrnl = pSuperJrnl.expect("super-journal file");

        /* Write the name of each database file in the transaction into the new
         ** super-journal file. If an error occurs at this point close
         ** and delete the super-journal file. All the individual journal files
         ** still have 'null' as the super-journal pointer, so they will roll
         ** back independently if a failure occurs.
         */
        for i in 0..db.aDb.len() {
            if txnState(db, i) == SQLITE_TXN_WRITE {
                let pBt = db.aDb[i].pBt.clone().unwrap();
                let Some(zFile) = crate::btree::sqlite3BtreeGetJournalname(&pBt) else {
                    continue; /* Ignore TEMP and :memory: databases */
                };
                let mut zFile = zFile[..strlenNul(&zFile)].to_vec();
                debug_assert!(!zFile.is_empty());
                zFile.push(0);
                rc = crate::os::sqlite3OsWrite(&mut pSuperJrnl, &zFile, zFile.len() as i32, offset);
                offset += zFile.len() as i64;
                if rc != SQLITE_OK {
                    crate::os::sqlite3OsCloseFree(&mut pSuperJrnl);
                    crate::os::sqlite3OsDelete(&pVfs, &zSuper, 0);
                    return rc;
                }
            }
        }

        /* Sync the super-journal file. If the IOCAP_SEQUENTIAL device
         ** flag is set this is not required.
         */
        if 0 == (crate::os::sqlite3OsDeviceCharacteristics(&mut pSuperJrnl)
            & SQLITE_IOCAP_SEQUENTIAL)
        {
            rc = crate::os::sqlite3OsSync(&mut pSuperJrnl, SQLITE_SYNC_NORMAL);
            if rc != SQLITE_OK {
                crate::os::sqlite3OsCloseFree(&mut pSuperJrnl);
                crate::os::sqlite3OsDelete(&pVfs, &zSuper, 0);
                return rc;
            }
        }

        /* Sync all the db files involved in the transaction. The same call
         ** sets the super-journal pointer in each individual journal. If
         ** an error occurs here, do not delete the super-journal file.
         **
         ** If the error occurs during the first call to
         ** sqlite3BtreeCommitPhaseOne(), then there is a chance that the
         ** super-journal file will be orphaned. But we cannot delete it,
         ** in case the super-journal file name was written into the journal
         ** file before the failure occurred.
         */
        let mut i = 0usize;
        while rc == SQLITE_OK && i < db.aDb.len() {
            if let Some(pBt) = db.aDb[i].pBt.clone() {
                rc = crate::btree::sqlite3BtreeCommitPhaseOne(&pBt, Some(&zSuper));
            }
            i += 1;
        }
        crate::os::sqlite3OsCloseFree(&mut pSuperJrnl);
        debug_assert!(rc != SQLITE_BUSY);
        if rc != SQLITE_OK {
            return rc;
        }

        /* Delete the super-journal file. This commits the transaction. After
         ** doing this the directory is synced again before any individual
         ** transaction files are deleted.
         */
        rc = crate::os::sqlite3OsDelete(&pVfs, &zSuper, 1);
        if rc != 0 {
            return rc;
        }

        /* All files and directories have already been synced, so the following
         ** calls to sqlite3BtreeCommitPhaseTwo() are only closing files and
         ** deleting or truncating journals. If something goes wrong while
         ** this is happening we don't really care. The integrity of the
         ** transaction is already guaranteed, but some stray 'cold' journals
         ** may be lying around. Returning an error code won't help matters.
         */
        for i in 0..db.aDb.len() {
            if let Some(pBt) = db.aDb[i].pBt.clone() {
                crate::btree::sqlite3BtreeCommitPhaseTwo(&pBt, 1);
            }
        }

        crate::vtab::sqlite3VtabCommit(db);
    }

    rc
}

/// vdbeCloseStatement()
///
/// If the Vdbe passed as the first argument opened a statement-transaction,
/// close it now.  Argument eOp must be either SAVEPOINT_ROLLBACK or
/// SAVEPOINT_RELEASE.  If it is SAVEPOINT_ROLLBACK, then the statement
/// transaction is rolled back.  If eOp is SAVEPOINT_RELEASE, then the
/// statement transaction is committed.
///
/// If an IO error occurs, an SQLITE_IOERR_XXX error code is returned.
/// Otherwise SQLITE_OK.
fn vdbeCloseStatement(db: &mut Sqlite3, p: &mut Vdbe, eOp: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let iSavepoint = p.iStatement - 1;

    debug_assert!(eOp == SAVEPOINT_ROLLBACK || eOp == SAVEPOINT_RELEASE);
    debug_assert!(db.nStatement > 0);
    debug_assert!(p.iStatement == (db.nStatement + db.nSavepoint));

    for i in 0..db.aDb.len() {
        let mut rc2 = SQLITE_OK;
        if let Some(pBt) = db.aDb[i].pBt.clone() {
            if eOp == SAVEPOINT_ROLLBACK {
                rc2 = crate::btree::sqlite3BtreeSavepoint(&pBt, SAVEPOINT_ROLLBACK, iSavepoint);
            }
            if rc2 == SQLITE_OK {
                rc2 = crate::btree::sqlite3BtreeSavepoint(&pBt, SAVEPOINT_RELEASE, iSavepoint);
            }
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
    }
    db.nStatement -= 1;
    p.iStatement = 0;

    if rc == SQLITE_OK {
        if eOp == SAVEPOINT_ROLLBACK {
            rc = crate::vtab::sqlite3VtabSavepoint(db, SAVEPOINT_ROLLBACK, iSavepoint);
        }
        if rc == SQLITE_OK {
            rc = crate::vtab::sqlite3VtabSavepoint(db, SAVEPOINT_RELEASE, iSavepoint);
        }
    }

    /* If the statement transaction is being rolled back, also restore the
     ** database handles deferred constraint counter to the value it had when
     ** the statement transaction was opened.  */
    if eOp == SAVEPOINT_ROLLBACK {
        db.nDeferredCons = p.nStmtDefCons;
        db.nDeferredImmCons = p.nStmtDefImmCons;
    }
    rc
}

/// C: `int sqlite3VdbeCloseStatement(Vdbe *p, int eOp)`
pub fn sqlite3VdbeCloseStatement(db: &mut Sqlite3, p: &mut Vdbe, eOp: i32) -> i32 {
    if db.nStatement != 0 && p.iStatement != 0 {
        return vdbeCloseStatement(db, p, eOp);
    }
    SQLITE_OK
}

/// vdbeFkError()
fn vdbeFkError(p: &mut Vdbe) -> i32 {
    p.rc = SQLITE_CONSTRAINT_FOREIGNKEY;
    p.errorAction = OE_Abort;
    p.zErrMsg = Some(b"FOREIGN KEY constraint failed".to_vec());
    if (p.prepFlags as i32 & SQLITE_PREPARE_SAVESQL) == 0 {
        return SQLITE_ERROR;
    }
    SQLITE_CONSTRAINT_FOREIGNKEY
}

/// C: `int sqlite3VdbeCheckFkImmediate(Vdbe *p)`
///
/// Called when a transaction opened by the database handle associated
/// with the VM is about to be committed.  If there are outstanding foreign
/// key constraint violations return an error code.  Otherwise, SQLITE_OK.
pub fn sqlite3VdbeCheckFkImmediate(p: &mut Vdbe) -> i32 {
    if p.nFkConstraint == 0 {
        return SQLITE_OK;
    }
    vdbeFkError(p)
}

/// C: `int sqlite3VdbeCheckFkDeferred(Vdbe *p)`
pub fn sqlite3VdbeCheckFkDeferred(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    if (db.nDeferredCons + db.nDeferredImmCons) == 0 {
        return SQLITE_OK;
    }
    vdbeFkError(p)
}

/// C: `int sqlite3VdbeHalt(Vdbe *p)`
///
/// This routine is called the when a VDBE tries to halt.  If the VDBE has
/// made changes and is in autocommit mode, then commit those changes.  If
/// a rollback is needed, then do the rollback.
///
/// This routine is the only way to move the state of a VM from
/// SQLITE_STATE_RUN to SQLITE_STATE_HALT.  It is harmless to call this on
/// a VM that is in the SQLITE_STATE_HALT state.
///
/// Return an error code.  If the commit could not complete because of
/// lock contention, return SQLITE_BUSY.  If SQLITE_BUSY is returned, it
/// means the close did not happen and needs to be repeated.
pub fn sqlite3VdbeHalt(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut rc: i32;

    /* This function contains the logic that determines if a statement or
     ** transaction will be committed or rolled back as a result of the
     ** execution of this virtual machine.
     **
     ** If any of the following errors occur:
     **
     **     SQLITE_NOMEM
     **     SQLITE_IOERR
     **     SQLITE_FULL
     **     SQLITE_INTERRUPT
     **
     ** Then the internal cache might have been left in an inconsistent
     ** state.  We need to rollback the statement transaction, if there is
     ** one, or the complete transaction if there is no statement transaction.
     */

    debug_assert!(p.eVdbeState == VDBE_RUN_STATE);
    if db.mallocFailed != 0 {
        p.rc = SQLITE_NOMEM;
    }
    closeAllCursors(db, p);

    /* No commit or rollback needed if the program never started or if the
     ** SQL statement does not read or write a database file.  */
    if p.bIsReader != 0 {
        let mrc: i32; /* Primary error code from p->rc */
        let mut eStatementOp = 0;
        let isSpecialError: bool; /* Set to true if a 'special' error */

        /* Lock all btrees used by the statement */
        sqlite3VdbeEnter(db, p);

        /* Check for one of the special errors */
        if p.rc != 0 {
            mrc = p.rc & 0xff;
            isSpecialError = mrc == SQLITE_NOMEM
                || mrc == SQLITE_IOERR
                || mrc == SQLITE_INTERRUPT
                || mrc == SQLITE_FULL;
        } else {
            mrc = 0;
            isSpecialError = false;
        }
        if isSpecialError {
            /* If the query was read-only and the error code is SQLITE_INTERRUPT,
             ** no rollback is necessary. Otherwise, at least a savepoint
             ** transaction must be rolled back to restore the database to a
             ** consistent state.
             **
             ** Even if the statement is read-only, it is important to perform
             ** a statement or transaction rollback operation. If the error
             ** occurred while writing to the journal, sub-journal or database
             ** file as part of an effort to free up cache space (see function
             ** pagerStress() in pager.c), the rollback is required to restore
             ** the pager to a consistent state.
             */
            if p.readOnly == 0 || mrc != SQLITE_INTERRUPT {
                if (mrc == SQLITE_NOMEM || mrc == SQLITE_FULL) && p.usesStmtJournal != 0 {
                    eStatementOp = SAVEPOINT_ROLLBACK;
                } else {
                    /* We are forced to roll back the active transaction. Before doing
                     ** so, abort any other statements this handle currently has active.
                     */
                    crate::mainapi::sqlite3RollbackAll(db, SQLITE_ABORT_ROLLBACK);
                    crate::mainapi::sqlite3CloseSavepoints(db);
                    db.autoCommit = 1;
                    p.nChange = 0;
                }
            }
        }

        /* Check for immediate foreign key violations. */
        if p.rc == SQLITE_OK || (p.errorAction == OE_Fail && !isSpecialError) {
            let _ = sqlite3VdbeCheckFkImmediate(p);
        }

        /* If the auto-commit flag is set and this is the only active writer
         ** VM, then we do either a commit or rollback of the current transaction.
         **
         ** Note: This block also runs if one of the special errors handled
         ** above has occurred.
         */
        if !sqlite3VtabInSync(db) && db.autoCommit != 0 && db.nVdbeWrite == (p.readOnly == 0) as i32
        {
            if p.rc == SQLITE_OK || (p.errorAction == OE_Fail && !isSpecialError) {
                rc = sqlite3VdbeCheckFkDeferred(db, p);
                if rc != SQLITE_OK {
                    if p.readOnly != 0 {
                        sqlite3VdbeLeave(p);
                        return SQLITE_ERROR;
                    }
                    rc = SQLITE_CONSTRAINT_FOREIGNKEY;
                } else if db.flags & SQLITE_CorruptRdOnly != 0 {
                    rc = SQLITE_CORRUPT;
                    db.flags &= !SQLITE_CorruptRdOnly;
                } else {
                    /* The auto-commit flag is true, the vdbe program was successful
                     ** or hit an 'OR FAIL' constraint and there are no deferred foreign
                     ** key constraints to hold up the transaction. This means a commit
                     ** is required. */
                    rc = vdbeCommit(db, p);
                }
                if rc == SQLITE_BUSY && p.readOnly != 0 {
                    sqlite3VdbeLeave(p);
                    return SQLITE_BUSY;
                } else if rc != SQLITE_OK {
                    crate::util::sqlite3SystemError(db, rc);
                    p.rc = rc;
                    crate::mainapi::sqlite3RollbackAll(db, SQLITE_OK);
                    p.nChange = 0;
                } else {
                    db.nDeferredCons = 0;
                    db.nDeferredImmCons = 0;
                    db.flags &= !SQLITE_DeferFKs;
                    crate::build::sqlite3CommitInternalChanges(db);
                }
            } else if p.rc == SQLITE_SCHEMA && db.nVdbeActive > 1 {
                p.nChange = 0;
            } else {
                crate::mainapi::sqlite3RollbackAll(db, SQLITE_OK);
                p.nChange = 0;
            }
            db.nStatement = 0;
        } else if eStatementOp == 0 {
            if p.rc == SQLITE_OK || p.errorAction == OE_Fail {
                eStatementOp = SAVEPOINT_RELEASE;
            } else if p.errorAction == OE_Abort {
                eStatementOp = SAVEPOINT_ROLLBACK;
            } else {
                crate::mainapi::sqlite3RollbackAll(db, SQLITE_ABORT_ROLLBACK);
                crate::mainapi::sqlite3CloseSavepoints(db);
                db.autoCommit = 1;
                p.nChange = 0;
            }
        }

        /* If eStatementOp is non-zero, then a statement transaction needs to
         ** be committed or rolled back. Call sqlite3VdbeCloseStatement() to
         ** do so. If this operation returns an error, and the current statement
         ** error code is SQLITE_OK or SQLITE_CONSTRAINT, then promote the
         ** current statement error code.
         */
        if eStatementOp != 0 {
            rc = sqlite3VdbeCloseStatement(db, p, eStatementOp);
            if rc != 0 {
                if p.rc == SQLITE_OK || (p.rc & 0xff) == SQLITE_CONSTRAINT {
                    p.rc = rc;
                    p.zErrMsg = None;
                }
                crate::mainapi::sqlite3RollbackAll(db, SQLITE_ABORT_ROLLBACK);
                crate::mainapi::sqlite3CloseSavepoints(db);
                db.autoCommit = 1;
                p.nChange = 0;
            }
        }

        /* If this was an INSERT, UPDATE or DELETE and no statement transaction
         ** has been rolled back, update the database connection change-counter.
         */
        if p.changeCntOn != 0 {
            if eStatementOp != SAVEPOINT_ROLLBACK {
                sqlite3VdbeSetChanges(db, p.nChange);
            } else {
                sqlite3VdbeSetChanges(db, 0);
            }
            p.nChange = 0;
        }

        /* Release the locks */
        sqlite3VdbeLeave(p);
    }

    /* We have successfully halted and closed the VM.  Record this fact. */
    db.nVdbeActive -= 1;
    if p.readOnly == 0 {
        db.nVdbeWrite -= 1;
    }
    if p.bIsReader != 0 {
        db.nVdbeRead -= 1;
    }
    debug_assert!(db.nVdbeActive >= db.nVdbeRead);
    debug_assert!(db.nVdbeRead >= db.nVdbeWrite);
    debug_assert!(db.nVdbeWrite >= 0);
    p.eVdbeState = VDBE_HALT_STATE;
    if db.mallocFailed != 0 {
        p.rc = SQLITE_NOMEM;
    }

    /* If the auto-commit flag is set to true, then any locks that were held
     ** by connection db have now been released. Call sqlite3ConnectionUnlocked()
     ** to invoke any required unlock-notify callbacks.
     ** (SQLITE_ENABLE_UNLOCK_NOTIFY is not compiled in.)
     */

    debug_assert!(db.nVdbeActive > 0 || db.autoCommit == 0 || db.nStatement == 0);
    if p.rc == SQLITE_BUSY {
        SQLITE_BUSY
    } else {
        SQLITE_OK
    }
}

/// C: `void sqlite3VdbeResetStepResult(Vdbe *p)`
///
/// Each VDBE holds the result of the most recent sqlite3_step() call in
/// p->rc.  This routine sets that result back to SQLITE_OK.
pub fn sqlite3VdbeResetStepResult(p: &mut Vdbe) {
    p.rc = SQLITE_OK;
}

/// C: `int sqlite3VdbeTransferError(Vdbe *p)`
///
/// Copy the error code and error message belonging to the VDBE passed as
/// the first argument to its database handle (so that they will be
/// returned by calls to sqlite3_errcode() and sqlite3_errmsg()).
///
/// This function does not clear the VDBE error code or message, just
/// copies them to the database handle.
pub fn sqlite3VdbeTransferError(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let rc = p.rc;
    if let Some(zErrMsg) = &p.zErrMsg {
        db.bBenignMalloc = db.bBenignMalloc.wrapping_add(1);
        if db.pErr.is_none() {
            db.pErr = Some(Box::new(Mem::default()));
        }
        let pErr = db.pErr.as_mut().unwrap();
        let n = strlenNul(zErrMsg);
        crate::vdbemem::sqlite3ValueSetStr(pErr, n as i32, &zErrMsg[..n], SQLITE_UTF8);
        db.bBenignMalloc = db.bBenignMalloc.wrapping_sub(1);
    } else if let Some(pErr) = db.pErr.as_mut() {
        crate::vdbemem::sqlite3ValueSetNull(pErr);
    }
    db.errCode = rc;
    db.errByteOffset = -1;
    rc
}

/// C: `int sqlite3VdbeReset(Vdbe *p)`
///
/// Clean up a VDBE after execution but do not delete the VDBE just yet.
/// Write any error messages into *pzErrMsg.  Return the result code.
///
/// After this routine is run, the VDBE should be ready to be executed
/// again.
///
/// To look at it another way, this routine resets the state of the
/// virtual machine from VDBE_RUN_STATE or VDBE_HALT_STATE back to
/// VDBE_READY_STATE.
pub fn sqlite3VdbeReset(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    /* If the VM did not run to completion or if it encountered an
     ** error, then it might not have been halted properly.  So halt
     ** it now.
     */
    if p.eVdbeState == VDBE_RUN_STATE {
        sqlite3VdbeHalt(db, p);
    }

    /* If the VDBE has been run even partially, then transfer the error code
     ** and error message from the VDBE into the main database structure.  But
     ** if the VDBE has just been set to run but has not actually executed any
     ** instructions yet, leave the main database error information unchanged.
     */
    if p.pc >= 0 {
        if db.pErr.is_some() || p.zErrMsg.is_some() {
            sqlite3VdbeTransferError(db, p);
        } else {
            db.errCode = p.rc;
        }
    }

    /* Reset register contents and reclaim error message memory.
     */
    p.zErrMsg = None;
    p.iResultRow = -1;
    p.rc & db.errMask
}

/// C: `int sqlite3VdbeFinalize(Vdbe *p)`
///
/// Clean up and delete a VDBE after execution.  Return an integer which is
/// the result code.
/// Rust: the caller removes the statement from its db.aVdbe slot
/// (stmt_remove) and drops it afterwards.
pub fn sqlite3VdbeFinalize(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut rc = SQLITE_OK;
    if p.eVdbeState >= VDBE_READY_STATE {
        rc = sqlite3VdbeReset(db, p);
        debug_assert!((rc & db.errMask) == rc);
    }
    sqlite3VdbeDelete(db, p);
    rc
}

/// C: `void sqlite3VdbeDeleteAuxData(sqlite3 *db, AuxData **pp, int iOp, int mask)`
///
/// If parameter iOp is less than zero, then invoke the destructor for all
/// auxiliary data pointers currently cached by the VM passed as the first
/// argument.
///
/// Or, if iOp is greater than or equal to zero, then the destructor is
/// only invoked for those auxiliary data pointers created by the user
/// function invoked by the OP_Function opcode at instruction iOp of VM
/// pVdbe, and only then if:
///
///    * the associated function parameter is the 32nd or later (counting
///      from left to right), or
///
///    * the corresponding bit in argument mask is clear (where the first
///      function parameter corresponds to bit 0 etc.).
///
/// Rust: the list is a Vec; the destructor runs when the Rc is dropped.
pub fn sqlite3VdbeDeleteAuxData(db: &mut Sqlite3, pp: &mut Vec<AuxData>, iOp: i32, mask: i32) {
    let mask = mask as u32;
    pp.retain(|pAux| {
        let del = iOp < 0
            || (pAux.iAuxOp == iOp
                && pAux.iAuxArg >= 0
                && (pAux.iAuxArg > 31 || (mask & MASKBIT32(pAux.iAuxArg)) == 0));
        !del
    });
}

/// sqlite3VdbeClearObject()
///
/// Free all memory associated with the Vdbe passed as the second argument,
/// except for object itself, which is preserved.
fn sqlite3VdbeClearObject(db: &mut Sqlite3, p: &mut Vdbe) {
    if !p.aColName.is_empty() {
        releaseMemArray(&mut p.aColName);
        p.aColName = Vec::new();
    }
    for pSub in std::mem::take(&mut p.pProgram) {
        /* Rust: the op arrays of sub-programs are released when the last
         ** reference to the SubProgram goes away; give back TABLEREF
         ** references when this was the last one. */
        if let Ok(sub) = Rc::try_unwrap(pSub) {
            vdbeFreeOpArray(db, sub.into_inner().aOp);
        }
    }
    if p.eVdbeState != VDBE_INIT_STATE {
        releaseMemArray(&mut p.aVar);
        p.pVList = VList::default();
    }
    let aOp = std::mem::take(&mut p.aOp);
    vdbeFreeOpArray(db, aOp);
    p.zSql = None;
    p.aScan = Vec::new();
}

/// C: `void sqlite3VdbeDelete(Vdbe *p)`
///
/// Delete an entire VDBE.
/// Rust: releases everything the Vdbe holds; unlinking it from db.aVdbe
/// (stmt_remove) and dropping the Box are the caller's job.
pub fn sqlite3VdbeDelete(db: &mut Sqlite3, p: &mut Vdbe) {
    sqlite3VdbeClearObject(db, p);
}

/// C: `int SQLITE_NOINLINE sqlite3VdbeFinishMoveto(VdbeCursor *p)`
///
/// The cursor "p" has a pending seek operation that has not yet been
/// carried out.  Seek the cursor now.  If an error occurs, return the
/// appropriate error code.
/// Rust: takes `db` (needed by sqlite3BtreeTableMoveto()).
pub fn sqlite3VdbeFinishMoveto(db: &mut Sqlite3, p: &mut VdbeCursor) -> i32 {
    let mut res = 0;
    debug_assert!(p.deferredMoveto != 0);
    debug_assert!(p.isTable != 0);
    debug_assert!(p.eCurType == CURTYPE_BTREE);
    let pCur = p.pCursor.clone().expect("btree cursor");
    let rc = crate::btree::sqlite3BtreeTableMoveto(db, &pCur, p.movetoTarget, 0, &mut res);
    if rc != 0 {
        return rc;
    }
    if res != 0 {
        return SQLITE_CORRUPT;
    }
    p.deferredMoveto = 0;
    p.cacheStatus = CACHE_STALE as u32;
    SQLITE_OK
}

/// C: `int SQLITE_NOINLINE sqlite3VdbeHandleMovedCursor(VdbeCursor *p)`
///
/// Something has moved cursor "p" out of place.  Maybe the row it was
/// pointed to was deleted out from under it.  Or maybe the btree was
/// rebalanced.  Whatever the cause, try to restore "p" to the place it is
/// supposed to be pointing.  If the row was deleted out from under the
/// cursor, set the cursor to point to a NULL row.
pub fn sqlite3VdbeHandleMovedCursor(p: &mut VdbeCursor) -> i32 {
    let mut isDifferentRow = 0;
    debug_assert!(p.eCurType == CURTYPE_BTREE);
    let pCur = p.pCursor.clone().expect("btree cursor");
    let rc = crate::btree::sqlite3BtreeCursorRestore(&pCur, &mut isDifferentRow);
    p.cacheStatus = CACHE_STALE as u32;
    if isDifferentRow != 0 {
        p.nullRow = 1;
    }
    rc
}

/// C: `int sqlite3VdbeCursorRestore(VdbeCursor *p)`
///
/// Check to ensure that the cursor is valid.  Restore the cursor if need
/// be.  Return any I/O error from the restore operation.
pub fn sqlite3VdbeCursorRestore(p: &mut VdbeCursor) -> i32 {
    let Some(pCur) = p.pCursor.clone() else {
        return SQLITE_OK; /* IsNullCursor() */
    };
    if crate::btree::sqlite3BtreeCursorHasMoved(&pCur) != 0 {
        return sqlite3VdbeHandleMovedCursor(p);
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Serial types
//
//   serial type        bytes of data      type
//   --------------     ---------------    ---------------
//      0                     0            NULL
//      1                     1            signed integer
//      2                     2            signed integer
//      3                     3            signed integer
//      4                     4            signed integer
//      5                     6            signed integer
//      6                     8            signed integer
//      7                     8            IEEE float
//      8                     0            Integer constant 0
//      9                     0            Integer constant 1
//     10,11                               reserved for expansion
//    N>=12 and even       (N-12)/2        BLOB
//    N>=13 and odd        (N-13)/2        text
// ---------------------------------------------------------------------------

/// C: `u32 sqlite3VdbeSerialType(Mem *pMem, int file_format, u32 *pLen)`
///
/// Return the serial-type for the value stored in pMem (and its data
/// length in *pLen).  This routine might convert a large MEM_IntReal value
/// into MEM_Real.  (In C this routine is `#if 0`ed out because it was
/// inlined into OP_MakeRecord; it is kept here as a helper.)
pub fn sqlite3VdbeSerialType(
    db: &mut Sqlite3,
    pMem: &mut Mem,
    file_format: i32,
    pLen: &mut u32,
) -> u32 {
    serialType(pMem, file_format, pLen)
}

fn serialType(pMem: &mut Mem, file_format: i32, pLen: &mut u32) -> u32 {
    let flags = pMem.flags;
    if flags & MEM_Null != 0 {
        *pLen = 0;
        return 0;
    }
    if flags & (MEM_Int | MEM_IntReal) != 0 {
        /* Figure out whether to use 1, 2, 4, 6 or 8 bytes. */
        const MAX_6BYTE: u64 = ((0x00008000u64) << 32) - 1;
        let i = pMem.u.i;
        let u: u64 = if i < 0 { !i as u64 } else { i as u64 };
        if u <= 127 {
            if (i & 1) == i && file_format >= 4 {
                *pLen = 0;
                return 8 + u as u32;
            } else {
                *pLen = 1;
                return 1;
            }
        }
        if u <= 32767 {
            *pLen = 2;
            return 2;
        }
        if u <= 8388607 {
            *pLen = 3;
            return 3;
        }
        if u <= 2147483647 {
            *pLen = 4;
            return 4;
        }
        if u <= MAX_6BYTE {
            *pLen = 6;
            return 5;
        }
        *pLen = 8;
        if flags & MEM_IntReal != 0 {
            /* If the value is IntReal and is going to take up 8 bytes to store
             ** as an integer, then we might as well make it an 8-byte floating
             ** point value */
            pMem.u.r = pMem.u.i as f64;
            pMem.flags &= !MEM_IntReal;
            pMem.flags |= MEM_Real;
            return 7;
        }
        return 6;
    }
    if flags & MEM_Real != 0 {
        *pLen = 8;
        return 7;
    }
    debug_assert!(flags & (MEM_Str | MEM_Blob) != 0);
    debug_assert!(pMem.n >= 0);
    let mut n = pMem.n as u32;
    if flags & MEM_Zero != 0 {
        n = n.wrapping_add(pMem.u.nZero as u32);
    }
    *pLen = n;
    (n * 2) + 12 + ((flags & MEM_Str) != 0) as u32
}

/// Write the serialized data blob for the value stored in pMem into buf
/// (sqlite3VdbeSerialPut(), inlined into OP_MakeRecord in C).  It is
/// assumed that the caller has allocated sufficient space.  Return the
/// number of bytes written.  For a MEM_Zero blob only the explicit bytes
/// are written (the caller zero-fills the rest), as in C.
pub fn sqlite3VdbeSerialPut(buf: &mut [u8], pMem: &Mem, serial_type: u32) -> u32 {
    /* Integer and Real */
    if serial_type <= 7 && serial_type > 0 {
        let mut v: u64 = if serial_type == 7 {
            pMem.u.r.to_bits()
        } else {
            pMem.u.i as u64
        };
        let len = sqlite3VdbeSerialTypeLen(serial_type);
        let mut i = len as usize;
        while i > 0 {
            buf[i - 1] = (v & 0xFF) as u8;
            v >>= 8;
            i -= 1;
        }
        return len;
    }

    /* String or blob */
    if serial_type >= 12 {
        let len = pMem.n.max(0) as usize;
        if len > 0 {
            buf[..len].copy_from_slice(&pMem.bytes()[..len]);
        }
        return len as u32;
    }

    /* NULL or constants 0 or 1 */
    0
}

/// The sizes for serial types less than 128.
pub static sqlite3SmallTypeSizes: [u8; 128] = [
    /*  0   1   2   3   4   5   6   7   8   9 */
    /*   0 */ 0, 1, 2, 3, 4, 6, 8, 8, 0, 0,
    /*  10 */ 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, /*  20 */ 4, 4, 5, 5, 6, 6, 7, 7, 8, 8,
    /*  30 */ 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, /*  40 */ 14, 14, 15, 15, 16, 16, 17,
    17, 18, 18, /*  50 */ 19, 19, 20, 20, 21, 21, 22, 22, 23, 23, /*  60 */ 24, 24, 25,
    25, 26, 26, 27, 27, 28, 28, /*  70 */ 29, 29, 30, 30, 31, 31, 32, 32, 33, 33,
    /*  80 */ 34, 34, 35, 35, 36, 36, 37, 37, 38, 38, /*  90 */ 39, 39, 40, 40, 41, 41,
    42, 42, 43, 43, /* 100 */ 44, 44, 45, 45, 46, 46, 47, 47, 48, 48, /* 110 */ 49, 49,
    50, 50, 51, 51, 52, 52, 53, 53, /* 120 */ 54, 54, 55, 55, 56, 56, 57, 57,
];

/// C: `u32 sqlite3VdbeSerialTypeLen(u32 serial_type)`
/// Return the length of the data corresponding to the supplied
/// serial-type.
pub fn sqlite3VdbeSerialTypeLen(serial_type: u32) -> u32 {
    if serial_type >= 128 {
        (serial_type - 12) / 2
    } else {
        debug_assert!(
            serial_type < 12
                || sqlite3SmallTypeSizes[serial_type as usize] as u32 == (serial_type - 12) / 2
        );
        sqlite3SmallTypeSizes[serial_type as usize] as u32
    }
}

/// C: `u8 sqlite3VdbeOneByteSerialTypeLen(u8 serial_type)`
pub fn sqlite3VdbeOneByteSerialTypeLen(serial_type: u8) -> u8 {
    debug_assert!(serial_type < 128);
    sqlite3SmallTypeSizes[serial_type as usize]
}

/// C: `u64 sqlite3FloatSwap(u64 in)`
/// SQLITE_MIXED_ENDIAN_64BIT_FLOAT is not defined: identity.
pub fn sqlite3FloatSwap(in_: u64) -> u64 {
    in_
}

/// Deserialize the REAL number at buf[off..] and store it in pMem
/// (sqlite3VdbeSerialGet7()).  Returns 1 (and sets pMem to NULL) for a NaN.
fn serialGet7(buf: &[u8], off: usize, pMem: &mut Mem) -> i32 {
    /* EVIDENCE-OF: R-57343-49114 Value is a big-endian IEEE 754-2008 64-bit
     ** floating point number. */
    let x = get8byte(buf, off);
    pMem.u.r = f64::from_bits(x);
    if isNaN64(x) {
        pMem.flags = MEM_Null;
        return 1;
    }
    pMem.flags = MEM_Real;
    0
}

/// C: `void sqlite3VdbeSerialGet( const unsigned char *buf, u32 serial_type, Mem *pMem )`
///
/// Deserialize the data blob pointed to by buf as serial type serial_type
/// and store the result in pMem.  Rust: string and blob content is copied
/// into pMem.z (MEM_Ephem is kept in the flags as in C).
pub fn sqlite3VdbeSerialGet(buf: &[u8], serial_type: u32, pMem: &mut Mem) {
    match serial_type {
        10 => {
            /* Internal use only: NULL with virtual table
             ** UPDATE no-change flag set */
            pMem.flags = MEM_Null | MEM_Zero;
            pMem.n = 0;
            pMem.u.nZero = 0;
        }
        11 | 0 => {
            /* Reserved for future use / Null */
            /* EVIDENCE-OF: R-24078-09375 Value is a NULL. */
            pMem.flags = MEM_Null;
        }
        1 => {
            /* EVIDENCE-OF: R-44885-25196 Value is an 8-bit twos-complement
             ** integer. */
            pMem.u.i = beInt(buf, 0, 1);
            pMem.flags = MEM_Int;
        }
        2 => {
            /* 2-byte signed integer */
            pMem.u.i = beInt(buf, 0, 2);
            pMem.flags = MEM_Int;
        }
        3 => {
            /* 3-byte signed integer */
            pMem.u.i = beInt(buf, 0, 3);
            pMem.flags = MEM_Int;
        }
        4 => {
            /* 4-byte signed integer */
            pMem.u.i = beInt(buf, 0, 4);
            pMem.flags = MEM_Int;
        }
        5 => {
            /* 6-byte signed integer */
            pMem.u.i = beInt(buf, 0, 6);
            pMem.flags = MEM_Int;
        }
        6 => {
            /* 8-byte signed integer */
            pMem.u.i = get8byte(buf, 0) as i64;
            pMem.flags = MEM_Int;
        }
        7 => {
            /* IEEE floating point */
            serialGet7(buf, 0, pMem);
        }
        8 | 9 => {
            /* Integer 0 / Integer 1 */
            pMem.u.i = serial_type as i64 - 8;
            pMem.flags = MEM_Int;
        }
        _ => {
            /* EVIDENCE-OF: R-14606-31564 Value is a BLOB that is (N-12)/2 bytes in
             ** length.
             ** EVIDENCE-OF: R-28401-00140 Value is a string in the text encoding and
             ** (N-13)/2 bytes in length. */
            const aFlag: [u16; 2] = [MEM_Blob | MEM_Ephem, MEM_Str | MEM_Ephem];
            let n = ((serial_type - 12) / 2) as usize;
            let m = n.min(buf.len());
            pMem.z.clear();
            pMem.z.extend_from_slice(&buf[..m]);
            pMem.n = m as i32;
            pMem.flags = aFlag[(serial_type & 1) as usize];
        }
    }
}

/// C: `UnpackedRecord * sqlite3VdbeAllocUnpackedRecord( KeyInfo *pKeyInfo )`
///
/// Allocate sufficient space for an UnpackedRecord structure large enough
/// to hold a decoded index record for pKeyInfo.
pub fn sqlite3VdbeAllocUnpackedRecord(
    db: &mut Sqlite3,
    pKeyInfo: &KeyInfoRef,
) -> Option<Box<UnpackedRecord>> {
    Some(allocUnpackedRecord(pKeyInfo))
}

fn allocUnpackedRecord(pKeyInfo: &KeyInfoRef) -> Box<UnpackedRecord> {
    let nField = pKeyInfo.borrow().nKeyField as usize + 1;
    let mut p = Box::new(UnpackedRecord::default());
    p.aMem = (0..nField).map(|_| Mem::default()).collect();
    p.pKeyInfo = Some(pKeyInfo.clone());
    p.nField = nField as u16;
    p
}

/// C: `void sqlite3VdbeRecordUnpack( int nKey, const void *pKey, UnpackedRecord *p )`
///
/// Given the nKey-byte encoding of a record in pKey[], populate the
/// UnpackedRecord structure indicated by the fourth argument with the
/// contents of the decoded record.
pub fn sqlite3VdbeRecordUnpack(db: &mut Sqlite3, nKey: i32, pKey: &[u8], p: &mut UnpackedRecord) {
    recordUnpack(nKey, pKey, p)
}

fn recordUnpack(nKey: i32, aKey: &[u8], p: &mut UnpackedRecord) {
    let enc = p
        .pKeyInfo
        .as_ref()
        .map(|k| k.borrow().enc)
        .unwrap_or(SQLITE_UTF8);
    p.default_rc = 0;
    if p.aMem.len() < p.nField as usize {
        p.aMem.resize_with(p.nField as usize, Mem::default);
    }
    let (szHdr, n0) = getVarint32(aKey, 0);
    let mut idx = n0 as u32;
    let mut d = szHdr;
    let mut u: u16 = 0;
    let mut k = 0usize; /* index of pMem in p.aMem */
    while idx < szHdr && d <= nKey as u32 {
        let (serial_type, n) = getVarint32(aKey, idx as usize);
        idx += n as u32;
        let pMem = &mut p.aMem[k];
        pMem.enc = enc;
        /* pMem->flags = 0; // sqlite3VdbeSerialGet() will set this for us */
        pMem.z.clear();
        let off = (d as usize).min(aKey.len());
        sqlite3VdbeSerialGet(&aKey[off..], serial_type, pMem);
        d = d.wrapping_add(sqlite3VdbeSerialTypeLen(serial_type));
        u += 1;
        if u >= p.nField {
            break;
        }
        k += 1;
    }
    if d > nKey as u32 && u != 0 {
        /* In a corrupt record entry, the last pMem might have been set up using
         ** uninitialized memory. Overwrite its value with NULL, to prevent
         ** warnings from MSAN. */
        let j = if u < p.nField { k - 1 } else { k };
        p.aMem[j].flags = MEM_Null;
    }
    p.nField = u;
}

// ---------------------------------------------------------------------------
// Value comparison
// ---------------------------------------------------------------------------

/// Convert text from encoding `from` to encoding `to` (minimal UTF-8 <->
/// UTF-16 conversion for collations registered with a UTF-16 encoding).
fn translateText(z: &[u8], from: u8, to: u8) -> Vec<u8> {
    if from == to {
        return z.to_vec();
    }
    let units: Vec<u16> = match from {
        SQLITE_UTF16LE | SQLITE_UTF16BE => {
            let s: Vec<u16> = z
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| {
                    if from == SQLITE_UTF16LE {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            if to == SQLITE_UTF8 {
                let s: String = char::decode_utf16(s.iter().copied())
                    .map(|r| r.unwrap_or('\u{FFFD}'))
                    .collect();
                return s.into_bytes();
            }
            s
        }
        _ => String::from_utf8_lossy(z).encode_utf16().collect(),
    };
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        if to == SQLITE_UTF16LE {
            out.extend_from_slice(&u.to_le_bytes());
        } else {
            out.extend_from_slice(&u.to_be_bytes());
        }
    }
    out
}

/// memcmp() of the common prefix, then the length difference (the BINARY
/// collation).
#[inline]
fn binCmp(a: &[u8], b: &[u8]) -> i32 {
    let n = a.len().min(b.len());
    for i in 0..n {
        if a[i] != b[i] {
            return a[i] as i32 - b[i] as i32;
        }
    }
    a.len() as i32 - b.len() as i32
}

/// memcmp() of the first n bytes only.
#[inline]
fn memcmpN(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let x = at(a, i);
        let y = at(b, i);
        if x != y {
            return x as i32 - y as i32;
        }
    }
    0
}

/// Compare two strings with the collating sequence pColl
/// (vdbeCompareMemString() / vdbeCompareMemStringWithEncodingChange()).
/// `enc1`/`enc2` are the encodings of the two strings.
fn compareStrings(z1: &[u8], enc1: u8, z2: &[u8], enc2: u8, pColl: &CollSeq) -> i32 {
    let xCmp = pColl.xCmp.borrow().clone();
    let cmp = |a: &[u8], b: &[u8]| -> i32 {
        match &xCmp {
            Some(f) => f(a, b),
            None => binCmp(a, b),
        }
    };
    if enc1 == pColl.enc || pColl.enc == 0 {
        /* The strings are already in the correct encoding.  Call the
         ** comparison function directly */
        cmp(z1, z2)
    } else {
        let v1 = translateText(z1, enc1, pColl.enc);
        let v2 = translateText(z2, if enc2 == 0 { enc1 } else { enc2 }, pColl.enc);
        cmp(&v1, &v2)
    }
}

/// The input pBlob is guaranteed to be a Blob that is not marked with
/// MEM_Zero.  Return true if it could be a zero-blob.
#[inline]
fn isAllZero(z: &[u8]) -> bool {
    z.iter().all(|&c| c == 0)
}

/// C: `SQLITE_NOINLINE int sqlite3BlobCompare(const Mem *pB1, const Mem *pB2)`
///
/// Compare two blobs.  Return negative, zero, or positive if the first is
/// less than, equal to, or greater than the second, respectively.  If one
/// blob is a prefix of the other, then the shorter is the lessor.
pub fn sqlite3BlobCompare(pB1: &Mem, pB2: &Mem) -> i32 {
    let n1 = pB1.n;
    let n2 = pB2.n;

    /* It is possible to have a Blob value that has some non-zero content
     ** followed by zero content.  But that only comes up for Blobs formed
     ** by the OP_MakeRecord opcode, and such Blobs never get passed into
     ** sqlite3MemCompare(). */
    debug_assert!((pB1.flags & MEM_Zero) == 0 || n1 == 0);
    debug_assert!((pB2.flags & MEM_Zero) == 0 || n2 == 0);

    if (pB1.flags | pB2.flags) & MEM_Zero != 0 {
        if pB1.flags & pB2.flags & MEM_Zero != 0 {
            return pB1.u.nZero - pB2.u.nZero;
        } else if pB1.flags & MEM_Zero != 0 {
            if !isAllZero(pB2.bytes()) {
                return -1;
            }
            return pB1.u.nZero - n2;
        } else {
            if !isAllZero(pB1.bytes()) {
                return 1;
            }
            return n1 - pB2.u.nZero;
        }
    }
    let c = memcmpN(pB1.bytes(), pB2.bytes(), n1.min(n2).max(0) as usize);
    if c != 0 {
        return c;
    }
    n1 - n2
}

/// C: `int sqlite3IntFloatCompare(i64 i, double r)`
///
/// Do a comparison between a 64-bit signed integer and a 64-bit
/// floating-point number.  Return negative, zero, or positive if the first
/// (i64) is less than, equal to, or greater than the second (double).
pub fn sqlite3IntFloatCompare(i: i64, r: f64) -> i32 {
    if r.is_nan() {
        /* SQLite considers NaN to be a NULL. And all integer values are greater
         ** than NULL */
        1
    } else {
        if r < -9223372036854775808.0 {
            return 1;
        }
        if r >= 9223372036854775808.0 {
            return -1;
        }
        let y = r as i64;
        if i < y {
            return -1;
        }
        if i > y {
            return 1;
        }
        if (i as f64) < r {
            -1
        } else {
            ((i as f64) > r) as i32
        }
    }
}

/// C: `int sqlite3MemCompare(const Mem *pMem1, const Mem *pMem2, const CollSeq *pColl)`
///
/// Compare the values contained by the two memory cells, returning
/// negative, zero or positive if pMem1 is less than, equal to, or greater
/// than pMem2.  Sorting order is NULL's first, followed by numbers
/// (integers and reals) sorted numerically, followed by text ordered by
/// the collating sequence pColl and finally blob's ordered by memcmp().
///
/// Two NULL values are considered equal by this function.
/// Rust: pColl is an Option (C NULL means memcmp()); db is not used.
pub fn sqlite3MemCompare(
    db: &mut Sqlite3,
    pMem1: &Mem,
    pMem2: &Mem,
    pColl: Option<&CollSeqRef>,
) -> i32 {
    memCompare(pMem1, pMem2, pColl.map(|c| &**c))
}

/// sqlite3MemCompare() without the connection.
pub fn memCompare(pMem1: &Mem, pMem2: &Mem, pColl: Option<&CollSeq>) -> i32 {
    let f1 = pMem1.flags;
    let f2 = pMem2.flags;
    let combined_flags = f1 | f2;

    /* If one value is NULL, it is less than the other. If both values
     ** are NULL, return 0.
     */
    if combined_flags & MEM_Null != 0 {
        return (f2 & MEM_Null) as i32 - (f1 & MEM_Null) as i32;
    }

    /* At least one of the two values is a number
     */
    if combined_flags & (MEM_Int | MEM_Real | MEM_IntReal) != 0 {
        if (f1 & f2 & (MEM_Int | MEM_IntReal)) != 0 {
            if pMem1.u.i < pMem2.u.i {
                return -1;
            }
            if pMem1.u.i > pMem2.u.i {
                return 1;
            }
            return 0;
        }
        if (f1 & f2 & MEM_Real) != 0 {
            if pMem1.u.r < pMem2.u.r {
                return -1;
            }
            if pMem1.u.r > pMem2.u.r {
                return 1;
            }
            return 0;
        }
        if (f1 & (MEM_Int | MEM_IntReal)) != 0 {
            if (f2 & MEM_Real) != 0 {
                return sqlite3IntFloatCompare(pMem1.u.i, pMem2.u.r);
            } else if (f2 & (MEM_Int | MEM_IntReal)) != 0 {
                if pMem1.u.i < pMem2.u.i {
                    return -1;
                }
                if pMem1.u.i > pMem2.u.i {
                    return 1;
                }
                return 0;
            } else {
                return -1;
            }
        }
        if (f1 & MEM_Real) != 0 {
            if (f2 & (MEM_Int | MEM_IntReal)) != 0 {
                return -sqlite3IntFloatCompare(pMem2.u.i, pMem1.u.r);
            } else {
                return -1;
            }
        }
        return 1;
    }

    /* If one value is a string and the other is a blob, the string is less.
     ** If both are strings, compare using the collating functions.
     */
    if combined_flags & MEM_Str != 0 {
        if (f1 & MEM_Str) == 0 {
            return 1;
        }
        if (f2 & MEM_Str) == 0 {
            return -1;
        }

        /* The collation sequence must be defined at this point, even if
         ** the user deletes the collation sequence after the vdbe program is
         ** compiled (this was not always the case).
         */
        if let Some(pColl) = pColl {
            return compareStrings(pMem1.bytes(), pMem1.enc, pMem2.bytes(), pMem2.enc, pColl);
        }
        /* If a NULL pointer was passed as the collate function, fall through
         ** to the blob case and use memcmp().  */
    }

    /* Both values must be blobs.  Compare using memcmp().  */
    sqlite3BlobCompare(pMem1, pMem2)
}

/// The first argument passed to this function is a serial-type that
/// corresponds to an integer - all values between 1 and 9 inclusive except
/// 7.  The second points to a buffer containing an integer value
/// serialized according to serial_type.  This function deserializes and
/// returns the value (vdbeRecordDecodeInt()).
fn vdbeRecordDecodeInt(serial_type: u32, aKey: &[u8], off: usize) -> i64 {
    match serial_type {
        0 | 1 => beInt(aKey, off, 1),
        2 => beInt(aKey, off, 2),
        3 => beInt(aKey, off, 3),
        4 => beInt(aKey, off, 4),
        5 => beInt(aKey, off, 6),
        6 => get8byte(aKey, off) as i64,
        _ => serial_type as i64 - 8,
    }
}

/// C: `int sqlite3VdbeRecordCompareWithSkip( int nKey1, const void *pKey1, UnpackedRecord *pPKey2, int bSkip )`
///
/// This function compares the two table rows or index records specified
/// by {nKey1, pKey1} and pPKey2.  It returns a negative, zero or positive
/// integer if key1 is less than, equal to or greater than key2.  The
/// {nKey1, pKey1} key must be a blob created by the OP_MakeRecord opcode
/// of the VDBE.  The pPKey2 key must be a parsed key such as obtained from
/// sqlite3VdbeParseRecord.
///
/// If argument bSkip is non-zero, it is assumed that the caller has
/// already determined that the first fields of the keys are equal.
///
/// Key1 and Key2 do not have to contain the same number of fields.  If all
/// fields that appear in both keys are equal, then pPKey2->default_rc is
/// returned.
///
/// If database corruption is discovered, set pPKey2->errCode to
/// SQLITE_CORRUPT and return 0.
pub fn sqlite3VdbeRecordCompareWithSkip(
    db: &mut Sqlite3,
    nKey1: i32,
    pKey1: &[u8],
    pPKey2: &mut UnpackedRecord,
    bSkip: i32,
) -> i32 {
    recordCompareWithSkip(nKey1, pKey1, pPKey2, bSkip)
}

fn recordCompareWithSkip(nKey1: i32, aKey1: &[u8], pPKey2: &mut UnpackedRecord, bSkip: i32) -> i32 {
    let mut d1: u32; /* Offset into aKey[] of next data element */
    let mut i: usize; /* Index of next field to compare */
    let szHdr1: u32; /* Size of record header in bytes */
    let mut idx1: u32; /* Offset of first type in header */
    let mut rc: i32 = 0; /* Return value */
    let mut mem1 = Mem::default();

    let kiRc = pPKey2
        .pKeyInfo
        .clone()
        .expect("UnpackedRecord without KeyInfo");
    let pKeyInfo = kiRc.borrow();

    /* If bSkip is true, then the caller has already determined that the first
     ** two elements in the keys are equal. Fix the various stack variables so
     ** that this routine begins comparing at the second field. */
    if bSkip != 0 {
        let mut s1 = at(aKey1, 1) as u32;
        if s1 < 0x80 {
            idx1 = 2;
        } else {
            let (v, n) = getVarint32(aKey1, 1);
            s1 = v;
            idx1 = 1 + n as u32;
        }
        szHdr1 = at(aKey1, 0) as u32;
        d1 = szHdr1 + sqlite3VdbeSerialTypeLen(s1);
        i = 1;
    } else {
        let h = at(aKey1, 0) as u32;
        if h < 0x80 {
            szHdr1 = h;
            idx1 = 1;
        } else {
            let (v, n) = getVarint32(aKey1, 0);
            szHdr1 = v;
            idx1 = n as u32;
        }
        d1 = szHdr1;
        i = 0;
    }
    if d1 > nKey1 as u32 {
        pPKey2.errCode = SQLITE_CORRUPT as u8;
        return 0; /* Corruption */
    }

    debug_assert!(pKeyInfo.nKeyField > 0);
    loop {
        /*exit-by-break*/
        let serial_type: u32;
        let pRhs = &pPKey2.aMem[i];

        /* RHS is an integer */
        if pRhs.flags & (MEM_Int | MEM_IntReal) != 0 {
            serial_type = at(aKey1, idx1 as usize) as u32;
            if serial_type >= 10 {
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else if serial_type == 7 {
                serialGet7(aKey1, d1 as usize, &mut mem1);
                rc = -sqlite3IntFloatCompare(pRhs.u.i, mem1.u.r);
            } else {
                let lhs = vdbeRecordDecodeInt(serial_type, aKey1, d1 as usize);
                let rhs = pRhs.u.i;
                if lhs < rhs {
                    rc = -1;
                } else if lhs > rhs {
                    rc = 1;
                }
            }
        }
        /* RHS is real */
        else if pRhs.flags & MEM_Real != 0 {
            serial_type = at(aKey1, idx1 as usize) as u32;
            if serial_type >= 10 {
                /* Serial types 12 or greater are strings and blobs (greater than
                 ** numbers). Types 10 and 11 are currently "reserved for future
                 ** use", so it doesn't really matter what the results of comparing
                 ** them to numeric values are.  */
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else if serial_type == 7 {
                if serialGet7(aKey1, d1 as usize, &mut mem1) != 0 {
                    rc = -1; /* mem1 is a NaN */
                } else if mem1.u.r < pRhs.u.r {
                    rc = -1;
                } else if mem1.u.r > pRhs.u.r {
                    rc = 1;
                } else {
                    debug_assert!(rc == 0);
                }
            } else {
                let lhs = vdbeRecordDecodeInt(serial_type, aKey1, d1 as usize);
                rc = sqlite3IntFloatCompare(lhs, pRhs.u.r);
            }
        }
        /* RHS is a string */
        else if pRhs.flags & MEM_Str != 0 {
            let (st, _) = getVarint32(aKey1, idx1 as usize);
            serial_type = st;
            if serial_type < 12 {
                rc = -1;
            } else if serial_type & 0x01 == 0 {
                rc = 1;
            } else {
                let n1 = (serial_type - 12) / 2;
                if (d1 as u64 + n1 as u64) > nKey1 as u64 || pKeyInfo.nAllField as usize <= i {
                    pPKey2.errCode = SQLITE_CORRUPT as u8;
                    return 0; /* Corruption */
                }
                let lo = (d1 as usize).min(aKey1.len());
                let hi = (d1 as usize + n1 as usize).min(aKey1.len());
                let z1 = &aKey1[lo..hi];
                if let Some(Some(pColl)) = pKeyInfo.aColl.get(i) {
                    rc = compareStrings(z1, pKeyInfo.enc, pRhs.bytes(), pRhs.enc, pColl);
                } else {
                    let nCmp = (n1 as i32).min(pRhs.n).max(0) as usize;
                    rc = memcmpN(z1, pRhs.bytes(), nCmp);
                    if rc == 0 {
                        rc = n1 as i32 - pRhs.n;
                    }
                }
            }
        }
        /* RHS is a blob */
        else if pRhs.flags & MEM_Blob != 0 {
            debug_assert!((pRhs.flags & MEM_Zero) == 0 || pRhs.n == 0);
            let (st, _) = getVarint32(aKey1, idx1 as usize);
            serial_type = st;
            if serial_type < 12 || (serial_type & 0x01) != 0 {
                rc = -1;
            } else {
                let nStr = ((serial_type - 12) / 2) as usize;
                if (d1 as u64 + nStr as u64) > nKey1 as u64 {
                    pPKey2.errCode = SQLITE_CORRUPT as u8;
                    return 0; /* Corruption */
                }
                let lo = (d1 as usize).min(aKey1.len());
                let hi = (d1 as usize + nStr).min(aKey1.len());
                let z1 = &aKey1[lo..hi];
                if pRhs.flags & MEM_Zero != 0 {
                    if !isAllZero(z1) {
                        rc = 1;
                    } else {
                        rc = nStr as i32 - pRhs.u.nZero;
                    }
                } else {
                    let nCmp = (nStr as i32).min(pRhs.n).max(0) as usize;
                    rc = memcmpN(z1, pRhs.bytes(), nCmp);
                    if rc == 0 {
                        rc = nStr as i32 - pRhs.n;
                    }
                }
            }
        }
        /* RHS is null */
        else {
            serial_type = at(aKey1, idx1 as usize) as u32;
            if serial_type == 0
                || serial_type == 10
                || (serial_type == 7 && serialGet7(aKey1, d1 as usize, &mut mem1) != 0)
            {
                debug_assert!(rc == 0);
            } else {
                rc = 1;
            }
        }

        if rc != 0 {
            let sortFlags = pKeyInfo.aSortFlags.get(i).copied().unwrap_or(0);
            if sortFlags != 0 {
                if (sortFlags & KEYINFO_ORDER_BIGNULL) == 0
                    || ((sortFlags & KEYINFO_ORDER_DESC) != 0)
                        != (serial_type == 0 || (pRhs.flags & MEM_Null) != 0)
                {
                    rc = -rc;
                }
            }
            return rc;
        }

        i += 1;
        if i == pPKey2.nField as usize {
            break;
        }
        d1 = d1.wrapping_add(sqlite3VdbeSerialTypeLen(serial_type));
        if d1 > nKey1 as u32 {
            break;
        }
        idx1 += varintLen(serial_type as u64);
        if idx1 >= szHdr1 {
            pPKey2.errCode = SQLITE_CORRUPT as u8;
            return 0; /* Corrupt index */
        }
    }

    /* rc==0 here means that one or both of the keys ran out of fields and
     ** all the fields up to that point were equal. Return the default_rc
     ** value.  */
    pPKey2.eqSeen = 1;
    pPKey2.default_rc as i32
}

/// C: `int sqlite3VdbeRecordCompare( int nKey1, const void *pKey1, UnpackedRecord *pPKey2 )`
pub fn sqlite3VdbeRecordCompare(nKey1: i32, pKey1: &[u8], pPKey2: &mut UnpackedRecord) -> i32 {
    recordCompareWithSkip(nKey1, pKey1, pPKey2, 0)
}

/// sqlite3VdbeRecordCompare() as a RecordCompare (the record is the whole
/// slice).
fn vdbeRecordCompareDefault(pKey1: &[u8], pPKey2: &mut UnpackedRecord) -> i32 {
    recordCompareWithSkip(pKey1.len() as i32, pKey1, pPKey2, 0)
}

/// vdbeRecordCompareInt()
///
/// This function is an optimized version of sqlite3VdbeRecordCompare()
/// that (a) the first field of pPKey2 is an integer, and (b) the
/// size-of-header varint at the start of (pKey1/nKey1) fits in a single
/// byte (i.e. is less than 128).
fn vdbeRecordCompareInt(pKey1: &[u8], pPKey2: &mut UnpackedRecord) -> i32 {
    let nKey1 = pKey1.len() as i32;
    let off = (at(pKey1, 0) & 0x3F) as usize;
    let serial_type = at(pKey1, 1) as i32;
    let res: i32;
    let lhs: i64;

    /* Serial types 1 through 6 are big-endian integers of 1, 2, 3, 4,
     ** 6, or 8 bytes.  Rather than handle each width in its own switch
     ** case, read 8 bytes and use an arithmetic right shift to drop the
     ** unwanted low-order bytes and sign-extend the value. */
    if ((serial_type - 1) as u32) <= 5 {
        const aShift: [u8; 7] = [0, 56, 48, 40, 32, 16, 0];
        lhs = (get8byte(pKey1, off) as i64) >> aShift[serial_type as usize];
    } else if serial_type == 8 || serial_type == 9 {
        lhs = serial_type as i64 - 8;
    } else {
        return sqlite3VdbeRecordCompare(nKey1, pKey1, pPKey2);
    }

    let v = pPKey2.u_i;
    if v > lhs {
        res = pPKey2.r1 as i32;
    } else if v < lhs {
        res = pPKey2.r2 as i32;
    } else if pPKey2.nField > 1 {
        /* The first fields of the two keys are equal. Compare the trailing
         ** fields.  */
        res = recordCompareWithSkip(nKey1, pKey1, pPKey2, 1);
    } else {
        /* The first fields of the two keys are equal and there are no trailing
         ** fields. Return pPKey2->default_rc in this case. */
        res = pPKey2.default_rc as i32;
        pPKey2.eqSeen = 1;
    }
    res
}

/// vdbeRecordCompareString()
///
/// This function is an optimized version of sqlite3VdbeRecordCompare()
/// that (a) the first field of pPKey2 is a string, that (b) the first
/// field uses the collation sequence BINARY and (c) that the
/// size-of-header varint at the start of (pKey1/nKey1) fits in a single
/// byte.
fn vdbeRecordCompareString(pKey1: &[u8], pPKey2: &mut UnpackedRecord) -> i32 {
    let nKey1 = pKey1.len() as i32;
    let mut serial_type: i32 = (at(pKey1, 1) as i8) as i32;
    let mut res: i32;

    loop {
        /* vrcs_restart: */
        if serial_type < 12 {
            if serial_type < 0 {
                let (v, _) = getVarint32(pKey1, 1);
                serial_type = v as i32;
                if serial_type >= 12 {
                    continue;
                }
            }
            res = pPKey2.r1 as i32; /* (pKey1/nKey1) is a number or a null */
        } else if serial_type & 0x01 == 0 {
            res = pPKey2.r2 as i32; /* (pKey1/nKey1) is a blob */
        } else {
            let szHdr = at(pKey1, 0) as i32;
            let nStr = (serial_type - 12) / 2;
            if (szHdr + nStr) > nKey1 {
                pPKey2.errCode = SQLITE_CORRUPT as u8;
                return 0; /* Corruption */
            }
            let nCmp = pPKey2.n.min(nStr).max(0) as usize;
            res = memcmpN(&pKey1[szHdr as usize..], &pPKey2.u_z, nCmp);

            if res > 0 {
                res = pPKey2.r2 as i32;
            } else if res < 0 {
                res = pPKey2.r1 as i32;
            } else {
                res = nStr - pPKey2.n;
                if res == 0 {
                    if pPKey2.nField > 1 {
                        res = recordCompareWithSkip(nKey1, pKey1, pPKey2, 1);
                    } else {
                        res = pPKey2.default_rc as i32;
                        pPKey2.eqSeen = 1;
                    }
                } else if res > 0 {
                    res = pPKey2.r2 as i32;
                } else {
                    res = pPKey2.r1 as i32;
                }
            }
        }
        break;
    }
    res
}

/// C: `RecordCompare sqlite3VdbeFindCompare(UnpackedRecord *p)`
///
/// Return a sqlite3VdbeRecordCompare() compatible function suitable for
/// comparing serialized records to the unpacked record passed as the only
/// argument.  The returned function takes the record as a slice holding
/// exactly the record bytes.
pub fn sqlite3VdbeFindCompare(p: &mut UnpackedRecord) -> RecordCompare {
    /* vdbeRecordCompareInt() and vdbeRecordCompareString() both assume
     ** that the size-of-header varint that occurs at the start of each record
     ** fits in a single byte (i.e. is 127 or less).  The easiest way to
     ** enforce this limit is to consider only records with 13 fields or less.
     ** If the first field is an integer, the maximum legal header size is
     ** (12*5 + 1 + 1) bytes.  */
    let kiRc = p.pKeyInfo.clone().expect("UnpackedRecord without KeyInfo");
    let ki = kiRc.borrow();
    if ki.nAllField <= 13 {
        let flags = p.aMem[0].flags;
        let sf0 = ki.aSortFlags.first().copied().unwrap_or(0);
        if sf0 != 0 {
            if sf0 & KEYINFO_ORDER_BIGNULL != 0 {
                return vdbeRecordCompareDefault;
            }
            p.r1 = 1;
            p.r2 = -1;
        } else {
            p.r1 = -1;
            p.r2 = 1;
        }
        if flags & MEM_Int != 0 {
            p.u_i = p.aMem[0].u.i;
            return vdbeRecordCompareInt;
        }
        let isBinary = match ki.aColl.first() {
            None | Some(None) => true,
            Some(Some(c)) => crate::mainapi::sqlite3IsBinary(Some(c)) != 0,
        };
        if (flags & (MEM_Real | MEM_IntReal | MEM_Null | MEM_Blob)) == 0 && isBinary {
            debug_assert!(flags & MEM_Str != 0);
            p.u_z = p.aMem[0].bytes().to_vec();
            p.n = p.aMem[0].n;
            return vdbeRecordCompareString;
        }
    }

    vdbeRecordCompareDefault
}

/// C: `int sqlite3VdbeIdxRowid(sqlite3 *db, BtCursor *pCur, i64 *rowid)`
///
/// pCur points at an index entry created using the OP_MakeRecord opcode.
/// Read the rowid (the last field in the record) and store it in *rowid.
/// Return SQLITE_OK if everything works, or an error code otherwise.
///
/// pCur might be pointing to text obtained from a corrupt database file.
/// So the content cannot be trusted.  Do appropriate checks on the
/// content.
pub fn sqlite3VdbeIdxRowid(db: &mut Sqlite3, pCur: &BtCursorP, rowid: &mut i64) -> i32 {
    /* Get the size of the index entry.  Only indices entries of less
     ** than 2GiB are support - anything large must be database corruption.
     ** Any corruption is detected in sqlite3BtreeParseCellPtr(), though, so
     ** this code can safely assume that nCellKey is 32-bits
     */
    let nCellKey = crate::btree::sqlite3BtreePayloadSize(pCur);

    /* Read in the complete content of the index entry */
    let mut m = Mem::default();
    let rc = crate::vdbemem::sqlite3VdbeMemFromBtreeZeroOffset(pCur, nCellKey, &mut m);
    if rc != 0 {
        return rc;
    }
    let z = m.bytes();
    let n = z.len() as u32;

    /* The index entry must begin with a header size */
    let (szHdr, _) = getVarint32(z, 0);
    if szHdr < 3 || szHdr > n {
        return SQLITE_CORRUPT;
    }

    /* The last field of the index should be an integer - the ROWID.
     ** Verify that the last entry really is an integer. */
    let (typeRowid, _) = getVarint32(z, (szHdr - 1) as usize);
    if typeRowid < 1 || typeRowid > 9 || typeRowid == 7 {
        return SQLITE_CORRUPT;
    }
    let lenRowid = sqlite3SmallTypeSizes[typeRowid as usize] as u32;
    if n < szHdr + lenRowid {
        return SQLITE_CORRUPT;
    }

    /* Fetch the integer off the end of the index record */
    let mut v = Mem::default();
    sqlite3VdbeSerialGet(&z[(n - lenRowid) as usize..], typeRowid, &mut v);
    *rowid = v.u.i;
    SQLITE_OK
}

/// C: `int sqlite3VdbeIdxKeyCompare( sqlite3 *db, VdbeCursor *pC, UnpackedRecord *pUnpacked, int *res )`
///
/// Compare the key of the index entry that cursor pC is pointing to
/// against the key string in pUnpacked.  Write into *pRes a number that is
/// negative, zero, or positive if pC is less than, equal to, or greater
/// than pUnpacked.  Return SQLITE_OK on success.
///
/// pUnpacked is either created without a rowid or is truncated so that it
/// omits the rowid at the end.  The rowid at the end of the index entry is
/// ignored as well.  Hence, this routine only compares the prefixes of the
/// keys prior to the final rowid, not the entire key.
pub fn sqlite3VdbeIdxKeyCompare(
    db: &mut Sqlite3,
    pC: &mut VdbeCursor,
    pUnpacked: &mut UnpackedRecord,
    res: &mut i32,
) -> i32 {
    debug_assert!(pC.eCurType == CURTYPE_BTREE);
    let pCur = pC.pCursor.clone().expect("btree cursor");
    let nCellKey = crate::btree::sqlite3BtreePayloadSize(&pCur) as i64;
    /* nCellKey will always be between 0 and 0xffffffff because of the way
     ** that btreeParseCellPtr() and sqlite3GetVarint32() are implemented */
    if nCellKey <= 0 || nCellKey > 0x7fffffff {
        *res = 0;
        return SQLITE_CORRUPT;
    }
    let mut m = Mem::default();
    let rc = crate::vdbemem::sqlite3VdbeMemFromBtreeZeroOffset(&pCur, nCellKey as u32, &mut m);
    if rc != 0 {
        return rc;
    }
    *res = recordCompareWithSkip(m.n, m.bytes(), pUnpacked, 0);
    SQLITE_OK
}

/// C: `void sqlite3VdbeSetChanges(sqlite3 *db, i64 nChange)`
///
/// This routine sets the value to be returned by subsequent calls to
/// sqlite3_changes() on the database handle 'db'.
pub fn sqlite3VdbeSetChanges(db: &mut Sqlite3, nChange: i64) {
    db.nChange = nChange;
    db.nTotalChange = db.nTotalChange.wrapping_add(nChange);
}

/// C: `void sqlite3VdbeCountChanges(Vdbe *v)`
///
/// Set a flag in the vdbe to update the change counter when it is
/// finalised or reset.
pub fn sqlite3VdbeCountChanges(v: &mut Vdbe) {
    v.changeCntOn = 1;
}

/// C: `void sqlite3ExpirePreparedStatements(sqlite3 *db, int iCode)`
///
/// Mark every prepared statement associated with a database connection as
/// expired.
///
/// An expired statement means that recompilation of the statement is
/// recommend.  Statements expire when things happen that make their
/// programs obsolete.  Removing user-defined functions or collating
/// sequences, or changing an authorization function are the types of
/// things that make prepared statements obsolete.
///
/// If iCode is 1, then expiration is advisory.  The statement should be
/// reprepared before being restarted, but if it is already running it is
/// allowed to run to completion.
///
/// Internally, this function just sets the Vdbe.expired flag on all
/// prepared statements.  The flag is set to 1 for an immediate expiration
/// and set to 2 for an advisory expiration.
///
/// Rust: a statement taken out of its slot (running) gets the flag through
/// VdbeSlot::expired (merged back by stmt_put()); statements still being
/// compiled (in db.parses, linked in db->pVdbe in C) are marked too.
pub fn sqlite3ExpirePreparedStatements(db: &mut Sqlite3, iCode: i32) {
    let e = (iCode + 1) as u8;
    for slot in db.aVdbe.iter_mut() {
        if !slot.used {
            continue;
        }
        match slot.v.as_mut() {
            Some(v) => v.expired = e,
            None => slot.expired = e,
        }
    }
    for pParse in db.parses.iter_mut() {
        if let Some(v) = pParse.pVdbe.as_mut() {
            v.expired = e;
        }
    }
}

/// C: `sqlite3 * sqlite3VdbeDb(Vdbe *v)`
/// Rust: the connection is always passed explicitly.
pub fn sqlite3VdbeDb(db: &mut Sqlite3, v: &mut Vdbe) -> () {}

/// C: `u8 sqlite3VdbePrepareFlags(Vdbe *v)`
/// Return the SQLITE_PREPARE flags for a Vdbe.
pub fn sqlite3VdbePrepareFlags(v: &mut Vdbe) -> u8 {
    v.prepFlags
}

/// C: `sqlite3_value * sqlite3VdbeGetBoundValue(Vdbe *v, int iVar, u8 aff)`
///
/// Return an sqlite3_value structure containing the value bound parameter
/// iVar of VM v.  Except, if the value is an SQL NULL, return 0 instead.
/// Unless it is NULL, apply affinity aff (one of the SQLITE_AFF_*
/// constants) to the value before returning it.
pub fn sqlite3VdbeGetBoundValue(
    db: &mut Sqlite3,
    v: &mut Vdbe,
    iVar: i32,
    aff: u8,
) -> Option<Box<Mem>> {
    debug_assert!(iVar > 0);
    let pMem = &v.aVar[(iVar - 1) as usize];
    if 0 == (pMem.flags & MEM_Null) {
        let mut pRet = Box::new(Mem::default());
        crate::vdbemem::sqlite3VdbeMemCopy(&mut pRet, pMem);
        crate::vdbe::sqlite3ValueApplyAffinity(db, &mut pRet, aff, SQLITE_UTF8);
        return Some(pRet);
    }
    None
}

/// C: `void sqlite3VdbeReprepareOnBind(Vdbe *v, int iVar, int bSmallint)`
///
/// Configure SQL variable iVar so that binding a new value to it signals
/// to sqlite3_reoptimize() that re-preparing the statement may result in
/// a better query plan.  If parameter bSmallint is true, then the
/// statement is only re-prepared if the new value is integer value 0 or 1.
///
/// The v->expmask bit is always set.  expmask means that a reprepare is
/// possible.  The v->smimask bit is only set if we want to restrict
/// reprepare when the value changes from (0,1) to something else, or from
/// something else to (0,1).
pub fn sqlite3VdbeReprepareOnBind(db: &mut Sqlite3, v: &mut Vdbe, iVar: i32, bSmallint: i32) {
    debug_assert!(iVar > 0);
    let m: u32 = if iVar >= 32 {
        0x80000000
    } else {
        1u32 << (iVar - 1)
    };
    if bSmallint != 0 {
        if (v.expmask & m) == 0 {
            v.smimask |= m;
        }
    } else {
        v.smimask &= !m;
    }
    v.expmask |= m;

    /* smimask is always a subset of expmask */
    debug_assert!((v.smimask & v.expmask) == v.smimask);
}

/// vdbeSkipField()
///
/// Helper function for vdbeIsMatchingIndexKey().  Return true if column
/// iCol should be ignored when comparing a record with a record from an
/// index on disk.  The field should be ignored if:
///
///   * the corresponding bit in mask is set, and
///   * either:
///       - bIntegrity is false, or
///       - the two Mem values are both real values that differ by
///         BTREE_ULPDISTORTION or fewer ULPs.
fn vdbeSkipField(mask: Bitmask, iCol: i32, pMem1: &Mem, pMem2: &Mem, bIntegrity: i32) -> bool {
    const BTREE_ULPDISTORTION: u64 = 2;
    if iCol >= BMS || (mask & MASKBIT(iCol)) == 0 {
        return false;
    }
    if bIntegrity == 0 {
        return true;
    }
    if (pMem1.flags & MEM_Real) != 0 && (pMem2.flags & MEM_Real) != 0 {
        let m1 = pMem1.u.r.to_bits();
        let m2 = pMem2.u.r.to_bits();
        if (if m1 < m2 { m2 - m1 } else { m1 - m2 }) <= BTREE_ULPDISTORTION {
            return true;
        }
    }
    false
}

/// vdbeIsMatchingIndexKey()
///
/// This function compares the unpacked record with the current key that
/// cursor pCur points to.  If bInt is false, all fields for which the
/// corresponding bit in parameter "mask" is set are ignored.  Or, if bInt
/// is true, then a difference of BTREE_ULPDISTORTION or fewer ULPs in real
/// values is overlooked for fields with the corresponding bit set in mask.
///
/// Return the usual less than zero, zero, or greater than zero if the
/// remaining fields of the cursor cursor key are less than, equal to or
/// greater than those in (*p).
fn vdbeIsMatchingIndexKey(
    pCur: &BtCursorP,
    bInt: i32,
    mask: Bitmask,
    p: &mut UnpackedRecord,
    piRes: &mut i32,
) -> i32 {
    let mut rc;
    let kiRc = p.pKeyInfo.clone().expect("UnpackedRecord without KeyInfo");
    let mut m = Mem::default();
    m.enc = kiRc.borrow().enc;
    let nRec = crate::btree::sqlite3BtreePayloadSize(pCur);
    if nRec > 0x7fffffff {
        return SQLITE_CORRUPT;
    }

    /* Allocate 5 extra bytes at the end of the buffer. This allows the
     ** getVarint32() call below to read slightly past the end of the buffer
     ** if the record is corrupt. */
    let mut aRec = vec![0u8; nRec as usize + 5];
    rc = crate::btree::sqlite3BtreePayload(pCur, 0, nRec, &mut aRec[..nRec as usize]);

    if rc == SQLITE_OK {
        let (szHdr, n0) = getVarint32(&aRec, 0);
        let mut idxHdr = n0 as u32;
        if szHdr > 98307 {
            rc = SQLITE_CORRUPT;
        } else {
            let mut res = 0;
            let mut idxRec = szHdr;
            let ki = kiRc.borrow();
            let nCol = ki.nAllField as usize;
            let mut ii = 0usize;
            while ii < nCol && rc == SQLITE_OK {
                if idxHdr >= szHdr {
                    rc = SQLITE_CORRUPT;
                    break;
                }
                let (iSerial, n) = getVarint32(&aRec, idxHdr as usize);
                idxHdr += n as u32;
                let nSerial = sqlite3VdbeSerialTypeLen(iSerial);
                if (idxRec as u64 + nSerial as u64) > nRec as u64 {
                    rc = SQLITE_CORRUPT;
                } else {
                    sqlite3VdbeSerialGet(&aRec[idxRec as usize..], iSerial, &mut m);
                    if ii < p.aMem.len() && !vdbeSkipField(mask, ii as i32, &p.aMem[ii], &m, bInt) {
                        let pColl = ki.aColl.get(ii).and_then(|c| c.as_deref());
                        res = memCompare(&m, &p.aMem[ii], pColl);
                        if res != 0 {
                            break;
                        }
                    }
                }
                idxRec = idxRec.wrapping_add(nSerial);
                ii += 1;
            }

            *piRes = res;
        }
    }
    rc
}

/// C: `int sqlite3VdbeFindIndexKey( BtCursor *pCur, Index *pIdx, UnpackedRecord *p, int *pRes, int bIntegrity )`
///
/// This is called when the record in (*p) should be found in the index
/// opened by cursor pCur, but was not.  This may happen as part of a
/// DELETE operation or an integrity check.
///
/// One reason that an exact match was not found may be the EIIB bug -
/// that a text-to-float conversion may have caused a real value in record
/// (*p) to be slightly different from its counterpart on disk.  This
/// function attempts to find the right index record.  If it does find the
/// right record, it leaves *pCur pointing to it and sets (*pRes) to 0
/// before returning.  Otherwise, (*pRes) is set to non-zero and an SQLite
/// error code returned.
///
/// The algorithm used to find the correct record is:
///
///   * Scan up to BTREE_FDK_RANGE entries either side of the current
///     entry.  If parameter bIntegrity is false, then all fields that are
///     indexed expressions or virtual table columns are omitted from the
///     comparison.  If bIntegrity is true, then small differences in real
///     values in such fields are overlooked, but they are not omitted from
///     the comparison altogether.
///
///   * If the above fails to find an entry and bIntegrity is false,
///     search the entire index.
///
/// Rust: takes `db` (needed by sqlite3BtreeFirst()).
pub fn sqlite3VdbeFindIndexKey(
    db: &mut Sqlite3,
    pCur: &BtCursorP,
    pIdx: &IndexRef,
    p: &mut UnpackedRecord,
    pRes: &mut i32,
    bIntegrity: i32,
) -> i32 {
    const BTREE_FDK_RANGE: i32 = 10;
    let mut nStep: i32;
    let mut res = 1;
    let mut rc = SQLITE_OK;

    /* Calculate a mask based on the first 64 columns of the index. The mask
     ** bit is set if the corresponding index field is either an expression
     ** or a virtual column of the table.  */
    let mut mask: Bitmask = 0;
    {
        let idx = pIdx.borrow();
        let pTab = idx.table();
        let tab = pTab.borrow();
        for ii in 0..(idx.nColumn as i32).min(BMS) {
            let iCol = idx.aiColumn[ii as usize];
            if iCol == XN_EXPR
                || (iCol >= 0 && (tab.aCol[iCol as usize].colFlags & COLFLAG_VIRTUAL) != 0)
            {
                mask |= MASKBIT(ii);
            }
        }
    }

    /* If the mask is 0 at this point, then the index contains no expressions
     ** or virtual columns. So do not search for a match - return so that the
     ** caller may declare the db corrupt immediately. Or, if mask is non-zero,
     ** proceed.  */
    if mask != 0 {
        /* Move the cursor back BTREE_FDK_RANGE entries. If this hits an EOF,
         ** position the cursor at the first entry in the index and set nStep
         ** to -1 so that the first loop below scans the entire index. Otherwise,
         ** set nStep to BTREE_FDK_RANGE*2 so that the first loop below scans
         ** just that many entries.  */
        let mut ii = 0;
        while crate::btree::sqlite3BtreeEof(pCur) == 0 && ii < BTREE_FDK_RANGE {
            rc = crate::btree::sqlite3BtreePrevious(pCur, 0);
            ii += 1;
        }
        if rc == SQLITE_DONE {
            rc = crate::btree::sqlite3BtreeFirst(db, pCur, &mut res);
            nStep = -1;
        } else {
            nStep = BTREE_FDK_RANGE * 2;
        }

        /* This loop runs at most twice to search for a key with matching PK
         ** fields in the index. The second iteration always searches the entire
         ** index. The first iteration searches nStep entries starting with the
         ** current cursor entry if (nStep>=0), or the entire index if (nStep<0).  */
        while crate::btree::sqlite3BtreeCursorIsValidNN(pCur) != 0 {
            let mut ii = 0;
            while rc == SQLITE_OK && (ii < nStep || nStep < 0) {
                rc = vdbeIsMatchingIndexKey(pCur, bIntegrity, mask, p, &mut res);
                if res == 0 || rc != SQLITE_OK {
                    break;
                }
                rc = crate::btree::sqlite3BtreeNext(pCur, 0);
                ii += 1;
            }
            if rc == SQLITE_DONE {
                rc = SQLITE_OK;
                debug_assert!(res != 0);
            }
            if nStep < 0 || rc != SQLITE_OK || res == 0 || bIntegrity != 0 {
                break;
            }

            /* The first, non-exhaustive, search failed to find an entry with
             ** matching PK fields. So restart for an exhaustive search of the
             ** entire index.  */
            nStep = -1;
            rc = crate::btree::sqlite3BtreeFirst(db, pCur, &mut res);
        }
    }

    *pRes = res;
    rc
}

/// C: `int sqlite3NotPureFunc(sqlite3_context *pCtx)`
///
/// Cause a function to throw an error if it was call from OP_PureFunc
/// rather than OP_Function.
///
/// OP_PureFunc means that the function must be deterministic, and should
/// throw an error if it is given inputs that would make it
/// non-deterministic.  This routine is invoked by date/time functions that
/// use non-deterministic features such as 'now'.
pub fn sqlite3NotPureFunc(pCtx: &mut Context) -> i32 {
    let Some((opcode, p5)) = pCtx.pOpInfo else {
        return 1;
    };
    if opcode == OP_PureFunc {
        let zContext: &[u8] = if (p5 as i32) & NC_IsCheck != 0 {
            b"a CHECK constraint"
        } else if (p5 as i32) & NC_GenCol != 0 {
            b"a generated column"
        } else {
            b"an index"
        };
        let zMsg = crate::printf::sqlite3_mprintf(
            b"non-deterministic use of %s() in %s",
            &[
                PfArg::Str(Some(&pCtx.pFunc.zName)),
                PfArg::Str(Some(zContext)),
            ],
        );
        crate::vdbeapi::sqlite3_result_error(pCtx, Some(&zMsg), zMsg.len() as i32);
        return 0;
    }
    1
}

/// C: `int sqlite3CursorRangeHintExprCheck(Walker *pWalker, Expr *pExpr)`
/// SQLITE_ENABLE_CURSOR_HINTS && SQLITE_DEBUG only: always WRC_Continue.
pub fn sqlite3CursorRangeHintExprCheck(w: &mut Walker, pExpr: ExprId) -> i32 {
    WRC_Continue
}

/// C: `void sqlite3VtabImportErrmsg(Vdbe *p, sqlite3_vtab *pVtab)`
///
/// Transfer error message text from an sqlite3_vtab.zErrMsg into a
/// Vdbe.zErrMsg.  Virtual tables are not ported yet: no-op.
pub fn sqlite3VtabImportErrmsg(db: &mut Sqlite3, p: &mut Vdbe, pVtab: ()) {}

/// C: `void sqlite3VdbePreUpdateHook( Vdbe *v, VdbeCursor *pCsr, int op, const char *zDb, Table *pTab, i64 iKey1, int iReg, int iBlobWrite )`
/// SQLITE_ENABLE_PREUPDATE_HOOK is not compiled in: no-op.
pub fn sqlite3VdbePreUpdateHook(
    db: &mut Sqlite3,
    v: &mut Vdbe,
    pCsr: &mut VdbeCursor,
    op: i32,
    zDb: Option<&[u8]>,
    pTab: &TableRef,
    iKey1: i64,
    iReg: i32,
    iBlobWrite: i32,
) {
}

/// C: `const char * sqlite3VdbeFuncName(const sqlite3_context *pCtx)`
/// Return the name of an SQL function associated with the sqlite3_context.
pub fn sqlite3VdbeFuncName(pCtx: &Context) -> Option<ZStr> {
    Some(pCtx.pFunc.zName.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn putVarint(out: &mut Vec<u8>, v: u64) {
        /* simple encoder for values < 2^56 */
        let mut buf = Vec::new();
        let mut x = v;
        buf.push((x & 0x7f) as u8);
        x >>= 7;
        while x != 0 {
            buf.push(((x & 0x7f) as u8) | 0x80);
            x >>= 7;
        }
        buf.reverse();
        out.extend(buf);
    }

    /// Build a record (OP_MakeRecord format) from a list of Mems.
    fn makeRecord(vals: &mut [Mem]) -> Vec<u8> {
        let mut hdr = Vec::new();
        let mut body = Vec::new();
        for m in vals.iter_mut() {
            let mut len = 0;
            let st = serialType(m, 4, &mut len);
            putVarint(&mut hdr, st as u64);
            let mut buf = vec![0u8; len as usize];
            let n = sqlite3VdbeSerialPut(&mut buf, m, st);
            assert_eq!(n, len);
            body.extend(buf);
        }
        let mut szHdr = hdr.len() as u64 + 1;
        if szHdr >= 128 {
            szHdr += 1;
        }
        let mut rec = Vec::new();
        putVarint(&mut rec, szHdr);
        rec.extend(hdr);
        rec.extend(body);
        rec
    }

    fn mInt(i: i64) -> Mem {
        let mut m = newMem(MEM_Int);
        m.u.i = i;
        m
    }
    fn mReal(r: f64) -> Mem {
        let mut m = newMem(MEM_Real);
        m.u.r = r;
        m
    }
    fn mStr(s: &[u8]) -> Mem {
        let mut m = newMem(MEM_Str);
        m.z = s.to_vec();
        m.n = s.len() as i32;
        m.enc = SQLITE_UTF8;
        m
    }
    fn mBlob(s: &[u8]) -> Mem {
        let mut m = newMem(MEM_Blob);
        m.z = s.to_vec();
        m.n = s.len() as i32;
        m
    }
    fn mNull() -> Mem {
        newMem(MEM_Null)
    }

    fn coll(name: &[u8]) -> CollSeqRef {
        Rc::new(CollSeq {
            zName: name.to_vec(),
            enc: SQLITE_UTF8,
            xCmp: RefCell::new(None),
            xDel: RefCell::new(None),
        })
    }

    fn keyInfo(n: usize, sortFlags: &[u8]) -> KeyInfoRef {
        Rc::new(RefCell::new(KeyInfo {
            enc: SQLITE_UTF8,
            nKeyField: n as u16,
            nAllField: n as u16 + 1,
            aSortFlags: sortFlags.to_vec(),
            aColl: vec![None; n + 1],
        }))
    }

    fn unpacked(ki: &KeyInfoRef, vals: Vec<Mem>) -> UnpackedRecord {
        let mut p = *allocUnpackedRecord(ki);
        p.nField = vals.len() as u16;
        p.aMem = vals;
        p
    }

    #[test]
    fn serial_type_lengths() {
        let exp = [0, 1, 2, 3, 4, 6, 8, 8, 0, 0, 0, 0, 0, 0, 1, 1];
        for (i, &e) in exp.iter().enumerate() {
            assert_eq!(sqlite3VdbeSerialTypeLen(i as u32), e);
        }
        assert_eq!(sqlite3VdbeSerialTypeLen(200), 94);
        assert_eq!(sqlite3VdbeSerialTypeLen(201), 94);
        assert_eq!(sqlite3VdbeOneByteSerialTypeLen(127), 57);
    }

    #[test]
    fn serial_type_choice() {
        let mut l = 0;
        assert_eq!(serialType(&mut mNull(), 4, &mut l), 0);
        assert_eq!(serialType(&mut mInt(0), 4, &mut l), 8);
        assert_eq!(l, 0);
        assert_eq!(serialType(&mut mInt(1), 4, &mut l), 9);
        assert_eq!(serialType(&mut mInt(1), 1, &mut l), 1);
        assert_eq!(serialType(&mut mInt(-1), 4, &mut l), 1);
        assert_eq!(serialType(&mut mInt(127), 4, &mut l), 1);
        assert_eq!(serialType(&mut mInt(128), 4, &mut l), 2);
        assert_eq!(serialType(&mut mInt(-32768), 4, &mut l), 2);
        assert_eq!(serialType(&mut mInt(32768), 4, &mut l), 3);
        assert_eq!(serialType(&mut mInt(8388608), 4, &mut l), 4);
        assert_eq!(serialType(&mut mInt(2147483648), 4, &mut l), 5);
        assert_eq!(l, 6);
        assert_eq!(serialType(&mut mInt(i64::MAX), 4, &mut l), 6);
        assert_eq!(l, 8);
        assert_eq!(serialType(&mut mReal(1.5), 4, &mut l), 7);
        assert_eq!(serialType(&mut mStr(b"abc"), 4, &mut l), 19);
        assert_eq!(l, 3);
        assert_eq!(serialType(&mut mBlob(b"abc"), 4, &mut l), 18);
    }

    #[test]
    fn serial_roundtrip() {
        let ints = [
            0i64,
            1,
            -1,
            127,
            -128,
            300,
            -300,
            70000,
            -70000,
            1 << 30,
            -(1 << 31),
            1 << 40,
            -(1 << 46),
            i64::MAX,
            i64::MIN,
        ];
        for &i in &ints {
            let mut m = mInt(i);
            let mut l = 0;
            let st = serialType(&mut m, 4, &mut l);
            let mut buf = vec![0u8; l as usize];
            sqlite3VdbeSerialPut(&mut buf, &m, st);
            let mut out = Mem::default();
            sqlite3VdbeSerialGet(&buf, st, &mut out);
            assert_eq!(out.flags, MEM_Int);
            assert_eq!(out.u.i, i, "serial type {}", st);
        }
        for &r in &[0.5f64, -1e300, 3.141592653589793] {
            let mut m = mReal(r);
            let mut l = 0;
            let st = serialType(&mut m, 4, &mut l);
            let mut buf = vec![0u8; l as usize];
            sqlite3VdbeSerialPut(&mut buf, &m, st);
            assert_eq!(buf, r.to_bits().to_be_bytes());
            let mut out = Mem::default();
            sqlite3VdbeSerialGet(&buf, st, &mut out);
            assert_eq!(out.flags, MEM_Real);
            assert_eq!(out.u.r, r);
        }
        /* NaN reads as NULL */
        let mut out = Mem::default();
        sqlite3VdbeSerialGet(&f64::NAN.to_bits().to_be_bytes(), 7, &mut out);
        assert_eq!(out.flags, MEM_Null);
        /* text and blob */
        sqlite3VdbeSerialGet(b"hello", 12 + 2 * 5 + 1, &mut out);
        assert_eq!(out.flags, MEM_Str | MEM_Ephem);
        assert_eq!(out.bytes(), b"hello");
        sqlite3VdbeSerialGet(b"hel", 12 + 2 * 3, &mut out);
        assert_eq!(out.flags, MEM_Blob | MEM_Ephem);
        assert_eq!(out.bytes(), b"hel");
        sqlite3VdbeSerialGet(b"", 10, &mut out);
        assert_eq!(out.flags, MEM_Null | MEM_Zero);
    }

    #[test]
    fn int_float_compare() {
        assert_eq!(sqlite3IntFloatCompare(1, 1.0), 0);
        assert_eq!(sqlite3IntFloatCompare(1, 1.5), -1);
        assert_eq!(sqlite3IntFloatCompare(2, 1.5), 1);
        assert_eq!(sqlite3IntFloatCompare(i64::MAX, 9223372036854775808.0), -1);
        assert_eq!(sqlite3IntFloatCompare(i64::MIN, -1e300), 1);
        assert_eq!(sqlite3IntFloatCompare(0, f64::NAN), 1);
        assert_eq!(
            sqlite3IntFloatCompare(9007199254740993, 9007199254740992.0),
            1
        );
    }

    #[test]
    fn mem_compare() {
        assert_eq!(memCompare(&mNull(), &mNull(), None), 0);
        assert!(memCompare(&mNull(), &mInt(1), None) < 0);
        assert!(memCompare(&mInt(1), &mNull(), None) > 0);
        assert!(memCompare(&mInt(1), &mInt(2), None) < 0);
        assert!(memCompare(&mInt(2), &mReal(1.5), None) > 0);
        assert!(memCompare(&mReal(1.5), &mInt(2), None) < 0);
        assert_eq!(memCompare(&mReal(2.0), &mInt(2), None), 0);
        assert!(memCompare(&mInt(100), &mStr(b"1"), None) < 0);
        assert!(memCompare(&mStr(b"a"), &mBlob(b"a"), None) < 0);
        assert!(memCompare(&mBlob(b"a"), &mStr(b"a"), None) > 0);
        assert!(memCompare(&mStr(b"abc"), &mStr(b"abd"), None) < 0);
        assert!(memCompare(&mStr(b"ab"), &mStr(b"abc"), None) < 0);
        assert!(memCompare(&mBlob(b"\x01"), &mBlob(b"\x00\x01"), None) > 0);
        /* collation: case-insensitive closure */
        let c = coll(b"NOCASE");
        *c.xCmp.borrow_mut() = Some(Rc::new(|a: &[u8], b: &[u8]| {
            binCmp(&a.to_ascii_lowercase(), &b.to_ascii_lowercase())
        }));
        assert_eq!(memCompare(&mStr(b"ABC"), &mStr(b"abc"), Some(&c)), 0);
        assert!(memCompare(&mStr(b"ABC"), &mStr(b"abc"), None) < 0);
        /* zero blobs */
        let mut z = newMem(MEM_Blob | MEM_Zero);
        z.u.nZero = 3;
        assert_eq!(sqlite3BlobCompare(&z, &mBlob(b"\0\0\0")), 0);
        assert!(sqlite3BlobCompare(&z, &mBlob(b"\0\x01")) < 0);
    }

    #[test]
    fn record_compare() {
        let ki = keyInfo(2, &[0, 0]);
        let rec = makeRecord(&mut [mInt(5), mStr(b"hello"), mInt(42)]);

        /* equal prefix -> default_rc */
        let mut p = unpacked(&ki, vec![mInt(5), mStr(b"hello")]);
        p.default_rc = 0;
        assert_eq!(sqlite3VdbeRecordCompare(rec.len() as i32, &rec, &mut p), 0);
        assert_eq!(p.eqSeen, 1);
        p.default_rc = -1;
        assert_eq!(sqlite3VdbeRecordCompare(rec.len() as i32, &rec, &mut p), -1);

        let cases: Vec<(Vec<Mem>, i32)> = vec![
            (vec![mInt(4)], 1),
            (vec![mInt(6)], -1),
            (vec![mReal(5.5)], -1),
            (vec![mReal(4.5)], 1),
            (vec![mNull()], 1),
            (vec![mStr(b"x")], -1),
            (vec![mInt(5), mStr(b"hellp")], -1),
            (vec![mInt(5), mStr(b"hell")], 1),
            (vec![mInt(5), mInt(1000)], 1),
            (vec![mInt(5), mBlob(b"a")], -1),
        ];
        for (vals, exp) in cases {
            let mut p = unpacked(&ki, vals.clone());
            let r1 = sqlite3VdbeRecordCompare(rec.len() as i32, &rec, &mut p);
            assert_eq!(r1.signum(), exp);
            /* the fast path chosen by FindCompare must agree */
            let mut p2 = unpacked(&ki, vals);
            let f = sqlite3VdbeFindCompare(&mut p2);
            assert_eq!(f(&rec, &mut p2).signum(), exp);
        }

        /* DESC first column inverts the result */
        let kid = keyInfo(2, &[KEYINFO_ORDER_DESC, 0]);
        let mut p = unpacked(&kid, vec![mInt(4)]);
        assert_eq!(sqlite3VdbeRecordCompare(rec.len() as i32, &rec, &mut p), -1);
        let mut p = unpacked(&kid, vec![mInt(4)]);
        let f = sqlite3VdbeFindCompare(&mut p);
        assert_eq!(f(&rec, &mut p), -1);

        /* string fast path */
        let rs = makeRecord(&mut [mStr(b"abc"), mInt(1)]);
        let ki1 = keyInfo(1, &[0]);
        for (s, exp) in [
            (&b"abc"[..], 0),
            (b"abd", -1),
            (b"abb", 1),
            (b"ab", 1),
            (b"abcd", -1),
        ] {
            let mut p = unpacked(&ki1, vec![mStr(s)]);
            let f = sqlite3VdbeFindCompare(&mut p);
            assert_eq!(f(&rs, &mut p).signum(), exp, "{:?}", s);
        }

        /* NULLS FIRST/LAST (BIGNULL) */
        let rn = makeRecord(&mut [mNull()]);
        let kin = keyInfo(1, &[KEYINFO_ORDER_BIGNULL]);
        let mut p = unpacked(&kin, vec![mInt(1)]);
        assert_eq!(sqlite3VdbeRecordCompare(rn.len() as i32, &rn, &mut p), 1);

        /* corrupt record */
        let mut p = unpacked(&ki, vec![mInt(1)]);
        assert_eq!(sqlite3VdbeRecordCompare(1, &[5u8], &mut p), 0);
        assert_eq!(p.errCode as i32, SQLITE_CORRUPT);
    }

    #[test]
    fn record_unpack() {
        let ki = keyInfo(3, &[0, 0, 0]);
        let rec = makeRecord(&mut [mInt(-7), mStr(b"xy"), mReal(2.5), mBlob(b"\x01\x02")]);
        let mut p = allocUnpackedRecord(&ki);
        recordUnpack(rec.len() as i32, &rec, &mut p);
        assert_eq!(p.nField, 4);
        assert_eq!(p.aMem[0].u.i, -7);
        assert_eq!(p.aMem[1].bytes(), b"xy");
        assert_eq!(p.aMem[2].u.r, 2.5);
        assert_eq!(p.aMem[3].bytes(), b"\x01\x02");
        /* record compares equal to its own unpacking */
        p.default_rc = 0;
        assert_eq!(sqlite3VdbeRecordCompare(rec.len() as i32, &rec, &mut p), 0);
    }

    fn op(p4type: i8, p4: P4) -> VdbeOp {
        VdbeOp {
            p4type,
            p4,
            ..Default::default()
        }
    }

    fn s(z: Option<ZStr>) -> Option<String> {
        z.map(|v| String::from_utf8(v).unwrap())
    }

    #[test]
    fn display_p4() {
        let ki = Rc::new(RefCell::new(KeyInfo {
            enc: SQLITE_UTF8,
            nKeyField: 3,
            nAllField: 4,
            aSortFlags: vec![KEYINFO_ORDER_DESC, 0, KEYINFO_ORDER_BIGNULL, 0],
            aColl: vec![Some(coll(b"BINARY")), Some(coll(b"NOCASE")), None, None],
        }));
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(P4_KEYINFO, P4::KeyInfo(ki)))).unwrap(),
            "k(3,-B,NOCASE,N.)"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(P4_INT32, P4::I(-42)))).unwrap(),
            "-42"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_COLLSEQ,
                P4::Coll(coll(b"RTRIM"))
            )))
            .unwrap(),
            "RTRIM-8"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_INTARRAY,
                P4::Ai(Rc::new(vec![3, 1, 20, 7]))
            )))
            .unwrap(),
            "[1,20,7]"
        );
        let f = Rc::new(FuncDef {
            nArg: 2,
            funcFlags: 0,
            pUserData: FuncUserData::None,
            xSFunc: None,
            xFinalize: None,
            xValue: None,
            xInverse: None,
            zName: b"substr".to_vec(),
            pDestructor: None,
        });
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(P4_FUNCDEF, P4::Func(f.clone())))).unwrap(),
            "substr(2)"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(P4_FUNCCTX, P4::FuncCtx(f, 2)))).unwrap(),
            "substr(2)"
        );
        /* (P4_MEM reals use "%.16g", which needs util.rs sqlite3FpDecode) */
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_MEM,
                P4::Mem(Box::new(mInt(-5)))
            )))
            .unwrap(),
            "-5"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_MEM,
                P4::Mem(Box::new(mNull()))
            )))
            .unwrap(),
            "NULL"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_MEM,
                P4::Mem(Box::new(mBlob(b"x")))
            )))
            .unwrap(),
            "(blob)"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_SUBPROGRAM,
                P4::Program(Rc::new(SubProgram::default()))
            )))
            .unwrap(),
            "program"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_DYNAMIC,
                P4::Z(b"a'b".to_vec())
            )))
            .unwrap(),
            "a'b"
        );
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_DYNAMIC,
                P4::Z(b"\0zz".to_vec())
            )))
            .unwrap(),
            ""
        );
        assert_eq!(sqlite3VdbeDisplayP4(&op(P4_NOTUSED, P4::None)), None);
        let sig = SubrtnSig {
            selId: 3,
            zAff: Some(b"DB".to_vec()),
            ..Default::default()
        };
        assert_eq!(
            s(sqlite3VdbeDisplayP4(&op(
                P4_SUBRTNSIG,
                P4::SubrtnSig(Box::new(sig))
            )))
            .unwrap(),
            "subrtnsig:3,DB"
        );
    }

    #[test]
    fn builder() {
        let mut v = Vdbe::default();
        sqlite3VdbeAddOp2(&mut v, OP_Init, 0, 1);
        let lbl = -1; /* sqlite3VdbeMakeLabel() */
        let a1 = sqlite3VdbeAddOp2(&mut v, OP_Goto, 0, lbl);
        assert_eq!(a1, 1);
        sqlite3VdbeAddOp4(&mut v, OP_String8, 0, 1, 0, P4::Z(b"abc\0def".to_vec()), 0);
        assert_eq!(v.aOp[2].p4type, P4_DYNAMIC);
        assert!(matches!(&v.aOp[2].p4, P4::Z(z) if z == b"abc"));
        sqlite3VdbeAddOp4(
            &mut v,
            OP_String8,
            0,
            1,
            0,
            P4::Z(b"stat".to_vec()),
            P4_STATIC as i32,
        );
        assert_eq!(v.aOp[3].p4type, P4_STATIC);
        sqlite3VdbeChangeP4(&mut v, -1, P4::Z(b"dyn".to_vec()), P4_DYNAMIC as i32);
        assert_eq!(v.aOp[3].p4type, P4_DYNAMIC);
        sqlite3VdbeAddOp4Int(&mut v, OP_Integer, 1, 2, 3, 4);
        assert!(matches!(v.aOp[4].p4, P4::I(4)));
        sqlite3VdbeResolveLabel(&mut v, lbl);
        sqlite3VdbeAddOp0(&mut v, OP_Halt);
        let addr = sqlite3VdbeAddOp1(&mut v, OP_Once, 0);
        sqlite3VdbeJumpHereOrPopInst(&mut v, addr);
        assert_eq!(v.aOp.len(), 6);
        let list = [
            VdbeOpList {
                opcode: OP_Goto,
                p1: 0,
                p2: 1,
                p3: 0,
            },
            VdbeOpList {
                opcode: OP_Noop,
                p1: 0,
                p2: 0,
                p3: 0,
            },
        ];
        let first = sqlite3VdbeAddOpList(&mut v, &list, 0);
        assert_eq!(first, 6);
        assert_eq!(v.aOp[6].p2, 7);
        assert_eq!(sqlite3VdbeDeletePriorOpcode(&mut v, OP_Noop), 1);
        assert_eq!(v.aOp[7].opcode, OP_Noop);
        let mut nArg = 0;
        let ops = sqlite3VdbeTakeOpArray(&mut v, &mut nArg);
        assert_eq!(ops[1].p2, 5);
        assert!(v.aLabel.is_empty());
    }
}
