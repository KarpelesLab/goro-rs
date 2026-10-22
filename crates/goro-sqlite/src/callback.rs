//! Port of callback.c.
//!
//! This file contains functions used to access the internal hash tables
//! of user defined functions and collation sequences.

use crate::global::sqlite3UpperToLower;
use crate::int::*;
// sqlite3StrICmp() folds ASCII case only, like eq_ignore_ascii_case().
use std::rc::Rc;

/// The hash table of built-in SQL functions (C `FuncDefHash`, the global
/// `sqlite3BuiltinFunctions`).
///
/// C keeps SQLITE_FUNC_HASH_SZ buckets; each bucket is a list (through
/// `FuncDef.u.pHash`) of the first FuncDef of every distinct name, and
/// each of those heads a list (through `FuncDef.pNext`) of the overloads
/// of that name.  The Rust structure mirrors this exactly so that the
/// iteration order (PRAGMA function_list) is the same:
/// `a[h]` is the u.pHash chain of bucket `h` (index 0 = head) and each
/// entry of it is the pNext chain of one name (index 0 = head).
pub struct FuncDefHash {
    pub a: Vec<Vec<Vec<FuncDefRef>>>,
}

impl Default for FuncDefHash {
    fn default() -> Self {
        FuncDefHash::new()
    }
}

impl FuncDefHash {
    pub fn new() -> FuncDefHash {
        FuncDefHash {
            a: (0..SQLITE_FUNC_HASH_SZ).map(|_| Vec::new()).collect(),
        }
    }

    /// The pNext chain of the built-in functions named zFunc in bucket h
    /// (the chain whose head sqlite3FunctionSearch() returns).
    pub fn chain(&self, h: i32, zFunc: &[u8]) -> Option<&[FuncDefRef]> {
        for c in self.a[h as usize].iter() {
            debug_assert!((c[0].funcFlags & SQLITE_FUNC_BUILTIN) != 0);
            if c[0].zName.eq_ignore_ascii_case(zFunc) {
                return Some(&c[..]);
            }
        }
        None
    }

    /// Every built-in function in the order of C's
    /// `for(i..SQLITE_FUNC_HASH_SZ) for(p=a[i]; p; p=p->u.pHash)
    /// for(; p; p=p->pNext)` loops (PRAGMA function_list).
    pub fn all(&self) -> Vec<FuncDefRef> {
        let mut v = Vec::new();
        for b in self.a.iter() {
            for c in b.iter() {
                v.extend(c.iter().cloned());
            }
        }
        v
    }
}

/// SQLITE_FUNC_HASH(C,L)
#[inline]
pub fn SQLITE_FUNC_HASH(c: u8, l: i32) -> i32 {
    ((c as i32) + l) % (SQLITE_FUNC_HASH_SZ as i32)
}

/// Invoke the 'collation needed' callback to request a collation sequence
/// in the encoding enc of name zName, length nName.
fn callCollNeeded(db: &mut Sqlite3, enc: i32, zName: &[u8]) {
    if let Some(x) = db.xCollNeeded.clone() {
        let zExternal = zName.to_vec();
        x(db, enc, &zExternal);
    }
    // UTF-16 xCollNeeded16 callbacks are not supported (UTF-8 only port).
}

/// This routine is called if the collation factory fails to deliver a
/// collation function in the best encoding but there may be other versions
/// of this collation function (for other text encodings) available. Use one
/// of these instead if they exist. Avoid a UTF-8 <-> UTF-16 conversion if
/// possible.
fn synthCollSeq(db: &mut Sqlite3, pColl: &CollSeqRef) -> i32 {
    let z = pColl.zName.clone();
    const aEnc: [u8; 3] = [SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF8];
    for &e in aEnc.iter() {
        let pColl2 = sqlite3FindCollSeq(db, e, Some(&z), 0);
        if let Some(p2) = pColl2 {
            let x = p2.xCmp.borrow().clone();
            if x.is_some() {
                // C copies the whole CollSeq (name, enc, xCmp) except the
                // destructor.
                *pColl.xCmp.borrow_mut() = x;
                pColl.enc.set(p2.enc.get());
                return SQLITE_OK;
            }
        }
    }
    SQLITE_ERROR
}

/// C: `int sqlite3CheckCollSeq(Parse *pParse, CollSeq *pColl)`
///
/// This routine is called on a collation sequence before it is used to
/// check that it is defined. An undefined collation sequence exists when
/// a database is loaded that contains references to collation sequences
/// that have not been defined by sqlite3_create_collation() etc.
///
/// If required, this routine calls the 'collation needed' callback to
/// request a definition of the collating sequence. If this doesn't work,
/// an equivalent collating sequence that uses a text encoding different
/// from the main database is substituted, if one is available.
///
/// Rust: pColl may be None (C NULL).
pub fn sqlite3CheckCollSeq(db: &mut Sqlite3, pColl: Option<&CollSeqRef>) -> i32 {
    if let Some(pColl) = pColl {
        if pColl.xCmp.borrow().is_none() {
            let zName = pColl.zName.clone();
            let enc = db.ENC();
            let p = sqlite3GetCollSeq(db, enc, Some(pColl), Some(&zName));
            match p {
                None => return SQLITE_ERROR,
                Some(p) => debug_assert!(Rc::ptr_eq(&p, pColl)),
            }
        }
    }
    SQLITE_OK
}

/// Locate and return an entry from the db.aCollSeq hash table. If the entry
/// specified by zName and nName is not found and parameter 'create' is
/// true, then create a new entry. Otherwise return NULL.
///
/// Each pointer stored in the sqlite3.aCollSeq hash table contains an
/// array of three CollSeq structures. The first is the collation sequence
/// preferred for UTF-8, the second UTF-16le, and the third UTF-16be.
fn findCollSeqEntry(db: &mut Sqlite3, zName: &[u8], create: i32) -> Option<[CollSeqRef; 3]> {
    if let Some(p) = db.aCollSeq.find(zName) {
        return Some(p.clone());
    }
    if create != 0 {
        let mk = |enc: u8| {
            Rc::new(CollSeq {
                zName: zName.to_vec(),
                enc: std::cell::Cell::new(enc),
                xCmp: std::cell::RefCell::new(None),
                xDel: std::cell::RefCell::new(None),
            })
        };
        let pColl = [mk(SQLITE_UTF8), mk(SQLITE_UTF16LE), mk(SQLITE_UTF16BE)];
        db.aCollSeq.insert(zName, pColl.clone());
        return Some(pColl);
    }
    None
}

/// C: `CollSeq * sqlite3FindCollSeq( sqlite3 *db, u8 enc, const char *zName, int create )`
///
/// Parameter zName points to a UTF-8 encoded string nName bytes long.
/// Return the CollSeq* pointer for the collation sequence named zName
/// for the encoding 'enc' from the database 'db'.
///
/// If the entry specified is not found and 'create' is true, then create a
/// new entry.  Otherwise return NULL.
///
/// A separate function sqlite3LocateCollSeq() is a wrapper around
/// this routine.  sqlite3LocateCollSeq() invokes the collation factory
/// if necessary and generates an error message if the collating sequence
/// cannot be found.
pub fn sqlite3FindCollSeq(
    db: &mut Sqlite3,
    enc: u8,
    zName: Option<&[u8]>,
    create: i32,
) -> Option<CollSeqRef> {
    debug_assert!(enc >= SQLITE_UTF8 && enc <= SQLITE_UTF16BE);
    match zName {
        Some(z) => findCollSeqEntry(db, z, create).map(|a| a[(enc - 1) as usize].clone()),
        None => db.pDfltColl.clone(),
    }
}

/// C: `void sqlite3SetTextEncoding(sqlite3 *db, u8 enc)`
///
/// Change the text encoding for a database connection. This means that
/// the pDfltColl must change as well.
pub fn sqlite3SetTextEncoding(db: &mut Sqlite3, enc: u8) {
    debug_assert!(enc == SQLITE_UTF8 || enc == SQLITE_UTF16LE || enc == SQLITE_UTF16BE);
    db.enc = enc;
    // EVIDENCE-OF: R-08308-17224 The default collating function for all
    // strings is BINARY.
    db.pDfltColl = sqlite3FindCollSeq(db, enc, Some(crate::global::sqlite3StrBINARY), 0);
    crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 1);
}

/// C: `CollSeq * sqlite3GetCollSeq( Parse *pParse, u8 enc, CollSeq *pColl, const char *zName )`
///
/// This function is responsible for invoking the collation factory callback
/// or substituting a collation sequence of a different encoding when the
/// requested collation sequence is not available in the desired encoding.
///
/// If it is not NULL, then pColl must point to the database native encoding
/// collation sequence with name zName, length nName.
///
/// The return value is either the collation sequence to be used in database
/// db for collation type name zName, length nName, or NULL, if no collation
/// sequence can be found.  If no collation is found, leave an error message.
///
/// Rust: pColl may be None (C NULL).
pub fn sqlite3GetCollSeq(
    db: &mut Sqlite3,
    enc: u8,
    pColl: Option<&CollSeqRef>,
    zName: Option<&[u8]>,
) -> Option<CollSeqRef> {
    let mut p = pColl.cloned();
    if p.is_none() {
        p = sqlite3FindCollSeq(db, enc, zName, 0);
    }
    if p.as_ref().map_or(true, |c| c.xCmp.borrow().is_none()) {
        // No collation sequence of this type for this encoding is registered.
        // Call the collation factory to see if it can supply us with one.
        callCollNeeded(db, enc as i32, zName.unwrap_or(b""));
        p = sqlite3FindCollSeq(db, enc, zName, 0);
    }
    if let Some(c) = p.clone() {
        if c.xCmp.borrow().is_none() && synthCollSeq(db, &c) != 0 {
            p = None;
        }
    }
    debug_assert!(p.as_ref().map_or(true, |c| c.xCmp.borrow().is_some()));
    if p.is_none() {
        errmsg!(db, "no such collation sequence: %s", zName);
        db.pp().rc = SQLITE_ERROR_MISSING_COLLSEQ;
    }
    p
}

/// C: `CollSeq * sqlite3LocateCollSeq(Parse *pParse, const char *zName)`
///
/// This function returns the collation sequence for database native text
/// encoding identified by the string zName.
///
/// If the requested collation sequence is not available, or not available
/// in the database native encoding, the collation factory is invoked to
/// request it. If the collation factory does not supply such a sequence,
/// and the sequence is available in another text encoding, then that is
/// returned instead.
///
/// If no versions of the requested collations sequence are available, or
/// another error occurs, NULL is returned and an error message written into
/// pParse.
///
/// This routine is a wrapper around sqlite3FindCollSeq().  This routine
/// invokes the collation factory if the named collation cannot be found
/// and generates an error message.
///
/// See also: sqlite3FindCollSeq(), sqlite3GetCollSeq()
pub fn sqlite3LocateCollSeq(db: &mut Sqlite3, zName: Option<&[u8]>) -> Option<CollSeqRef> {
    let enc = db.ENC();
    let initbusy = db.init.busy;
    let mut pColl = sqlite3FindCollSeq(db, enc, zName, initbusy as i32);
    if initbusy == 0 && pColl.as_ref().map_or(true, |c| c.xCmp.borrow().is_none()) {
        let pc = pColl.clone();
        pColl = sqlite3GetCollSeq(db, enc, pc.as_ref(), zName);
    }
    pColl
}

/// The score for a perfect match
const FUNC_PERFECT_MATCH: i32 = 6;

/// During the search for the best function definition, this procedure
/// is called to test how well the function passed as the first argument
/// matches the request for a function with nArg arguments in a system
/// that uses encoding enc. The value returned indicates how well the
/// request is matched. A higher value indicates a better match.
///
/// If nArg is -1 that means to only return a match (non-zero) if p->nArg
/// is also -1.  In other words, we are searching for a function that
/// takes a variable number of arguments.
///
/// If nArg is -2 that means that we are searching for any function
/// regardless of the number of arguments it uses, so return a positive
/// match score for any
///
/// The returned value is always between 0 and 6, as follows:
///
/// 0: Not a match.
/// 1: UTF8/16 conversion required and function takes any number of arguments.
/// 2: UTF16 byte order change required and function takes any number of args.
/// 3: encoding matches and function takes any number of arguments
/// 4: UTF8/16 conversion required - argument count matches exactly
/// 5: UTF16 byte order conversion required - argument count matches exactly
/// 6: Perfect match:  encoding and argument count match exactly.
///
/// If nArg==(-2) then any function with a non-null xSFunc is
/// a perfect match and any function with xSFunc NULL is
/// a non-match.
fn matchQuality(p: &FuncDef, nArg: i32, enc: u8) -> i32 {
    let mut m;
    debug_assert!(p.nArg >= -4 && p.nArg != -2);
    debug_assert!(nArg >= -2);
    let pn = p.nArg as i32;

    // Wrong number of arguments means "no match"
    if pn != nArg {
        if nArg == -2 {
            return if p.xSFunc.is_none() {
                0
            } else {
                FUNC_PERFECT_MATCH
            };
        }
        if pn >= 0 {
            return 0;
        }
        // Special p->nArg values available to built-in functions only:
        //    -3     1 or more arguments required
        //    -4     2 or more arguments required
        if pn < -2 && nArg < (-2 - pn) {
            return 0;
        }
    }

    // Give a better score to a function with a specific number of arguments
    // than to function that accepts any number of arguments.
    if pn == nArg {
        m = 4;
    } else {
        m = 1;
    }

    // Bonus points if the text encoding matches
    if enc as u32 == (p.funcFlags & SQLITE_FUNC_ENCMASK) {
        m += 2; // Exact encoding match
    } else if ((enc as u32) & p.funcFlags & 2) != 0 {
        m += 1; // Both are UTF16, but with different byte orders
    }

    m
}

/// C: `FuncDef * sqlite3FunctionSearch( int h, const char *zFunc )`
///
/// Search a FuncDefHash for a function with the given name.  Return
/// a pointer to the matching FuncDef if found, or 0 if there is no match.
///
/// Rust: the hash table (C global sqlite3BuiltinFunctions) is passed
/// explicitly; use FuncDefHash::chain() to get the whole pNext chain.
pub fn sqlite3FunctionSearch(pHash: &FuncDefHash, h: i32, zFunc: &[u8]) -> Option<FuncDefRef> {
    pHash.chain(h, zFunc).map(|c| c[0].clone())
}

/// C: `void sqlite3InsertBuiltinFuncs( FuncDef *aDef, int nDef )`
///
/// Insert a new FuncDef into a FuncDefHash hash table.
///
/// Rust: the hash table is passed explicitly and the FuncDefs are copied
/// into reference-counted objects.
pub fn sqlite3InsertBuiltinFuncs(pHash: &mut FuncDefHash, aDef: &[FuncDef]) {
    for d in aDef.iter() {
        let zName = &d.zName;
        let nName = zName.len() as i32;
        let h = SQLITE_FUNC_HASH(zName.first().copied().unwrap_or(0), nName);
        debug_assert!((d.funcFlags & SQLITE_FUNC_BUILTIN) != 0);
        let def = Rc::new(d.clone());
        let bucket = &mut pHash.a[h as usize];
        let mut found = false;
        for c in bucket.iter_mut() {
            if c[0].zName.eq_ignore_ascii_case(zName) {
                // aDef[i].pNext = pOther->pNext; pOther->pNext = &aDef[i];
                c.insert(1, def.clone());
                found = true;
                break;
            }
        }
        if !found {
            // aDef[i].u.pHash = sqlite3BuiltinFunctions.a[h];
            // sqlite3BuiltinFunctions.a[h] = &aDef[i];
            bucket.insert(0, vec![def]);
        }
    }
}

/// C: `FuncDef * sqlite3FindFunction( sqlite3 *db, const char *zName, int nArg, u8 enc, u8 createFlag )`
///
/// Locate a user function given a name, a number of arguments and a flag
/// indicating whether the function prefers UTF-16 over UTF-8.  Return a
/// pointer to the FuncDef structure that defines that function, or return
/// NULL if the function does not exist.
///
/// If the createFlag argument is true, then a new (blank) FuncDef
/// structure is created and liked into the "db" structure if a
/// no matching function previously existed.
///
/// If nArg is -2, then the first valid function found is returned.  A
/// function is valid if xSFunc is non-zero.  The nArg==(-2)
/// case is used to see if zName is a valid function name for some number
/// of arguments.  If nArg is -2, then createFlag must be 0.
///
/// If createFlag is false, then a function with the required name and
/// number of arguments may be returned even if the eTextRep flag does not
/// match that requested.
///
/// Rust: FuncDefs are immutable; a caller that wants to fill in a created
/// (or existing) application function must build a new FuncDef and install
/// it with sqlite3FuncDefReplace().
pub fn sqlite3FindFunction(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    nArg: i32,
    enc: u8,
    createFlag: u8,
) -> Option<FuncDefRef> {
    let zName = zName.unwrap_or(b"");
    let mut pBest: Option<FuncDefRef> = None;
    let mut bestScore = 0;
    debug_assert!(nArg >= -2);
    debug_assert!(nArg >= -1 || createFlag == 0);
    let nName = zName.len() as i32;

    // First search for a match amongst the application-defined functions.
    if let Some(chain) = db.aFunc.find(zName) {
        for p in chain.iter() {
            let score = matchQuality(p, nArg, enc);
            if score > bestScore {
                pBest = Some(p.clone());
                bestScore = score;
            }
        }
    }

    // If no match is found, search the built-in functions.
    //
    // If the DBFLAG_PreferBuiltin flag is set, then search the built-in
    // functions even if a prior app-defined function was found.  And give
    // priority to built-in functions.
    //
    // Except, if createFlag is true, that means that we are trying to
    // install a new function.  Whatever FuncDef structure is returned it will
    // have fields overwritten with new information appropriate for the
    // new function.  But the FuncDefs for built-in functions are read-only.
    // So we must not search for built-ins when creating a new function.
    if createFlag == 0 && (pBest.is_none() || (db.mDbFlags & DBFLAG_PreferBuiltin) != 0) {
        bestScore = 0;
        let c0 = sqlite3UpperToLower[zName.first().copied().unwrap_or(0) as usize];
        let h = SQLITE_FUNC_HASH(c0, nName);
        let builtins = db.builtinFuncs.clone();
        if let Some(chain) = builtins.chain(h, zName) {
            for p in chain.iter() {
                let score = matchQuality(p, nArg, enc);
                if score > bestScore {
                    pBest = Some(p.clone());
                    bestScore = score;
                }
            }
        }
    }

    // If the createFlag parameter is true and the search did not reveal an
    // exact match for the name, number of arguments and encoding, then add a
    // new entry to the hash table and return it.
    if createFlag != 0 && bestScore < FUNC_PERFECT_MATCH {
        let lname: ZStr = zName
            .iter()
            .map(|&c| sqlite3UpperToLower[c as usize])
            .collect();
        let p = Rc::new(FuncDef {
            nArg: nArg as u16 as i16,
            funcFlags: enc as u32,
            pUserData: FuncUserData::None,
            xSFunc: None,
            xFinalize: None,
            xValue: None,
            xInverse: None,
            zName: lname.clone(),
            pDestructor: None,
        });
        // pBest->pNext = pOther (the new entry becomes the head of the chain)
        if let Some(chain) = db.aFunc.find_mut(&lname) {
            chain.insert(0, p.clone());
        } else {
            db.aFunc.insert(&lname, vec![p.clone()]);
        }
        pBest = Some(p);
    }

    if let Some(p) = pBest {
        if p.xSFunc.is_some() || createFlag != 0 {
            return Some(p);
        }
    }
    None
}

/// Rust helper: replace the application-defined function `pOld` (as
/// returned by sqlite3FindFunction(..., createFlag=1)) by `pNew` in
/// db.aFunc, keeping its position in the chain.  This is the Rust
/// equivalent of C code that overwrites the fields of the FuncDef returned
/// by sqlite3FindFunction().  Returns the installed FuncDef.
pub fn sqlite3FuncDefReplace(db: &mut Sqlite3, pOld: &FuncDefRef, pNew: FuncDef) -> FuncDefRef {
    let p = Rc::new(pNew);
    let key = pOld.zName.clone();
    if let Some(chain) = db.aFunc.find_mut(&key) {
        for f in chain.iter_mut() {
            if Rc::ptr_eq(f, pOld) {
                *f = p.clone();
                return p;
            }
        }
        chain.insert(0, p.clone());
    } else {
        db.aFunc.insert(&key, vec![p.clone()]);
    }
    p
}

/// C: `void sqlite3SchemaClear(void *p)`
///
/// Free all resources held by the schema structure. The void* argument
/// points at a Schema struct. This function does not call sqlite3DbFree(db, )
/// on the pointer itself, it just cleans up subsidiary resources (i.e. the
/// contents of the schema hash tables).
///
/// The Schema.cache_size variable is not cleared.
///
/// Rust: the schema is db.aDb[iDb].pSchema.
pub fn sqlite3SchemaClear(db: &mut Sqlite3, iDb: i32) {
    let i = iDb as usize;
    let temp1 = std::mem::take(&mut db.aDb[i].pSchema.tblHash);
    let temp2 = std::mem::take(&mut db.aDb[i].pSchema.trigHash);
    db.aDb[i].pSchema.idxHash.clear();
    for pTrig in temp2.values() {
        crate::trigger::sqlite3DeleteTrigger(db, &pTrig);
    }
    drop(temp2);
    for pTab in temp1.values() {
        crate::build::sqlite3DeleteTable(db, &pTab);
    }
    drop(temp1);
    let s = &mut db.aDb[i].pSchema;
    s.fkeyHash.clear();
    s.pSeqTab = None;
    if (s.schemaFlags & DB_SchemaLoaded) != 0 {
        s.iGeneration += 1;
    }
    s.schemaFlags &= !(DB_SchemaLoaded | DB_ResetWanted);
}

/// C: `Schema * sqlite3SchemaGet(sqlite3 *db, Btree *pBt)`
///
/// Find and return the schema associated with a BTree.  Create
/// a new one if necessary.
///
/// Rust: without shared cache every Btree has its own schema, so this
/// always returns a fresh (empty) Schema with a new SchemaId (ids start at
/// 1; SchemaId(0) is never handed out).
pub fn sqlite3SchemaGet(db: &mut Sqlite3, pBt: Option<&BtreeP>) -> Schema {
    let _ = pBt;
    db.nextSchemaId += 1;
    Schema {
        id: SchemaId(db.nextSchemaId),
        enc: SQLITE_UTF8,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fd(name: &[u8], nArg: i16) -> FuncDef {
        FuncDef {
            nArg,
            funcFlags: SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32,
            pUserData: FuncUserData::None,
            xSFunc: None,
            xFinalize: None,
            xValue: None,
            xInverse: None,
            zName: name.to_vec(),
            pDestructor: None,
        }
    }

    #[test]
    fn test_builtin_hash_order() {
        let mut h = FuncDefHash::new();
        sqlite3InsertBuiltinFuncs(&mut h, &[fd(b"abc", 1), fd(b"abc", 2), fd(b"abc", 3)]);
        let hh = SQLITE_FUNC_HASH(b'a', 3);
        assert_eq!(h.a[hh as usize].len(), 1);
        let c = &h.a[hh as usize][0];
        // the first one inserted is the head, the others follow it in
        // reverse order of insertion (C inserts after the head)
        assert_eq!(c.iter().map(|f| f.nArg).collect::<Vec<_>>(), vec![1, 3, 2]);
    }

    #[test]
    fn test_match_quality() {
        let f = fd(b"x", 2);
        assert_eq!(matchQuality(&f, 2, SQLITE_UTF8), 6);
        assert_eq!(matchQuality(&f, 1, SQLITE_UTF8), 0);
        let g = fd(b"x", -1);
        assert_eq!(matchQuality(&g, 3, SQLITE_UTF8), 3);
        let k = fd(b"x", -3);
        assert_eq!(matchQuality(&k, 0, SQLITE_UTF8), 0);
        assert_eq!(matchQuality(&k, 1, SQLITE_UTF16LE), 1);
    }
}
