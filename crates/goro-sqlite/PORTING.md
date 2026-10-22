# goro-sqlite porting guide

goro-sqlite is a **faithful, module-by-module port of SQLite 3.54.1** to
safe Rust.  The goal is behavioural identity with the C library: same SQL
dialect, same query plans, same VDBE programs (EXPLAIN output), same error
codes and messages, same file format.  When in doubt, do what the C code
does.

Reference sources (read-only, do not copy them into the repo):

* C sources: `/tmp/claude-1000/-home-magicaltux-projects-goro-rs/3d2a04d8-60d8-47cf-8c66-cf536b5391ca/scratchpad/sqlite/dl/sqlite-src-3540100/src/`
* generated files (parse.c, opcodes.h, keywordhash.h, pragma.h, ...): `.../scratchpad/sqlite/dl/bld/`
* amalgamation: `.../scratchpad/sqlite/dl/sqlite-amalgamation-3540100/sqlite3.c`
* reference CLI built from the same sources: `.../scratchpad/sqlite/sqlite3ref`
  (use it to compare behaviour, e.g. `sqlite3ref :memory: "explain select 1"`)

Compile-time configuration we mirror: the default amalgamation build plus
`SQLITE_ENABLE_COLUMN_METADATA`, `SQLITE_ENABLE_MATH_FUNCTIONS`,
`SQLITE_OMIT_SHARED_CACHE`, default `SQLITE_DQS=3`, no
`SQLITE_ENABLE_EXPLAIN_COMMENTS`, no `SQLITE_DEBUG` (asserts become
`debug_assert!`), UTF-8 only (UTF-16 entry points may stay unimplemented),
no load_extension, no mmap of the database file (the WAL -shm file is
mapped, see below).  JSON (json.rs) and WAL (wal.rs) are ported.

Virtual tables (vtab.rs) are ported: a module is an `Rc<dyn VtabModule>`
(its pAux is captured by the implementation), an instance a
`Box<dyn Vtab>` held by the per-connection `VTable` (`Table::u.vtab.p`),
a cursor a `Box<dyn VtabCursor>` stored in `VdbeCursor::pVCur`.  See the
traits at the top of vtab.rs for the contract (optional C methods are
default methods plus `has_*` predicates).

## Layout

One Rust module per C file, same name (`main.c` -> `mainapi.rs`,
`where.c` -> `where_.rs`).  Function, struct, field and constant names are
the C names (`sqlite3ExprCode`, `pParse->nMem` -> `db.pp().nMem`), so the
port can be diffed against upstream.  The crate allows non-snake-case names
and all clippy lints.

* `consts.rs` — every integer `#define` of the headers (generated; do not
  edit).  Types: `TK_*`/`OP_*` u8, `EP_*`/`SF_*`/`TF_*` u32, `MEM_*` u16,
  `SQLITE_*` result codes i32, db flags (`SQLITE_ForeignKeys` ...) u64, ...
  Cast with `as` where C mixes types.
* `int.rs` — the structures of sqliteInt.h / vdbeInt.h / vdbe.h.
* `arena.rs` — the parse-tree arena (see below).
* `hash.rs` — port of hash.c (`Hash<T>`, iteration order identical to C).
* `global.rs` — global.c tables, opcode names/properties, `sqlite3Isspace`
  & co, `sqlite3GlobalConfig()`.
* `printf.rs` — SQLite's printf (done).  Use `mprintf!(db, "fmt", a, b)`
  (sqlite3MPrintf) and `errmsg!(db, "fmt", a, b)` (sqlite3ErrorMsg).
  Argument conversions: integers, `f64`, `&[u8]`/`&Vec<u8>`/`&str`/
  `Option<&[u8]>` (C NULL), `&Token` (%T), `ExprId` (%#T), `SrcItemId` (%S).
* every other module starts as a skeleton: each non-static C function has
  a stub `pub fn ...{ todo!(...) }` with the C prototype in a doc comment.
  Your job is to replace the stubs of your modules by real ports (and add
  the static helper functions of the C file as private Rust functions).

## The big conventions

### 1. The connection is the context

`Parse*` and `sqlite3*` parameters are replaced by **one** parameter
`db: &mut Sqlite3`.  The parser state lives in `db.parses` (a stack; the
last one is the current parse = C `db->pParse`):

* `db.pp()` -> `&mut Parse` (current parse), `db.ppr()` -> `&Parse`
* `db.toplevel()` -> the toplevel parse (`sqlite3ParseToplevel()`); a
  sub-parse (trigger programs) is pushed with its `iToplevel` set to the
  index of the toplevel one.
* `db.v()` -> `&mut Vdbe` under construction (`pParse->pVdbe`).
  Builder functions in vdbeaux take `v: &mut Vdbe`:
  `sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, r)`.  Compute arguments that
  need `db` *before* the call (`let r = sqlite3GetTempReg(db);`), since
  `db.v()` borrows `db` mutably.
* Labels: the label counter is `Parse::nLabel` (`sqlite3VdbeMakeLabel(db)`),
  the label table is `Vdbe::aLabel` (`sqlite3VdbeResolveLabel(v, x)`).

Functions that in C take a `Vdbe*`, `BtCursor*`, `Btree*` ... and use
`p->db` inside get an extra leading `db: &mut Sqlite3` parameter (the
skeleton generator already did this when the body mentions `db`).

### 2. Parse trees live in an arena

All parse-tree nodes (Expr, ExprList, Select, SrcList, IdList, Window,
With, Upsert, AggInfo, NameContext, CteUse, TriggerStep) live in
`db.arena` and are referenced by typed ids (`ExprId`, `ExprListId`,
`SelectId`, `SrcListId`, `IdListId`, `WindowId`, `WithId`, `UpsertId`,
`AggInfoId`, `NcId`, `CteUseId`, `TriggerStepId`).  Id 0 is NULL:
`ExprId::NULL`, `e.is_null()`, `e.nn()`.

* access: `db.ex(e)` / `db.exm(e)` (Expr), `db.el(l)`/`db.elm(l)`,
  `db.sel(s)`/`db.selm(s)`, `db.src(s)`/`db.srcm(s)`,
  `db.item(SrcItemId)`/`db.itemm(..)`, `db.idl(..)`, `db.win(..)`,
  `db.arena.with(..)`, `db.arena.agg(..)`, `db.arena.nc(..)` (+ `m`
  variants), allocation `db.arena.new_expr(Expr{..})` etc.
* a C `SrcItem *pItem = &pSrc->a[i]` becomes `SrcItemId(pSrc, i)`.
* list lengths: `nExpr()`, `nSrc()`, `nId()` methods (the `a` Vec holds the
  items); `With::a` holds the CTEs.
* C unions are flattened into structs with the member names
  (`pExpr->u.zToken` -> `db.ex(e).u.zToken`, `pExpr->y.pTab` ->
  `.y.pTab`, `pExpr->x.pList` -> `.x.pList`).
* "delete" functions (`sqlite3ExprDelete` ...) do not free anything (the
  arena is reset when the outermost parse ends) but must keep any other
  side effect of the C function (e.g. unlinking a window).
* **Never keep a `&Expr` (or any arena borrow) across a call that takes
  `db`.** Copy the fields you need first:
  `let (op, l) = { let x = db.ex(e); (x.op, x.pLeft) };`
* Rust borrow rules also forbid `f(db, db.ex(e).pLeft)`: bind to a local.

Schema objects keep their trees in `Tree<I>` fields (`Table::pCheck`,
`Table::u.view.pSelect`, `Table::u.tab.pDfltList`, `Index::pPartIdxWhere`,
`Index::aColExpr`, `Trigger::pWhen`/`step_list`/`pColumns`): `Live(id)`
while the object is being built by the current parse, `Owned` once it is
published in a `Schema` (call `tree.publish(&db.arena)` right before/when
inserting the object into the schema hash).  `db.tree(&t)` returns an id
usable in `db.arena` (an owned tree is graph-copied into the arena; each
call makes a fresh copy, so call it once and keep the id).  `t.is_some()`
is the C `p != 0` test.

### 3. Schema objects are reference counted

`TableRef = Rc<RefCell<Table>>`, `IndexRef`, `FKeyRef`, `TriggerRef`,
`KeyInfoRef` (all `Rc<RefCell<..>>`), `CollSeqRef = Rc<CollSeq>`,
`FuncDefRef = Rc<FuncDef>` (immutable; replace instead of mutating).
Back pointers are `Weak` (`Index::pTable`, `FKey::pFrom`,
`TriggerStep::pTrig`).  Linked lists of the C code become `Vec`s in list
order: `Table::pIndex` (C pIndex/pNext), `Table::pTrigger`,
`Table::u.tab.pFKey`, `Sqlite3::pSavepoint` (most recent first),
`Schema::fkeyHash` values.  **Keep `borrow()`/`borrow_mut()` short**: never
hold one across a call that might borrow the same object (clone the Rc or
copy the fields out first).  Panics from a double borrow are bugs.

`Table::pSchema`, `Index::pSchema` ... are `SchemaId`s; the schema itself is
`db.aDb[i].pSchema` (owned by the Db); `db.schema_index(id)` is
`sqlite3SchemaToIndex()`.

### 4. Strings are bytes

SQL text, names, error messages: `&[u8]` / `ZStr = Vec<u8>` (SQLite is
8-bit clean; never assume UTF-8).  C `const char*` parameters are
`Option<&[u8]>` in the skeleton (None = NULL) — keep that unless a function
can never receive NULL, in which case `&[u8]` is fine.  Use byte literals
`b"..."`.  Case-insensitive comparison: `sqlite3StrICmp` /
`sqlite3StrNICmp` (util.rs), never Rust's Unicode-aware functions.

`Token { z: Option<Rc<[u8]>>, off, n }`: `t.text()`; tokens from the same
SQL share the same `Rc`, so C pointer arithmetic between tokens of one
statement becomes offset arithmetic (`t2.off - t1.off`).  The SQL text of
the current parse is `db.ppr().zSql` (`Rc<[u8]>`) and C `zTail` pointers
are offsets into it.

### 5. Values, functions, callbacks

* `Mem` (= `sqlite3_value`): C fields, owned `z: Vec<u8>` with `n` valid
  bytes; `u.i`, `u.r`, `u.nZero` are separate fields of `u`.  MEM_Dyn /
  MEM_Static / MEM_Ephem flags are kept for fidelity, storage is always
  owned.  Aggregate state: `pAgg: Option<Box<dyn Any>>`.
* SQL functions: `XSFunc = Rc<dyn Fn(&mut Context, &mut [Mem])>`
  (xSFunc/xStep/xInverse) and `XFinal = Rc<dyn Fn(&mut Context)>`
  (xFinal/xValue).  `Context` carries `db`, `pOut` (result), `pMem`
  (aggregate accumulator), `pAuxData`.  `sqlite3_value_text(&mut Mem) ->
  Option<&[u8]>`; `sqlite3_aggregate_context::<T>(ctx, n) -> Option<&mut T>`.
  Built-in functions are plain `fn(&mut Context, &mut [Mem])` wrapped in
  `Rc`.  `pUserData` is `FuncUserData` (`Int(n)` for SQLITE_INT_TO_PTR).
* Application callbacks (authorizer, busy handler, collations, hooks) are
  `Rc<dyn Fn ...>` closures stored in `Sqlite3`; user data is captured by
  the closure.  No re-entrancy into the same connection from callbacks.
* No out-of-memory: allocations cannot fail; `db.mallocFailed` stays 0
  (keep the checks, they are harmless).

### 6. VDBE

`Vdbe` is in int.rs (vdbeInt.h).  `aOp: Vec<VdbeOp>`, `VdbeOp::p4: P4`
(enum) plus `p4type` (the C P4_* code, kept for EXPLAIN).  Registers:
`aMem[1..=nMem]` (index = register number, as in C).  Cursors:
`apCsr: Vec<Option<Box<VdbeCursor>>>`.  Prepared statements live in
`db.aVdbe` (slots; `StmtId` handles); while a statement runs it is taken
out of its slot so that `sqlite3VdbeExec(db, &mut v)` can borrow both.

### 7. Storage

* pager <-> btree: `PagerP = Rc<RefCell<Pager>>`, pages are
  `DbPage = Rc<RefCell<PgHdr>>` with public `aData` (page bytes) and
  `pExtra` (btree's `MemPage`).  Page-level functions
  (`sqlite3PagerWrite(&DbPage)` ...) reach the pager through `PgHdr::pPager`
  (Weak); so never hold a `borrow_mut()` of the pager across them.
* btree <-> vdbe: `BtreeP = Rc<RefCell<Btree>>`,
  `BtCursorP = Rc<RefCell<BtCursor>>`; the btree keeps (weak) references to
  its open cursors for saveAllCursors().  The btree layer cannot reference
  `Sqlite3`; functions that need connection state take `db` as a
  parameter.  The busy handler is `Sqlite3::busyHandler`
  (`Rc<RefCell<BusyHandler>>`, shareable with the pager).
* files: `os.rs` / `os_unix.rs` implement the VFS over `std::fs` plus POSIX
  advisory locks (`libc::fcntl`), with SQLite's exact locking protocol so
  that the real `sqlite3` CLI can share databases with us.
* WAL: `wal.rs` is a complete port of wal.c.  The wal-index lives in the
  "-shm" file, mapped `MAP_SHARED` with `libc::mmap` by os_unix.rs (one
  `UnixShmNode` per inode and process, POSIX locks on bytes 120..128 of
  the -shm file), so goro and the C library can use the same WAL
  database concurrently.  The mapped memory is seen as `&[AtomicU32]`
  (`os::ShmRegion`); the WAL reaches the shm methods of the database
  file through an `os::ShmHandle` obtained when the WAL is opened.

### 8. Asserts, testcase(), debugging aids

`assert(x)` -> `debug_assert!(x)` (skip asserts that would need
infrastructure we do not have), `testcase(x)` / `VVA_ONLY` /
`SQLITE_DEBUG`-only code -> drop, `NEVER(x)` / `ALWAYS(x)` -> `x`.
TreeView / WHERETRACE / VdbeComment: drop (comments are not compiled in).

### 9. Signatures

The stub signatures were generated mechanically from the C prototypes
(rules above).  Other modules are being ported at the same time *against
these signatures*, so keep them unless they are unworkable.  If you must
change a public signature (e.g. a C out-pointer that is better returned,
a `/*...*/ ()` placeholder type, a callback), do it, and list every change
in your final report so the integrator can fix callers.  You may add
public helper functions.  Do not change other modules' files except for
trivial fixes needed to compile; never implement another module's
functions (call them; they are stubs for now).  You may add fields to
structures in int.rs if needed (report them).

### 10. Testing

Most modules cannot run until the whole engine is assembled.  Make your
module compile (`cargo check -p goro-sqlite` with your own
`CARGO_TARGET_DIR`), with no warnings in your files beyond the crate-level
allows, and add `#[cfg(test)]` unit tests where a module is testable on its
own (util, utf, printf, hash, btree+pager once both exist ...).
`cargo fmt` your files.
