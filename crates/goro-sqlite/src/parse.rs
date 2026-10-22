//! Port of parse.y: SQLite's SQL parser.
//!
//! The canonical source of the grammar is parse.y, a Lemon grammar file.
//! Lemon turns it into an LALR(1) push-down automaton (parse.c): the
//! parsing tables live in `parse_tables.rs` (generated from parse.c by the
//! scratch script gen_tables.py, do not edit by hand), the driver below is
//! a port of lempar.c (as instantiated in parse.c) and `yy_reduce_action()`
//! holds the C code of every grammar rule (the expanded versions of
//! parse.c's yy_reduce switch).
//!
//! Representation notes:
//!
//! * The C `YYMINORTYPE` union is the enum [`YYMINORTYPE`]; members are read
//!   with accessor methods named after the union members (`yy454()` reads
//!   an `Expr*`, `yy454m()` gives a mutable reference, turning the slot into
//!   that member as a C store into the union would).  Reading a member that
//!   was not stored yields its default (NULL / 0), where C would read
//!   garbage.
//! * Tokens carry the SQL text (`Token::z`, shared `Rc`) and an offset;
//!   C pointer arithmetic between tokens becomes offset arithmetic.
//! * Span arguments (C `const char *zStart, *zEnd` pairs given to
//!   sqlite3ExprListSetSpan, sqlite3AddDefaultValue, sqlite3AddCheckConstraint
//!   and the sqlite3TriggerXxxStep functions) are byte offsets into the SQL
//!   text being parsed, `db.ppr().zSql` (which is also the text of every
//!   token of the parse).  The `scanpt` non-terminal (C `const char*`,
//!   yy168) is such an offset.

use crate::alter::*;
use crate::analyze::sqlite3Analyze;
use crate::attach::{sqlite3Attach, sqlite3Detach};
use crate::build::*;
use crate::delete::sqlite3DeleteFrom;
use crate::expr::*;
use crate::global::{sqlite3Isdigit, sqlite3Isquote};
use crate::insert::{sqlite3Insert, sqlite3MultiValues, sqlite3MultiValuesEnd};
use crate::int::*;
use crate::pragma::sqlite3Pragma;
use crate::prepare::sqlite3ReadSchema;
use crate::select::*;
use crate::trigger::*;
use crate::update::sqlite3Update;
use crate::upsert::sqlite3UpsertNew;
use crate::util::{sqlite3_strnicmp, sqlite3DequoteExpr, sqlite3DequoteNumber, sqlite3GetInt32};
use crate::vacuum::sqlite3Vacuum;
use crate::vtab::*;
use crate::window::*;
use std::rc::Rc;

mod tables {
    include!("parse_tables.rs");
}
pub use tables::*;

// ---------------------------------------------------------------------------
// %include section of parse.y
// ---------------------------------------------------------------------------

/// An instance of the following structure describes the event of a
/// TRIGGER.  "a" is the event type, one of TK_UPDATE, TK_INSERT,
/// TK_DELETE, or TK_INSTEAD.  If the event is of the form
///
///      UPDATE ON (a,b,c)
///
/// Then the "b" IdList records the list "a,b,c".
#[derive(Clone, Copy, Default, Debug)]
pub struct TrigEvent {
    pub a: i32,
    pub b: IdListId,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct FrameBound {
    pub eType: i32,
    pub pExpr: ExprId,
}

/// The `struct {int value; int mask;}` of the refarg non-terminal.
#[derive(Clone, Copy, Default, Debug)]
pub struct RefArg {
    pub value: i32,
    pub mask: i32,
}

/// Generate a syntax error
/// The SQL text being parsed (C: the buffer the zStart/zEnd pointers
/// passed to sqlite3AddDefaultValue()/sqlite3AddCheckConstraint() point
/// into); token offsets are relative to it.
fn parseSql(db: &Sqlite3) -> Rc<[u8]> {
    db.ppr().zSql.clone().unwrap_or_else(|| Rc::from(&b""[..]))
}

fn parserSyntaxError(db: &mut Sqlite3, p: &Token) {
    errmsg!(db, b"near \"%T\": syntax error", p);
}

/// Disable lookaside memory allocation for objects that might be
/// shared across database connections.
fn disableLookaside(db: &mut Sqlite3) {
    let p = db.pp();
    p.disableLookaside += 1;
    p.u1.cr = ParseCr::default();
    /* DisableLookaside: there is no lookaside allocator in this port */
}

/// The op of expression `e`, 0 for a NULL expression (where C would
/// dereference a NULL pointer, which cannot happen without OOM).
#[inline]
fn eop(db: &Sqlite3, e: ExprId) -> u8 {
    if e.is_null() { 0 } else { db.ex(e).op }
}

/// For a compound SELECT statement, make sure p->pPrior->pNext==p for
/// all elements in the list.  And make sure list length does not exceed
/// SQLITE_LIMIT_COMPOUND_SELECT.
fn parserDoubleLinkSelect(db: &mut Sqlite3, p: SelectId) {
    debug_assert!(p.nn());
    if db.sel(p).pPrior.nn() {
        let mut pNext = SelectId::NULL;
        let mut pLoop = p;
        let mut cnt = 1;
        loop {
            {
                let s = db.selm(pLoop);
                s.pNext = pNext;
                s.selFlags |= SF_Compound;
            }
            pNext = pLoop;
            pLoop = db.sel(pLoop).pPrior;
            if pLoop.is_null() {
                break;
            }
            cnt += 1;
            let (ob, lim) = {
                let s = db.sel(pLoop);
                (s.pOrderBy, s.pLimit)
            };
            if ob.nn() || lim.nn() {
                let op = db.sel(pNext).op;
                let zOp = sqlite3SelectOpName(op as i32);
                errmsg!(
                    db,
                    b"%s clause should come after %s not before",
                    if ob.nn() { "ORDER BY" } else { "LIMIT" },
                    &zOp
                );
                break;
            }
        }
        let mxSelect = db.aLimit[SQLITE_LIMIT_COMPOUND_SELECT as usize];
        if (db.sel(p).selFlags & (SF_MultiValue | SF_Values)) == 0 && mxSelect > 0 && cnt > mxSelect
        {
            errmsg!(db, b"too many terms in compound SELECT");
        }
    }
}

/// Attach a With object describing the WITH clause to a Select
/// object describing the query for which the WITH clause is a prefix.
fn attachWithToSelect(db: &mut Sqlite3, pSelect: SelectId, pWith: WithId) -> SelectId {
    if pSelect.nn() {
        db.selm(pSelect).pWith = pWith;
        parserDoubleLinkSelect(db, pSelect);
    } else {
        sqlite3WithDelete(db, pWith);
    }
    pSelect
}

/// Return an integer that is the maximum allowed stack size
fn parserStackSizeLimit(db: &Sqlite3) -> i32 {
    db.aLimit[SQLITE_LIMIT_PARSER_DEPTH as usize]
}

/// Construct a new Expr object from a single token
fn tokenExpr(db: &mut Sqlite3, op: i32, t: Token) -> ExprId {
    let mut e = Expr::default();
    e.op = op as u8;
    e.affExpr = 0;
    e.flags = EP_Leaf;
    /* p->iAgg = -1; // Not required */
    e.op2 = 0;
    e.iTable = 0;
    e.iColumn = 0;
    e.u.zToken = Some(t.text().to_vec());
    e.w.iOfst = t.off as i32 - db.ppr().zTail as i32;
    e.nHeight = 1;
    let first = t.text().first().copied().unwrap_or(0);
    let p = db.arena.new_expr(e);
    if sqlite3Isquote(first) {
        sqlite3DequoteExpr(db, p);
    }
    if db.ppr().IN_RENAME_OBJECT() {
        sqlite3RenameTokenMap(db, RenameKey::Expr(p), &t);
    }
    p
}

/// Create a TK_ISNULL or TK_NOTNULL expression, perhaps optimized to
/// to TK_TRUEFALSE, if possible
fn sqlite3PExprIsNull(
    db: &mut Sqlite3,
    op: i32,       /* TK_ISNULL or TK_NOTNULL */
    pLeft: ExprId, /* Operand */
) -> ExprId {
    let mut p = pLeft;
    debug_assert!(op == TK_ISNULL as i32 || op == TK_NOTNULL as i32);
    debug_assert!(pLeft.nn());
    while p.nn() && (db.ex(p).op == TK_UPLUS || db.ex(p).op == TK_UMINUS) {
        p = db.ex(p).pLeft;
        debug_assert!(p.nn());
    }
    match eop(db, p) {
        TK_INTEGER | TK_STRING | TK_FLOAT | TK_BLOB => {
            sqlite3ExprDeferredDelete(db, pLeft);
            return sqlite3ExprInt32(db, (op == TK_NOTNULL as i32) as i32);
        }
        _ => {}
    }
    sqlite3PExpr(db, op, pLeft, ExprId::NULL)
}

/// Create a TK_IS or TK_ISNOT operator, perhaps optimized to
/// TK_ISNULL or TK_NOTNULL or TK_TRUEFALSE.
fn sqlite3PExprIs(
    db: &mut Sqlite3,
    op: i32,        /* TK_IS or TK_ISNOT */
    pLeft: ExprId,  /* Left operand */
    pRight: ExprId, /* Right operand */
) -> ExprId {
    if pRight.nn() && db.ex(pRight).op == TK_NULL {
        sqlite3ExprDeferredDelete(db, pRight);
        return sqlite3PExprIsNull(
            db,
            if op == TK_IS as i32 {
                TK_ISNULL as i32
            } else {
                TK_NOTNULL as i32
            },
            pLeft,
        );
    }
    sqlite3PExpr(db, op, pLeft, pRight)
}

/// Add a single new term to an ExprList that is used to store a
/// list of identifiers.  Report an error if the ID list contains
/// a COLLATE clause or an ASC or DESC keyword, except ignore the
/// error while parsing a legacy schema.
fn parserAddExprIdListTerm(
    db: &mut Sqlite3,
    pPrior: ExprListId,
    pIdToken: &Token,
    hasCollate: i32,
    sortOrder: i32,
) -> ExprListId {
    let p = sqlite3ExprListAppend(db, pPrior, ExprId::NULL);
    if (hasCollate != 0 || sortOrder != SQLITE_SO_UNDEFINED as i32) && db.init.busy == 0 {
        errmsg!(
            db,
            b"syntax error after column name \"%.*s\"",
            pIdToken.n as i32,
            pIdToken.text()
        );
    }
    sqlite3ExprListSetName(db, p, pIdToken, 1);
    p
}

/// The SQL text of `t` from its start to the end of the source (C code
/// that passes `t.z` to a function reading a NUL-terminated string).
fn tok_rest(t: &Token) -> &[u8] {
    match &t.z {
        Some(s) => &s[(t.off as usize).min(s.len())..],
        None => b"",
    }
}

// ---------------------------------------------------------------------------
// The parser state (lempar.c)
// ---------------------------------------------------------------------------

/// The semantic value of a symbol (C union YYMINORTYPE).
#[derive(Default)]
pub enum YYMINORTYPE {
    #[default]
    yyinit,
    yy0(Token),
    yy14(ExprListId),
    yy59(WithId),
    yy67(Option<Box<Cte>>),
    yy122(UpsertId),
    yy132(IdListId),
    yy144(i32),
    /// `const char*` into the SQL text: an offset into db.ppr().zSql.
    yy168(usize),
    yy203(SrcListId),
    yy211(WindowId),
    yy269(OnOrUsing),
    yy286(TrigEvent),
    yy383(RefArg),
    yy391(u32),
    yy427(TriggerStepId),
    yy454(ExprId),
    yy462(u8),
    yy509(FrameBound),
    yy555(SelectId),
}

macro_rules! minor_acc {
    ($get:ident, $getm:ident, $var:ident, $ty:ty) => {
        /// Read this union member (default value if another member is stored).
        #[inline]
        pub fn $get(&self) -> $ty {
            match self {
                YYMINORTYPE::$var(v) => v.clone(),
                _ => <$ty>::default(),
            }
        }
        /// Store into this union member.
        #[inline]
        pub fn $getm(&mut self) -> &mut $ty {
            if !matches!(self, YYMINORTYPE::$var(_)) {
                *self = YYMINORTYPE::$var(<$ty>::default());
            }
            match self {
                YYMINORTYPE::$var(v) => v,
                _ => unreachable!(),
            }
        }
    };
}

impl YYMINORTYPE {
    minor_acc!(yy0, yy0m, yy0, Token);
    minor_acc!(yy14, yy14m, yy14, ExprListId);
    minor_acc!(yy59, yy59m, yy59, WithId);
    minor_acc!(yy122, yy122m, yy122, UpsertId);
    minor_acc!(yy132, yy132m, yy132, IdListId);
    minor_acc!(yy144, yy144m, yy144, i32);
    minor_acc!(yy168, yy168m, yy168, usize);
    minor_acc!(yy203, yy203m, yy203, SrcListId);
    minor_acc!(yy211, yy211m, yy211, WindowId);
    minor_acc!(yy269, yy269m, yy269, OnOrUsing);
    minor_acc!(yy286, yy286m, yy286, TrigEvent);
    minor_acc!(yy383, yy383m, yy383, RefArg);
    minor_acc!(yy391, yy391m, yy391, u32);
    minor_acc!(yy427, yy427m, yy427, TriggerStepId);
    minor_acc!(yy454, yy454m, yy454, ExprId);
    minor_acc!(yy462, yy462m, yy462, u8);
    minor_acc!(yy509, yy509m, yy509, FrameBound);
    minor_acc!(yy555, yy555m, yy555, SelectId);

    /// Take the Cte (yy67) out of the slot (C passes the pointer on).
    pub fn take_yy67(&mut self) -> Option<Box<Cte>> {
        match self {
            YYMINORTYPE::yy67(c) => c.take(),
            _ => None,
        }
    }
    pub fn set_yy67(&mut self, c: Option<Box<Cte>>) {
        *self = YYMINORTYPE::yy67(c);
    }
}

/// The following structure represents a single element of the
/// parser's stack.  Information stored includes:
///
///   +  The state number for the parser at this level of the stack.
///
///   +  The value of the token stored at this level of the stack.
///      (In other words, the "major" token.)
///
///   +  The semantic value stored at this level of the stack.  This is
///      the information used by the action routines in the grammar.
///      It is sometimes called the "minor" token.
#[derive(Default)]
pub struct yyStackEntry {
    pub stateno: u16, /* The state-number, or reduce action in SHIFTREDUCE */
    pub major: u16,   /* The major token value.  This is the code
                       ** number for the token at this stack level */
    pub minor: YYMINORTYPE, /* The user-supplied minor token value.  This
                             ** is the value of the token  */
}

/// The state of the parser is completely contained in an instance of
/// the following structure.  The stack is a vector whose length is the
/// C stack allocation (yystackEnd is its last index); yytos is the index
/// of the top element.
pub struct yyParser {
    pub yytos: usize,
    pub yystack: Vec<yyStackEntry>,
    pub yystackEnd: usize,
}

impl Default for yyParser {
    fn default() -> Self {
        yyParser::new()
    }
}

/// What the generic driver needs from its context (the C
/// `%extra_context Parse *pParse`): the actions of the grammar, the
/// %syntax_error / %stack_overflow code, the destructors and the stack size
/// limit.  The real context is the connection (`Sqlite3`); unit tests use a
/// context without actions to exercise the automaton alone.
pub trait YyCtx {
    fn yy_reduce_action(
        &mut self,
        p: &mut yyParser,
        yyruleno: u32,
        yyLookahead: i32,
        yyLookaheadToken: &Token,
    );
    fn yy_syntax_error(&mut self, yymajor: i32, yyminor: &Token);
    fn yy_stack_overflow(&mut self);
    fn yy_destructor(&mut self, yymajor: u16, yypminor: &mut YYMINORTYPE);
    fn yy_size_limit(&self) -> i32;
}

impl yyParser {
    /// Initialize a new parser (sqlite3ParserInit / sqlite3ParserAlloc).
    pub fn new() -> yyParser {
        let mut yystack = Vec::with_capacity(YYSTACKDEPTH as usize);
        yystack.resize_with(YYSTACKDEPTH as usize, yyStackEntry::default);
        yyParser {
            yytos: 0,
            yystack,
            yystackEnd: (YYSTACKDEPTH - 1) as usize,
        }
    }

    /// Clear all secondary memory allocations from the parser
    /// (sqlite3ParserFinalize / sqlite3ParserFree), calling the
    /// destructors of the symbols left on the stack.
    pub fn finalize(&mut self, db: &mut Sqlite3) {
        yy_finalize(self, db);
    }
}

fn yy_finalize<C: YyCtx + ?Sized>(p: &mut yyParser, ctx: &mut C) {
    /* In-lined version of calling yy_pop_parser_stack() for each
     ** element left in the stack */
    while p.yytos > 0 {
        let t = p.yytos;
        if p.yystack[t].major as i32 >= YY_MIN_DSTRCTR {
            let major = p.yystack[t].major;
            let mut m = std::mem::take(&mut p.yystack[t].minor);
            ctx.yy_destructor(major, &mut m);
        }
        p.yytos -= 1;
    }
}

/// Try to increase the size of the parser stack.  Return the number
/// of errors.  Return 0 on success.
fn yyGrowStack<C: YyCtx + ?Sized>(p: &mut yyParser, ctx: &mut C) -> i32 {
    let oldSize = 1 + p.yystackEnd as i32;
    let nLimit = ctx.yy_size_limit();
    let mut newSize = oldSize * 2 + 100;
    if newSize > nLimit {
        newSize = nLimit;
        if newSize <= oldSize {
            return 1;
        }
    }
    p.yystack
        .resize_with(newSize as usize, yyStackEntry::default);
    p.yystackEnd = (newSize - 1) as usize;
    0
}

/// Pop the parser's stack once.
/// If there is a destructor routine associated with the token which
/// is popped from the stack, then call it.
fn yy_pop_parser_stack<C: YyCtx + ?Sized>(p: &mut yyParser, ctx: &mut C) {
    debug_assert!(p.yytos > 0);
    let t = p.yytos;
    p.yytos -= 1;
    let major = p.yystack[t].major;
    let mut m = std::mem::take(&mut p.yystack[t].minor);
    ctx.yy_destructor(major, &mut m);
}

/// Find the appropriate action for a parser given the terminal
/// look-ahead token iLookAhead.
fn yy_find_shift_action(mut iLookAhead: u16, stateno: u16) -> u16 {
    if stateno as i32 > YY_MAX_SHIFT {
        return stateno;
    }
    debug_assert!(stateno as i32 <= YY_SHIFT_COUNT);
    loop {
        let mut i = yy_shift_ofst[stateno as usize] as i32;
        debug_assert!(i >= 0);
        debug_assert!(i <= YY_ACTTAB_COUNT);
        debug_assert!(i + YYNTOKEN <= yy_lookahead.len() as i32);
        debug_assert!(iLookAhead as i32 != YYNOCODE);
        debug_assert!((iLookAhead as i32) < YYNTOKEN);
        i += iLookAhead as i32;
        debug_assert!(i < yy_lookahead.len() as i32);
        if yy_lookahead[i as usize] != iLookAhead {
            /* Fallback token */
            let iFallback = yyFallback[iLookAhead as usize];
            if iFallback != 0 {
                debug_assert!(yyFallback[iFallback as usize] == 0); /* Fallback loop must terminate */
                iLookAhead = iFallback;
                continue;
            }
            {
                let j = i - iLookAhead as i32 + YYWILDCARD;
                debug_assert!((j as usize) < yy_lookahead.len());
                if yy_lookahead[j as usize] as i32 == YYWILDCARD && iLookAhead > 0 {
                    return yy_action[j as usize];
                }
            }
            return yy_default[stateno as usize];
        } else {
            debug_assert!(i >= 0 && (i as usize) < yy_action.len());
            return yy_action[i as usize];
        }
    }
}

/// Find the appropriate action for a parser given the non-terminal
/// look-ahead token iLookAhead.
fn yy_find_reduce_action(stateno: u16, iLookAhead: u16) -> u16 {
    debug_assert!(stateno as i32 <= YY_REDUCE_COUNT);
    let mut i = yy_reduce_ofst[stateno as usize] as i32;
    debug_assert!(iLookAhead as i32 != YYNOCODE);
    i += iLookAhead as i32;
    debug_assert!(i >= 0 && i < YY_ACTTAB_COUNT);
    debug_assert!(yy_lookahead[i as usize] == iLookAhead);
    yy_action[i as usize]
}

/// The following routine is called if the stack overflows.
fn yyStackOverflow<C: YyCtx + ?Sized>(p: &mut yyParser, ctx: &mut C) {
    while p.yytos > 0 {
        yy_pop_parser_stack(p, ctx);
    }
    /* Here code is inserted which will execute if the parser
     ** stack every overflows */
    ctx.yy_stack_overflow();
}

/// Perform a shift action.
fn yy_shift<C: YyCtx + ?Sized>(
    p: &mut yyParser,
    ctx: &mut C,
    mut yyNewState: u16, /* The new state to shift in */
    yyMajor: u16,        /* The major token to shift in */
    yyMinor: Token,      /* The minor token to shift in */
) {
    p.yytos += 1;
    if p.yytos > p.yystackEnd {
        if yyGrowStack(p, ctx) != 0 {
            p.yytos -= 1;
            yyStackOverflow(p, ctx);
            return;
        }
        debug_assert!(p.yytos <= p.yystackEnd);
    }
    if yyNewState as i32 > YY_MAX_SHIFT {
        yyNewState += (YY_MIN_REDUCE - YY_MIN_SHIFTREDUCE) as u16;
    }
    let e = &mut p.yystack[p.yytos];
    e.stateno = yyNewState;
    e.major = yyMajor;
    e.minor = YYMINORTYPE::yy0(yyMinor);
}

/// Perform a reduce action and the shift that must immediately
/// follow the reduce.
///
/// The yyLookahead and yyLookaheadToken parameters provide reduce actions
/// access to the lookahead token (if any).  The yyLookahead will be YYNOCODE
/// if the lookahead token has already been consumed.
fn yy_reduce<C: YyCtx + ?Sized>(
    p: &mut yyParser,
    ctx: &mut C,
    yyruleno: u32,            /* Number of the rule by which to reduce */
    yyLookahead: i32,         /* Lookahead token, or YYNOCODE if none */
    yyLookaheadToken: &Token, /* Value of the lookahead token */
) -> u16 {
    ctx.yy_reduce_action(p, yyruleno, yyLookahead, yyLookaheadToken);
    debug_assert!((yyruleno as usize) < yyRuleInfoLhs.len());
    let yymsp = p.yytos as isize;
    let yygoto = yyRuleInfoLhs[yyruleno as usize];
    let yysize = yyRuleInfoNRhs[yyruleno as usize] as isize;
    let yyact = yy_find_reduce_action(p.yystack[(yymsp + yysize) as usize].stateno, yygoto);

    /* There are no SHIFTREDUCE actions on nonterminals because the table
     ** generator has simplified them to pure REDUCE actions. */
    debug_assert!(!(yyact as i32 > YY_MAX_SHIFT && yyact as i32 <= YY_MAX_SHIFTREDUCE));

    /* It is not possible for a REDUCE to be followed by an error */
    debug_assert!(yyact as i32 != YY_ERROR_ACTION);

    let t = (yymsp + yysize + 1) as usize;
    p.yytos = t;
    p.yystack[t].stateno = yyact;
    p.yystack[t].major = yygoto;
    yyact
}

/// The main parser program, generic over its context.
///
/// Inputs: the parser, the context, the major token number and the minor
/// token (the token value).
pub fn sqlite3ParserCtx<C: YyCtx + ?Sized>(
    p: &mut yyParser,
    ctx: &mut C,
    yymajor: i32,
    yyminor: Token,
) {
    let mut yyact: u16 = p.yystack[p.yytos].stateno; /* The parser action. */

    loop {
        /* Exit by "break" */
        debug_assert!(yyact == p.yystack[p.yytos].stateno);
        yyact = yy_find_shift_action(yymajor as u16, yyact);
        if yyact as i32 >= YY_MIN_REDUCE {
            let yyruleno = (yyact as i32 - YY_MIN_REDUCE) as u32; /* Reduce by this rule */
            debug_assert!((yyruleno as i32) < YYNRULE);
            /* Check that the stack is large enough to grow by a single entry
             ** if the RHS of the rule is empty.  This ensures that there is room
             ** enough on the stack to push the LHS value */
            if yyRuleInfoNRhs[yyruleno as usize] == 0 && p.yytos >= p.yystackEnd {
                if yyGrowStack(p, ctx) != 0 {
                    yyStackOverflow(p, ctx);
                    break;
                }
            }
            yyact = yy_reduce(p, ctx, yyruleno, yymajor, &yyminor);
        } else if yyact as i32 <= YY_MAX_SHIFTREDUCE {
            yy_shift(p, ctx, yyact, yymajor as u16, yyminor);
            break;
        } else if yyact as i32 == YY_ACCEPT_ACTION {
            p.yytos -= 1;
            /* yy_accept(): nothing to do */
            debug_assert!(p.yytos == 0);
            return;
        } else {
            debug_assert!(yyact as i32 == YY_ERROR_ACTION);
            /* If the YYNOERRORRECOVERY macro is defined, then do not attempt to
             ** do any kind of error recovery.  Instead, simply invoke the syntax
             ** error routine and continue going as if nothing had happened.
             **
             ** Applications can set this macro (for example inside %include) if
             ** they intend to abandon the parse upon the first syntax error seen.
             */
            ctx.yy_syntax_error(yymajor, &yyminor);
            let mut u = YYMINORTYPE::yy0(yyminor);
            ctx.yy_destructor(yymajor as u16, &mut u);
            break;
        }
    }
}

/// C: `void sqlite3Parser(void *yyp, int yymajor, Token yyminor)` with
/// the connection as context.
pub fn sqlite3Parser(p: &mut yyParser, db: &mut Sqlite3, yymajor: i32, yyminor: Token) {
    sqlite3ParserCtx(p, db, yymajor, yyminor);
}

/// Return the fallback token corresponding to canonical token iToken, or
/// 0 if iToken has no fallback.
///
/// C: `int sqlite3ParserFallback(int iToken)`
pub fn sqlite3ParserFallback(iToken: i32) -> i32 {
    debug_assert!((iToken as usize) < yyFallback.len());
    yyFallback[iToken as usize] as i32
}

// ---------------------------------------------------------------------------
// The connection as parser context: destructors, error code and the
// reduce actions of every grammar rule.
// ---------------------------------------------------------------------------

impl YyCtx for Sqlite3 {
    fn yy_size_limit(&self) -> i32 {
        parserStackSizeLimit(self)
    }

    /// %syntax_error
    fn yy_syntax_error(&mut self, _yymajor: i32, TOKEN: &Token) {
        let db = self;
        if TOKEN.at(0) != 0 {
            parserSyntaxError(db, TOKEN);
        } else {
            errmsg!(db, b"incomplete input");
        }
    }

    /// %stack_overflow
    fn yy_stack_overflow(&mut self) {
        let db = self;
        if db.ppr().nErr == 0 {
            errmsg!(db, b"Recursion limit");
        }
    }

    /// The following function deletes the "minor type" or semantic value
    /// associated with a symbol (the %destructor directives of parse.y).
    fn yy_destructor(&mut self, yymajor: u16, m: &mut YYMINORTYPE) {
        let db = self;
        match yymajor {
            206 /* select */ | 241 /* selectnowith */ | 242 /* oneselect */
            | 254 /* values */ | 256 /* mvalues */ => {
                sqlite3SelectDelete(db, m.yy555());
            }
            218 /* term */ | 219 /* expr */ | 248 /* where_opt */ | 250 /* having_opt */
            | 270 /* where_opt_ret */ | 281 /* case_operand */ | 283 /* case_else */
            | 286 /* vinto */ | 293 /* when_clause */ | 297 /* key_opt */
            | 314 /* filter_clause */ => {
                sqlite3ExprDelete(db, m.yy454());
            }
            223 /* eidlist_opt */ | 233 /* sortlist */ | 234 /* eidlist */
            | 246 /* selcollist */ | 249 /* groupby_opt */ | 251 /* orderby_opt */
            | 255 /* nexprlist */ | 257 /* sclp */ | 264 /* exprlist */
            | 271 /* setlist */ | 280 /* paren_exprlist */ | 282 /* case_exprlist */
            | 313 /* part_opt */ => {
                sqlite3ExprListDelete(db, m.yy14());
            }
            240 /* fullname */ | 247 /* from */ | 259 /* seltablist */
            | 260 /* stl_prefix */ | 265 /* xfullname */ => {
                sqlite3SrcListDelete(db, m.yy203());
            }
            243 /* wqlist */ => {
                sqlite3WithDelete(db, m.yy59());
            }
            253 /* window_clause */ | 309 /* windowdefn_list */ => {
                sqlite3WindowListDelete(db, m.yy211());
            }
            266 /* idlist */ | 273 /* idlist_opt */ => {
                sqlite3IdListDelete(db, m.yy132());
            }
            276 /* filter_over */ | 310 /* windowdefn */ | 311 /* window */
            | 312 /* frame_opt */ | 315 /* over_clause */ => {
                sqlite3WindowDelete(db, m.yy211());
            }
            289 /* trigger_cmd_list */ | 294 /* trigger_cmd */ => {
                sqlite3DeleteTriggerStep(db, m.yy427());
            }
            291 /* trigger_event */ => {
                sqlite3IdListDelete(db, m.yy286().b);
            }
            317 /* frame_bound */ | 318 /* frame_bound_s */ | 319 /* frame_bound_e */ => {
                sqlite3ExprDelete(db, m.yy509().pExpr);
            }
            _ => {} /* If no destructor action specified: do nothing */
        }
    }

    fn yy_reduce_action(
        &mut self,
        p: &mut yyParser,
        yyruleno: u32,
        yyLookahead: i32,
        yyLookaheadToken: &Token,
    ) {
        reduce_action(self, p, yyruleno, yyLookahead, yyLookaheadToken);
    }
}

/// The reduce actions of parse.y (C yy_reduce switch).
fn reduce_action(
    db: &mut Sqlite3,
    p: &mut yyParser,
    yyruleno: u32,
    yyLookahead: i32,
    yyLookaheadToken: &Token,
) {
    let yymsp = p.yytos as isize;
    /* yymsp[k].minor */
    macro_rules! M {
        ($k:expr) => {
            p.yystack[(yymsp + ($k)) as usize].minor
        };
    }
    /* yymsp[k].major */
    macro_rules! MAJ {
        ($k:expr) => {
            (p.yystack[(yymsp + ($k)) as usize].major as i32)
        };
    }
    /* yymsp[k].minor.member = value (value evaluated first) */
    macro_rules! set {
        ($k:expr, $m:ident, $v:expr) => {{
            let v = $v;
            *M!($k).$m() = v;
        }};
    }
    const N: ExprId = ExprId::NULL;
    const NL: ExprListId = ExprListId::NULL;
    const NS: SrcListId = SrcListId::NULL;
    const NSEL: SelectId = SelectId::NULL;
    macro_rules! zTail {
        () => {
            (db.ppr().zTail as i32)
        };
    }
    macro_rules! IN_RENAME_OBJECT {
        () => {
            db.ppr().IN_RENAME_OBJECT()
        };
    }

    match yyruleno {
        0 => {
            /* explain ::= EXPLAIN */
            if db.ppr().pReprepare.is_none() {
                db.pp().explain = 1;
            }
        }
        1 => {
            /* explain ::= EXPLAIN QUERY PLAN */
            if db.ppr().pReprepare.is_none() {
                db.pp().explain = 2;
            }
        }
        2 => {
            /* cmdx ::= cmd */
            sqlite3FinishCoding(db);
        }
        3 => {
            /* cmd ::= BEGIN transtype trans_opt */
            sqlite3BeginTransaction(db, M!(-1).yy144());
        }
        4 => {
            /* transtype ::= */
            set!(1, yy144m, TK_DEFERRED as i32);
        }
        5 | 6 | 7 | 328 => {
            /* transtype ::= DEFERRED|IMMEDIATE|EXCLUSIVE */
            /* range_or_rows ::= RANGE|ROWS|GROUPS */
            set!(0, yy144m, MAJ!(0)); /*A-overwrites-X*/
        }
        8 | 9 => {
            /* cmd ::= COMMIT|END trans_opt */
            /* cmd ::= ROLLBACK trans_opt */
            sqlite3EndTransaction(db, MAJ!(-1));
        }
        10 => {
            /* cmd ::= SAVEPOINT nm */
            sqlite3Savepoint(db, SAVEPOINT_BEGIN, &M!(0).yy0());
        }
        11 => {
            /* cmd ::= RELEASE savepoint_opt nm */
            sqlite3Savepoint(db, SAVEPOINT_RELEASE, &M!(0).yy0());
        }
        12 => {
            /* cmd ::= ROLLBACK trans_opt TO savepoint_opt nm */
            sqlite3Savepoint(db, SAVEPOINT_ROLLBACK, &M!(0).yy0());
        }
        13 => {
            /* create_table ::= createkw temp TABLE ifnotexists nm dbnm */
            sqlite3StartTable(
                db,
                &M!(-1).yy0(),
                &M!(0).yy0(),
                M!(-4).yy144(),
                0,
                0,
                M!(-2).yy144(),
            );
        }
        14 => {
            /* createkw ::= CREATE */
            disableLookaside(db);
        }
        15 | 18 | 47 | 62 | 72 | 81 | 100 | 246 => {
            /* ifnotexists ::= */
            /* temp ::= */
            /* autoinc ::= */
            /* init_deferred_pred_opt ::= */
            /* defer_subclause_opt ::= */
            /* ifexists ::= */
            /* distinct ::= */
            /* collate ::= */
            set!(1, yy144m, 0);
        }
        16 => {
            /* ifnotexists ::= IF NOT EXISTS */
            set!(-2, yy144m, 1);
        }
        17 => {
            /* temp ::= TEMP */
            set!(0, yy144m, (db.init.busy == 0) as i32);
        }
        19 => {
            /* create_table_args ::= LP columnlist conslist_opt RP table_option_set */
            sqlite3EndTable(
                db,
                Some(&M!(-2).yy0()),
                Some(&M!(-1).yy0()),
                M!(0).yy391(),
                NSEL,
            );
        }
        20 => {
            /* create_table_args ::= AS select */
            sqlite3EndTable(db, None, None, 0, M!(0).yy555());
            sqlite3SelectDelete(db, M!(0).yy555());
        }
        21 => {
            /* table_option_set ::= */
            set!(1, yy391m, 0);
        }
        22 => {
            /* table_option_set ::= table_option_set COMMA table_option */
            set!(-2, yy391m, M!(-2).yy391() | M!(0).yy391());
        }
        23 => {
            /* table_option ::= WITHOUT nm */
            let t = M!(0).yy0();
            if t.n == 5 && sqlite3_strnicmp(Some(t.text()), Some(b"rowid"), 5) == 0 {
                set!(-1, yy391m, TF_WithoutRowid | TF_NoVisibleRowid);
            } else {
                set!(-1, yy391m, 0);
                errmsg!(db, b"unknown table option: %.*s", t.n as i32, t.text());
            }
        }
        24 => {
            /* table_option ::= nm */
            let t = M!(0).yy0();
            if t.n == 6 && sqlite3_strnicmp(Some(t.text()), Some(b"strict"), 6) == 0 {
                set!(0, yy391m, TF_Strict);
            } else {
                set!(0, yy391m, 0);
                errmsg!(db, b"unknown table option: %.*s", t.n as i32, t.text());
            }
        }
        25 => {
            /* columnname ::= nm typetoken */
            sqlite3AddColumn(db, M!(-1).yy0(), M!(0).yy0());
        }
        26 | 65 | 106 => {
            /* typetoken ::= */
            /* conslist_opt ::= */
            /* as ::= */
            let t = M!(1).yy0m();
            t.n = 0;
            t.z = None;
            t.off = 0;
        }
        27 => {
            /* typetoken ::= typename LP signed RP */
            let e = M!(0).yy0();
            let t = M!(-3).yy0m();
            t.n = e.off + e.n - t.off;
        }
        28 => {
            /* typetoken ::= typename LP signed COMMA signed RP */
            let e = M!(0).yy0();
            let t = M!(-5).yy0m();
            t.n = e.off + e.n - t.off;
        }
        29 => {
            /* typename ::= typename ID|STRING */
            let e = M!(0).yy0();
            let t = M!(-1).yy0m();
            t.n = e.n + (e.off - t.off);
        }
        30 => {
            /* scanpt ::= */
            debug_assert!(yyLookahead != YYNOCODE);
            set!(1, yy168m, yyLookaheadToken.off as usize);
        }
        31 => {
            /* scantok ::= */
            debug_assert!(yyLookahead != YYNOCODE);
            set!(1, yy0m, yyLookaheadToken.clone());
        }
        32 | 67 => {
            /* ccons ::= CONSTRAINT nm */
            /* tcons ::= CONSTRAINT nm */
            let t = M!(0).yy0();
            db.pp().u1.cr.constraintName = t;
        }
        33 => {
            /* ccons ::= DEFAULT scantok term */
            let s = M!(-1).yy0();
            let zSql = parseSql(db);
            sqlite3AddDefaultValue(
                db,
                M!(0).yy454(),
                &zSql,
                s.off as usize,
                (s.off + s.n) as usize,
            );
        }
        34 => {
            /* ccons ::= DEFAULT LP expr RP */
            let zSql = parseSql(db);
            sqlite3AddDefaultValue(
                db,
                M!(-1).yy454(),
                &zSql,
                M!(-2).yy0().off as usize + 1,
                M!(0).yy0().off as usize,
            );
        }
        35 => {
            /* ccons ::= DEFAULT PLUS scantok term */
            let s = M!(-1).yy0();
            let zSql = parseSql(db);
            sqlite3AddDefaultValue(
                db,
                M!(0).yy454(),
                &zSql,
                M!(-2).yy0().off as usize,
                (s.off + s.n) as usize,
            );
        }
        36 => {
            /* ccons ::= DEFAULT MINUS scantok term */
            let p = sqlite3PExpr(db, TK_UMINUS as i32, M!(0).yy454(), N);
            let s = M!(-1).yy0();
            let zSql = parseSql(db);
            sqlite3AddDefaultValue(
                db,
                p,
                &zSql,
                M!(-2).yy0().off as usize,
                (s.off + s.n) as usize,
            );
        }
        37 => {
            /* ccons ::= DEFAULT scantok ID|INDEXED */
            let t = M!(0).yy0();
            let p = tokenExpr(db, TK_STRING as i32, t.clone());
            if p.nn() {
                sqlite3ExprIdToTrueFalse(db, p);
            }
            let zSql = parseSql(db);
            sqlite3AddDefaultValue(db, p, &zSql, t.off as usize, (t.off + t.n) as usize);
        }
        38 => {
            /* ccons ::= NOT NULL onconf */
            sqlite3AddNotNull(db, M!(0).yy144());
        }
        39 => {
            /* ccons ::= PRIMARY KEY sortorder onconf autoinc */
            sqlite3AddPrimaryKey(db, NL, M!(-1).yy144(), M!(0).yy144(), M!(-2).yy144());
        }
        40 => {
            /* ccons ::= UNIQUE onconf */
            sqlite3CreateIndex(
                db,
                None,
                None,
                NS,
                NL,
                M!(0).yy144(),
                None,
                N,
                0,
                0,
                SQLITE_IDXTYPE_UNIQUE,
            );
        }
        41 => {
            /* ccons ::= CHECK LP expr RP */
            let zSql = parseSql(db);
            sqlite3AddCheckConstraint(
                db,
                M!(-1).yy454(),
                &zSql,
                M!(-2).yy0().off as usize,
                M!(0).yy0().off as usize,
            );
        }
        42 => {
            /* ccons ::= REFERENCES nm eidlist_opt refargs */
            sqlite3CreateForeignKey(db, NL, &M!(-2).yy0(), M!(-1).yy14(), M!(0).yy144());
        }
        43 => {
            /* ccons ::= defer_subclause */
            sqlite3DeferForeignKey(db, M!(0).yy144());
        }
        44 => {
            /* ccons ::= COLLATE ID|STRING */
            sqlite3AddCollateType(db, &M!(0).yy0());
        }
        45 => {
            /* generated ::= LP expr RP */
            sqlite3AddGenerated(db, M!(-1).yy454(), None);
        }
        46 => {
            /* generated ::= LP expr RP ID */
            sqlite3AddGenerated(db, M!(-2).yy454(), Some(&M!(0).yy0()));
        }
        48 => {
            /* autoinc ::= AUTOINCR */
            set!(0, yy144m, 1);
        }
        49 => {
            /* refargs ::= */
            set!(1, yy144m, OE_None as i32 * 0x0101); /* EV: R-19803-45884 */
        }
        50 => {
            /* refargs ::= refargs refarg */
            let ra = M!(0).yy383();
            set!(-1, yy144m, (M!(-1).yy144() & !ra.mask) | ra.value);
        }
        51 => {
            /* refarg ::= MATCH nm */
            set!(
                -1,
                yy383m,
                RefArg {
                    value: 0,
                    mask: 0x000000
                }
            );
        }
        52 => {
            /* refarg ::= ON INSERT refact */
            set!(
                -2,
                yy383m,
                RefArg {
                    value: 0,
                    mask: 0x000000
                }
            );
        }
        53 => {
            /* refarg ::= ON DELETE refact */
            set!(
                -2,
                yy383m,
                RefArg {
                    value: M!(0).yy144(),
                    mask: 0x0000ff
                }
            );
        }
        54 => {
            /* refarg ::= ON UPDATE refact */
            set!(
                -2,
                yy383m,
                RefArg {
                    value: M!(0).yy144() << 8,
                    mask: 0x00ff00
                }
            );
        }
        55 => {
            /* refact ::= SET NULL */
            set!(-1, yy144m, OE_SetNull as i32); /* EV: R-33326-45252 */
        }
        56 => {
            /* refact ::= SET DEFAULT */
            set!(-1, yy144m, OE_SetDflt as i32); /* EV: R-33326-45252 */
        }
        57 => {
            /* refact ::= CASCADE */
            set!(0, yy144m, OE_Cascade as i32); /* EV: R-33326-45252 */
        }
        58 => {
            /* refact ::= RESTRICT */
            set!(0, yy144m, OE_Restrict as i32); /* EV: R-33326-45252 */
        }
        59 => {
            /* refact ::= NO ACTION */
            set!(-1, yy144m, OE_None as i32); /* EV: R-33326-45252 */
        }
        60 => {
            /* defer_subclause ::= NOT DEFERRABLE init_deferred_pred_opt */
            set!(-2, yy144m, 0);
        }
        61 | 76 | 173 => {
            /* defer_subclause ::= DEFERRABLE init_deferred_pred_opt */
            /* orconf ::= OR resolvetype */
            /* insert_cmd ::= INSERT orconf */
            set!(-1, yy144m, M!(0).yy144());
        }
        63 | 80 | 219 | 222 | 247 => {
            /* init_deferred_pred_opt ::= INITIALLY DEFERRED */
            /* ifexists ::= IF EXISTS */
            /* between_op ::= NOT BETWEEN */
            /* in_op ::= NOT IN */
            /* collate ::= COLLATE ID|STRING */
            set!(-1, yy144m, 1);
        }
        64 => {
            /* init_deferred_pred_opt ::= INITIALLY IMMEDIATE */
            set!(-1, yy144m, 0);
        }
        66 => {
            /* tconscomma ::= COMMA */
            db.pp().u1.cr.constraintName.n = 0;
        }
        68 => {
            /* tcons ::= PRIMARY KEY LP sortlist autoinc RP onconf */
            sqlite3AddPrimaryKey(db, M!(-3).yy14(), M!(0).yy144(), M!(-2).yy144(), 0);
        }
        69 => {
            /* tcons ::= UNIQUE LP sortlist RP onconf */
            sqlite3CreateIndex(
                db,
                None,
                None,
                NS,
                M!(-2).yy14(),
                M!(0).yy144(),
                None,
                N,
                0,
                0,
                SQLITE_IDXTYPE_UNIQUE,
            );
        }
        70 => {
            /* tcons ::= CHECK LP expr RP onconf */
            let zSql = parseSql(db);
            sqlite3AddCheckConstraint(
                db,
                M!(-2).yy454(),
                &zSql,
                M!(-3).yy0().off as usize,
                M!(-1).yy0().off as usize,
            );
        }
        71 => {
            /* tcons ::= FOREIGN KEY LP eidlist RP REFERENCES nm eidlist_opt refargs defer_subclause_opt */
            sqlite3CreateForeignKey(
                db,
                M!(-6).yy14(),
                &M!(-3).yy0(),
                M!(-2).yy14(),
                M!(-1).yy144(),
            );
            sqlite3DeferForeignKey(db, M!(0).yy144());
        }
        73 | 75 => {
            /* onconf ::= */
            /* orconf ::= */
            set!(1, yy144m, OE_Default as i32);
        }
        74 => {
            /* onconf ::= ON CONFLICT resolvetype */
            set!(-2, yy144m, M!(0).yy144());
        }
        77 => {
            /* resolvetype ::= IGNORE */
            set!(0, yy144m, OE_Ignore as i32);
        }
        78 | 174 => {
            /* resolvetype ::= REPLACE */
            /* insert_cmd ::= REPLACE */
            set!(0, yy144m, OE_Replace as i32);
        }
        79 => {
            /* cmd ::= DROP TABLE ifexists fullname */
            sqlite3DropTable(db, M!(0).yy203(), 0, M!(-1).yy144());
        }
        82 => {
            /* cmd ::= createkw temp VIEW ifnotexists nm dbnm eidlist_opt AS select */
            sqlite3CreateView(
                db,
                &M!(-8).yy0(),
                &M!(-4).yy0(),
                &M!(-3).yy0(),
                M!(-2).yy14(),
                M!(0).yy555(),
                M!(-7).yy144(),
                M!(-5).yy144(),
            );
        }
        83 => {
            /* cmd ::= DROP VIEW ifexists fullname */
            sqlite3DropTable(db, M!(0).yy203(), 1, M!(-1).yy144());
        }
        84 => {
            /* cmd ::= select */
            let mut dest = SelectDest {
                eDest: SRT_Output,
                ..Default::default()
            };
            let s = M!(0).yy555();
            if (db.mDbFlags & DBFLAG_EncodingFixed) != 0 || sqlite3ReadSchema(db) == SQLITE_OK {
                sqlite3Select(db, s, &mut dest);
            }
            sqlite3SelectDelete(db, s);
        }
        85 => {
            /* select ::= WITH wqlist selectnowith */
            set!(
                -2,
                yy555m,
                attachWithToSelect(db, M!(0).yy555(), M!(-1).yy59())
            );
        }
        86 => {
            /* select ::= WITH RECURSIVE wqlist selectnowith */
            set!(
                -3,
                yy555m,
                attachWithToSelect(db, M!(0).yy555(), M!(-1).yy59())
            );
        }
        87 => {
            /* select ::= selectnowith */
            let p = M!(0).yy555();
            if p.nn() {
                parserDoubleLinkSelect(db, p);
            }
        }
        88 => {
            /* selectnowith ::= selectnowith multiselect_op oneselect */
            let mut pRhs = M!(0).yy555();
            let pLhs = M!(-2).yy555();
            let op = M!(-1).yy144();
            if pRhs.nn() && db.sel(pRhs).pPrior.nn() {
                let x = Token::default();
                parserDoubleLinkSelect(db, pRhs);
                let pFrom = sqlite3SrcListAppendFromTerm(db, NS, None, None, &x, pRhs, None);
                pRhs = sqlite3SelectNew(db, NL, pFrom, N, NL, N, NL, 0, N);
            }
            if pRhs.nn() {
                {
                    let r = db.selm(pRhs);
                    r.op = op as u8;
                    r.pPrior = pLhs;
                }
                if pLhs.nn() {
                    db.selm(pLhs).selFlags &= !SF_MultiValue;
                }
                db.selm(pRhs).selFlags &= !SF_MultiValue;
                if op != TK_ALL as i32 {
                    db.pp().hasCompound = 1;
                }
            } else {
                sqlite3SelectDelete(db, pLhs);
            }
            set!(-2, yy555m, pRhs);
        }
        89 | 91 => {
            /* multiselect_op ::= UNION */
            /* multiselect_op ::= EXCEPT|INTERSECT */
            set!(0, yy144m, MAJ!(0)); /*A-overwrites-OP*/
        }
        90 => {
            /* multiselect_op ::= UNION ALL */
            set!(-1, yy144m, TK_ALL as i32);
        }
        92 => {
            /* oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt orderby_opt limit_opt */
            set!(
                -8,
                yy555m,
                sqlite3SelectNew(
                    db,
                    M!(-6).yy14(),
                    M!(-5).yy203(),
                    M!(-4).yy454(),
                    M!(-3).yy14(),
                    M!(-2).yy454(),
                    M!(-1).yy14(),
                    M!(-7).yy144() as u32,
                    M!(0).yy454()
                )
            );
        }
        93 => {
            /* oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt window_clause orderby_opt limit_opt */
            let s = sqlite3SelectNew(
                db,
                M!(-7).yy14(),
                M!(-6).yy203(),
                M!(-5).yy454(),
                M!(-4).yy14(),
                M!(-3).yy454(),
                M!(-1).yy14(),
                M!(-8).yy144() as u32,
                M!(0).yy454(),
            );
            let w = M!(-2).yy211();
            if s.nn() {
                db.selm(s).pWinDefn = w;
            } else {
                sqlite3WindowListDelete(db, w);
            }
            set!(-9, yy555m, s);
        }
        94 => {
            /* values ::= VALUES LP nexprlist RP */
            set!(
                -3,
                yy555m,
                sqlite3SelectNew(db, M!(-1).yy14(), NS, N, NL, N, NL, SF_Values, N)
            );
        }
        95 => {
            /* oneselect ::= mvalues */
            sqlite3MultiValuesEnd(db, M!(0).yy555());
        }
        96 | 97 => {
            /* mvalues ::= values COMMA LP nexprlist RP */
            /* mvalues ::= mvalues COMMA LP nexprlist RP */
            set!(
                -4,
                yy555m,
                sqlite3MultiValues(db, M!(-4).yy555(), M!(-1).yy14())
            );
        }
        98 => {
            /* distinct ::= DISTINCT */
            set!(0, yy144m, SF_Distinct as i32);
        }
        99 => {
            /* distinct ::= ALL */
            set!(0, yy144m, SF_All as i32);
        }
        101 | 134 | 144 | 234 | 237 | 242 => {
            /* sclp ::= */
            /* orderby_opt ::= */
            /* groupby_opt ::= */
            /* exprlist ::= */
            /* paren_exprlist ::= */
            /* eidlist_opt ::= */
            set!(1, yy14m, NL);
        }
        102 => {
            /* selcollist ::= sclp scanpt expr scanpt as */
            let l = sqlite3ExprListAppend(db, M!(-4).yy14(), M!(-2).yy454());
            set!(-4, yy14m, l);
            let a = M!(0).yy0();
            if a.n > 0 {
                sqlite3ExprListSetName(db, l, &a, 1);
            }
            sqlite3ExprListSetSpan(db, l, M!(-3).yy168(), M!(-1).yy168());
        }
        103 => {
            /* selcollist ::= sclp scanpt STAR */
            let p = sqlite3Expr(db, TK_ASTERISK as i32, None);
            let ofst = M!(0).yy0().off as i32 - zTail!();
            sqlite3ExprSetErrorOffset(db, p, ofst);
            set!(-2, yy14m, sqlite3ExprListAppend(db, M!(-2).yy14(), p));
        }
        104 => {
            /* selcollist ::= sclp scanpt nm DOT STAR */
            let pRight = sqlite3PExpr(db, TK_ASTERISK as i32, N, N);
            let ofst = M!(0).yy0().off as i32 - zTail!();
            sqlite3ExprSetErrorOffset(db, pRight, ofst);
            let pLeft = tokenExpr(db, TK_ID as i32, M!(-2).yy0());
            let pDot = sqlite3PExpr(db, TK_DOT as i32, pLeft, pRight);
            set!(-4, yy14m, sqlite3ExprListAppend(db, M!(-4).yy14(), pDot));
        }
        105 | 117 | 258 | 259 => {
            /* as ::= AS nm */
            /* dbnm ::= DOT nm */
            /* plus_num ::= PLUS INTEGER|FLOAT */
            /* minus_num ::= MINUS INTEGER|FLOAT */
            set!(-1, yy0m, M!(0).yy0());
        }
        107 | 110 => {
            /* from ::= */
            /* stl_prefix ::= */
            set!(1, yy203m, NS);
        }
        108 => {
            /* from ::= FROM seltablist */
            let s = M!(0).yy203();
            set!(-1, yy203m, s);
            sqlite3SrcListShiftJoinType(db, s);
        }
        109 => {
            /* stl_prefix ::= seltablist joinop */
            let s = M!(-1).yy203();
            let jt = M!(0).yy144();
            if s.nn() && db.src(s).nSrc() > 0 {
                let n = db.src(s).a.len();
                db.srcm(s).a[n - 1].fg.jointype = jt as u8;
            }
        }
        111 => {
            /* seltablist ::= stl_prefix nm dbnm as on_using */
            let mut ou = M!(0).yy269();
            let r = sqlite3SrcListAppendFromTerm(
                db,
                M!(-4).yy203(),
                Some(&M!(-3).yy0()),
                Some(&M!(-2).yy0()),
                &M!(-1).yy0(),
                NSEL,
                Some(&mut ou),
            );
            set!(-4, yy203m, r);
        }
        112 => {
            /* seltablist ::= stl_prefix nm dbnm as indexed_by on_using */
            let mut ou = M!(0).yy269();
            let r = sqlite3SrcListAppendFromTerm(
                db,
                M!(-5).yy203(),
                Some(&M!(-4).yy0()),
                Some(&M!(-3).yy0()),
                &M!(-2).yy0(),
                NSEL,
                Some(&mut ou),
            );
            set!(-5, yy203m, r);
            sqlite3SrcListIndexedBy(db, r, &M!(-1).yy0());
        }
        113 => {
            /* seltablist ::= stl_prefix nm dbnm LP exprlist RP as on_using */
            let mut ou = M!(0).yy269();
            let r = sqlite3SrcListAppendFromTerm(
                db,
                M!(-7).yy203(),
                Some(&M!(-6).yy0()),
                Some(&M!(-5).yy0()),
                &M!(-1).yy0(),
                NSEL,
                Some(&mut ou),
            );
            set!(-7, yy203m, r);
            sqlite3SrcListFuncArgs(db, r, M!(-3).yy14());
        }
        114 => {
            /* seltablist ::= stl_prefix LP select RP as on_using */
            let mut ou = M!(0).yy269();
            let r = sqlite3SrcListAppendFromTerm(
                db,
                M!(-5).yy203(),
                None,
                None,
                &M!(-1).yy0(),
                M!(-3).yy555(),
                Some(&mut ou),
            );
            set!(-5, yy203m, r);
        }
        115 => {
            /* seltablist ::= stl_prefix LP seltablist RP as on_using */
            let pA = M!(-5).yy203();
            let pB = M!(-3).yy203();
            let zAs = M!(-1).yy0();
            let mut ou = M!(0).yy269();
            if pA.is_null() && zAs.n == 0 && ou.pOn.is_null() && ou.pUsing.is_null() {
                set!(-5, yy203m, pB);
            } else if pB.nn() && db.src(pB).nSrc() == 1 {
                let r = sqlite3SrcListAppendFromTerm(db, pA, None, None, &zAs, NSEL, Some(&mut ou));
                if r.nn() {
                    let iNew = db.src(r).a.len() - 1;
                    let o = &mut db.srcm(pB).a[0];
                    debug_assert!(o.fg.fixedSchema == 0);
                    let zName = o.zName.take();
                    let isSubquery = o.fg.isSubquery;
                    let mut pSubq = None;
                    let mut zDatabase = None;
                    if isSubquery != 0 {
                        pSubq = o.u4.pSubq.take();
                        o.fg.isSubquery = 0;
                    } else {
                        zDatabase = o.u4.zDatabase.take();
                    }
                    let isTabFunc = o.fg.isTabFunc;
                    let isIndexedBy = o.fg.isIndexedBy;
                    let mut pFuncArg = NL;
                    let mut zIndexedBy = None;
                    if isTabFunc != 0 {
                        pFuncArg = o.u1.pFuncArg;
                        o.u1.pFuncArg = NL;
                        o.fg.isTabFunc = 0;
                    } else if isIndexedBy != 0 {
                        zIndexedBy = o.u1.zIndexedBy.take();
                        o.fg.isIndexedBy = 0;
                    }
                    let notIndexed = o.fg.notIndexed;
                    let nested = match &pSubq {
                        Some(q) => {
                            debug_assert!(q.pSelect.nn());
                            (db.sel(q.pSelect).selFlags & SF_NestedFrom) != 0
                        }
                        None => false,
                    };
                    let n = &mut db.srcm(r).a[iNew];
                    n.zName = zName;
                    if isSubquery != 0 {
                        n.fg.isSubquery = 1;
                        n.u4.pSubq = pSubq;
                        if nested {
                            n.fg.isNestedFrom = 1;
                        }
                    } else {
                        n.u4.zDatabase = zDatabase;
                    }
                    if isTabFunc != 0 {
                        n.u1.pFuncArg = pFuncArg;
                        n.fg.isTabFunc = 1;
                    } else if isIndexedBy != 0 {
                        n.u1.zIndexedBy = zIndexedBy;
                        n.fg.isIndexedBy = 1;
                    }
                    n.fg.notIndexed = notIndexed;
                    if IN_RENAME_OBJECT!() {
                        /* C moves the zName pointer, which is the rename key */
                        sqlite3RenameTokenRemap(
                            db,
                            Some(RenameKey::SrcItemName(SrcItemId(r, iNew as u32))),
                            RenameKey::SrcItemName(SrcItemId(pB, 0)),
                        );
                    }
                }
                sqlite3SrcListDelete(db, pB);
                set!(-5, yy203m, r);
            } else {
                sqlite3SrcListShiftJoinType(db, pB);
                let pSubquery = sqlite3SelectNew(db, NL, pB, N, NL, N, NL, SF_NestedFrom, N);
                let r = sqlite3SrcListAppendFromTerm(
                    db,
                    pA,
                    None,
                    None,
                    &zAs,
                    pSubquery,
                    Some(&mut ou),
                );
                set!(-5, yy203m, r);
            }
        }
        116 | 131 => {
            /* dbnm ::= */
            /* indexed_opt ::= */
            let t = M!(1).yy0m();
            t.z = None;
            t.off = 0;
            t.n = 0;
        }
        118 | 120 => {
            /* fullname ::= nm */
            /* xfullname ::= nm */
            let t = M!(0).yy0();
            let l = sqlite3SrcListAppend(db, NS, Some(&t), None);
            if IN_RENAME_OBJECT!() && l.nn() {
                sqlite3RenameTokenMap(db, RenameKey::SrcItemName(SrcItemId(l, 0)), &t);
            }
            set!(0, yy203m, l);
        }
        119 | 121 => {
            /* fullname ::= nm DOT nm */
            /* xfullname ::= nm DOT nm */
            let t = M!(0).yy0();
            let l = sqlite3SrcListAppend(db, NS, Some(&M!(-2).yy0()), Some(&t));
            if IN_RENAME_OBJECT!() && l.nn() {
                sqlite3RenameTokenMap(db, RenameKey::SrcItemName(SrcItemId(l, 0)), &t);
            }
            set!(-2, yy203m, l);
        }
        122 => {
            /* xfullname ::= nm AS nm */
            let t = M!(-2).yy0();
            let l = sqlite3SrcListAppend(db, NS, Some(&t), None);
            if l.nn() {
                if IN_RENAME_OBJECT!() {
                    sqlite3RenameTokenMap(db, RenameKey::SrcItemName(SrcItemId(l, 0)), &t);
                } else {
                    let z = sqlite3NameFromToken(db, &M!(0).yy0());
                    db.srcm(l).a[0].zAlias = z;
                }
            }
            set!(-2, yy203m, l);
        }
        123 => {
            /* xfullname ::= nm DOT nm AS nm */
            let t = M!(-2).yy0();
            let l = sqlite3SrcListAppend(db, NS, Some(&M!(-4).yy0()), Some(&t));
            if l.nn() {
                if IN_RENAME_OBJECT!() {
                    sqlite3RenameTokenMap(db, RenameKey::SrcItemName(SrcItemId(l, 0)), &t);
                } else {
                    let z = sqlite3NameFromToken(db, &M!(0).yy0());
                    db.srcm(l).a[0].zAlias = z;
                }
            }
            set!(-4, yy203m, l);
        }
        124 => {
            /* joinop ::= COMMA|JOIN */
            set!(0, yy144m, JT_INNER as i32);
        }
        125 => {
            /* joinop ::= JOIN_KW JOIN */
            set!(-1, yy144m, sqlite3JoinType(db, &M!(-1).yy0(), None, None)); /*X-overwrites-A*/
        }
        126 => {
            /* joinop ::= JOIN_KW nm JOIN */
            set!(
                -2,
                yy144m,
                sqlite3JoinType(db, &M!(-2).yy0(), Some(&M!(-1).yy0()), None)
            ); /*X-overwrites-A*/
        }
        127 => {
            /* joinop ::= JOIN_KW nm nm JOIN */
            set!(
                -3,
                yy144m,
                sqlite3JoinType(db, &M!(-3).yy0(), Some(&M!(-2).yy0()), Some(&M!(-1).yy0()))
            ); /*X-overwrites-A*/
        }
        128 => {
            /* on_using ::= ON expr */
            set!(
                -1,
                yy269m,
                OnOrUsing {
                    pOn: M!(0).yy454(),
                    pUsing: IdListId::NULL
                }
            );
        }
        129 => {
            /* on_using ::= USING LP idlist RP */
            set!(
                -3,
                yy269m,
                OnOrUsing {
                    pOn: N,
                    pUsing: M!(-1).yy132()
                }
            );
        }
        130 => {
            /* on_using ::= */
            set!(1, yy269m, OnOrUsing::default());
        }
        132 => {
            /* indexed_by ::= INDEXED BY nm */
            set!(-2, yy0m, M!(0).yy0());
        }
        133 => {
            /* indexed_by ::= NOT INDEXED */
            let t = M!(-1).yy0m();
            t.z = None;
            t.off = 0;
            t.n = 1;
        }
        135 | 145 => {
            /* orderby_opt ::= ORDER BY sortlist */
            /* groupby_opt ::= GROUP BY nexprlist */
            set!(-2, yy14m, M!(0).yy14());
        }
        136 => {
            /* sortlist ::= sortlist COMMA expr sortorder nulls */
            let l = sqlite3ExprListAppend(db, M!(-4).yy14(), M!(-2).yy454());
            set!(-4, yy14m, l);
            sqlite3ExprListSetSortOrder(db, l, M!(-1).yy144(), M!(0).yy144());
        }
        137 => {
            /* sortlist ::= expr sortorder nulls */
            let l = sqlite3ExprListAppend(db, NL, M!(-2).yy454()); /*A-overwrites-Y*/
            set!(-2, yy14m, l);
            sqlite3ExprListSetSortOrder(db, l, M!(-1).yy144(), M!(0).yy144());
        }
        138 => {
            /* sortorder ::= ASC */
            set!(0, yy144m, SQLITE_SO_ASC as i32);
        }
        139 => {
            /* sortorder ::= DESC */
            set!(0, yy144m, SQLITE_SO_DESC as i32);
        }
        140 | 143 => {
            /* sortorder ::= */
            /* nulls ::= */
            set!(1, yy144m, SQLITE_SO_UNDEFINED as i32);
        }
        141 => {
            /* nulls ::= NULLS FIRST */
            set!(-1, yy144m, SQLITE_SO_ASC as i32);
        }
        142 => {
            /* nulls ::= NULLS LAST */
            set!(-1, yy144m, SQLITE_SO_DESC as i32);
        }
        146 | 148 | 153 | 155 | 232 | 233 | 252 => {
            /* having_opt ::= */
            /* limit_opt ::= */
            /* where_opt ::= */
            /* where_opt_ret ::= */
            /* case_else ::= */
            /* case_operand ::= */
            /* vinto ::= */
            set!(1, yy454m, N);
        }
        147 | 154 | 156 | 231 | 251 => {
            /* having_opt ::= HAVING expr */
            /* where_opt ::= WHERE expr */
            /* where_opt_ret ::= WHERE expr */
            /* case_else ::= ELSE expr */
            /* vinto ::= INTO expr */
            set!(-1, yy454m, M!(0).yy454());
        }
        149 => {
            /* limit_opt ::= LIMIT expr */
            set!(
                -1,
                yy454m,
                sqlite3PExpr(db, TK_LIMIT as i32, M!(0).yy454(), N)
            );
        }
        150 => {
            /* limit_opt ::= LIMIT expr OFFSET expr */
            set!(
                -3,
                yy454m,
                sqlite3PExpr(db, TK_LIMIT as i32, M!(-2).yy454(), M!(0).yy454())
            );
        }
        151 => {
            /* limit_opt ::= LIMIT expr COMMA expr */
            set!(
                -3,
                yy454m,
                sqlite3PExpr(db, TK_LIMIT as i32, M!(0).yy454(), M!(-2).yy454())
            );
        }
        152 => {
            /* cmd ::= with DELETE FROM xfullname indexed_opt where_opt_ret */
            sqlite3SrcListIndexedBy(db, M!(-2).yy203(), &M!(-1).yy0());
            sqlite3DeleteFrom(db, M!(-2).yy203(), M!(0).yy454(), NL, N);
        }
        157 => {
            /* where_opt_ret ::= RETURNING selcollist */
            sqlite3AddReturning(db, M!(0).yy14());
            set!(-1, yy454m, N);
        }
        158 => {
            /* where_opt_ret ::= WHERE expr RETURNING selcollist */
            sqlite3AddReturning(db, M!(0).yy14());
            set!(-3, yy454m, M!(-2).yy454());
        }
        159 => {
            /* cmd ::= with UPDATE orconf xfullname indexed_opt SET setlist from where_opt_ret */
            sqlite3SrcListIndexedBy(db, M!(-5).yy203(), &M!(-4).yy0());
            sqlite3ExprListCheckLength(db, M!(-2).yy14(), Some(b"set list"));
            let pFrom = M!(-1).yy203();
            if pFrom.nn() {
                let mut pFromClause = pFrom;
                if db.src(pFromClause).nSrc() > 1 {
                    let pSubquery =
                        sqlite3SelectNew(db, NL, pFromClause, N, NL, N, NL, SF_NestedFrom, N);
                    let as_ = Token::default();
                    pFromClause =
                        sqlite3SrcListAppendFromTerm(db, NS, None, None, &as_, pSubquery, None);
                }
                set!(
                    -5,
                    yy203m,
                    sqlite3SrcListAppendList(db, M!(-5).yy203(), pFromClause)
                );
            }
            sqlite3Update(
                db,
                M!(-5).yy203(),
                M!(-2).yy14(),
                M!(0).yy454(),
                M!(-6).yy144(),
                NL,
                N,
                UpsertId::NULL,
            );
        }
        160 => {
            /* setlist ::= setlist COMMA nm EQ expr */
            let l = sqlite3ExprListAppend(db, M!(-4).yy14(), M!(0).yy454());
            set!(-4, yy14m, l);
            sqlite3ExprListSetName(db, l, &M!(-2).yy0(), 1);
        }
        161 => {
            /* setlist ::= setlist COMMA LP idlist RP EQ expr */
            set!(
                -6,
                yy14m,
                sqlite3ExprListAppendVector(db, M!(-6).yy14(), M!(-3).yy132(), M!(0).yy454())
            );
        }
        162 => {
            /* setlist ::= nm EQ expr */
            let l = sqlite3ExprListAppend(db, NL, M!(0).yy454());
            sqlite3ExprListSetName(db, l, &M!(-2).yy0(), 1);
            set!(-2, yy14m, l);
        }
        163 => {
            /* setlist ::= LP idlist RP EQ expr */
            set!(
                -4,
                yy14m,
                sqlite3ExprListAppendVector(db, NL, M!(-3).yy132(), M!(0).yy454())
            );
        }
        164 => {
            /* cmd ::= with insert_cmd INTO xfullname idlist_opt select upsert */
            sqlite3Insert(
                db,
                M!(-3).yy203(),
                M!(-1).yy555(),
                M!(-2).yy132(),
                M!(-5).yy144(),
                M!(0).yy122(),
            );
        }
        165 => {
            /* cmd ::= with insert_cmd INTO xfullname idlist_opt DEFAULT VALUES returning */
            sqlite3Insert(
                db,
                M!(-4).yy203(),
                NSEL,
                M!(-3).yy132(),
                M!(-6).yy144(),
                UpsertId::NULL,
            );
        }
        166 => {
            /* upsert ::= */
            set!(1, yy122m, UpsertId::NULL);
        }
        167 => {
            /* upsert ::= RETURNING selcollist */
            set!(-1, yy122m, UpsertId::NULL);
            sqlite3AddReturning(db, M!(0).yy14());
        }
        168 => {
            /* upsert ::= ON CONFLICT LP sortlist RP where_opt DO UPDATE SET setlist where_opt upsert */
            set!(
                -11,
                yy122m,
                sqlite3UpsertNew(
                    db,
                    M!(-8).yy14(),
                    M!(-6).yy454(),
                    M!(-2).yy14(),
                    M!(-1).yy454(),
                    M!(0).yy122()
                )
            );
        }
        169 => {
            /* upsert ::= ON CONFLICT LP sortlist RP where_opt DO NOTHING upsert */
            set!(
                -8,
                yy122m,
                sqlite3UpsertNew(db, M!(-5).yy14(), M!(-3).yy454(), NL, N, M!(0).yy122())
            );
        }
        170 => {
            /* upsert ::= ON CONFLICT DO NOTHING returning */
            set!(
                -4,
                yy122m,
                sqlite3UpsertNew(db, NL, N, NL, N, UpsertId::NULL)
            );
        }
        171 => {
            /* upsert ::= ON CONFLICT DO UPDATE SET setlist where_opt returning */
            set!(
                -7,
                yy122m,
                sqlite3UpsertNew(db, NL, N, M!(-2).yy14(), M!(-1).yy454(), UpsertId::NULL)
            );
        }
        172 => {
            /* returning ::= RETURNING selcollist */
            sqlite3AddReturning(db, M!(0).yy14());
        }
        175 => {
            /* idlist_opt ::= */
            set!(1, yy132m, IdListId::NULL);
        }
        176 => {
            /* idlist_opt ::= LP idlist RP */
            set!(-2, yy132m, M!(-1).yy132());
        }
        177 => {
            /* idlist ::= idlist COMMA nm */
            set!(
                -2,
                yy132m,
                sqlite3IdListAppend(db, M!(-2).yy132(), &M!(0).yy0())
            );
        }
        178 => {
            /* idlist ::= nm */
            set!(
                0,
                yy132m,
                sqlite3IdListAppend(db, IdListId::NULL, &M!(0).yy0())
            ); /*A-overwrites-Y*/
        }
        179 => {
            /* expr ::= LP expr RP */
            set!(-2, yy454m, M!(-1).yy454());
        }
        180 => {
            /* expr ::= ID|INDEXED|JOIN_KW */
            set!(0, yy454m, tokenExpr(db, TK_ID as i32, M!(0).yy0())); /*A-overwrites-X*/
        }
        181 => {
            /* expr ::= nm DOT nm */
            let temp1 = tokenExpr(db, TK_ID as i32, M!(-2).yy0());
            let temp2 = tokenExpr(db, TK_ID as i32, M!(0).yy0());
            set!(-2, yy454m, sqlite3PExpr(db, TK_DOT as i32, temp1, temp2));
        }
        182 => {
            /* expr ::= nm DOT nm DOT nm */
            let temp1 = tokenExpr(db, TK_ID as i32, M!(-4).yy0());
            let temp2 = tokenExpr(db, TK_ID as i32, M!(-2).yy0());
            let temp3 = tokenExpr(db, TK_ID as i32, M!(0).yy0());
            let temp4 = sqlite3PExpr(db, TK_DOT as i32, temp2, temp3);
            if IN_RENAME_OBJECT!() {
                sqlite3RenameTokenRemap(db, None, RenameKey::Expr(temp1));
            }
            set!(-4, yy454m, sqlite3PExpr(db, TK_DOT as i32, temp1, temp4));
        }
        183 | 184 => {
            /* term ::= NULL|FLOAT|BLOB */
            /* term ::= STRING */
            set!(0, yy454m, tokenExpr(db, MAJ!(0), M!(0).yy0())); /*A-overwrites-X*/
        }
        185 => {
            /* term ::= INTEGER */
            let t = M!(0).yy0();
            let mut iValue: i32 = 0;
            let e = if sqlite3GetInt32(Some(tok_rest(&t)), &mut iValue) == 0 {
                sqlite3ExprAlloc(db, TK_INTEGER as i32, &t, 0)
            } else {
                sqlite3ExprInt32(db, iValue)
            };
            if e.nn() {
                db.exm(e).w.iOfst = t.off as i32 - zTail!();
            }
            set!(0, yy454m, e);
        }
        186 => {
            /* expr ::= VARIABLE */
            let t = M!(0).yy0(); /*A-overwrites-X*/
            if !(t.at(0) == b'#' && sqlite3Isdigit(t.at(1))) {
                let n = t.n;
                let e = tokenExpr(db, TK_VARIABLE as i32, t);
                set!(0, yy454m, e);
                sqlite3ExprAssignVarNumber(db, e, n);
            } else {
                /* When doing a nested parse, one can include terms in an expression
                 ** that look like this:   #1 #2 ...  These terms refer to registers
                 ** in the virtual machine.  #N is the N-th register. */
                debug_assert!(t.n >= 2);
                if db.ppr().nested == 0 {
                    parserSyntaxError(db, &t);
                    set!(0, yy454m, N);
                } else {
                    let e = sqlite3PExpr(db, TK_REGISTER as i32, N, N);
                    set!(0, yy454m, e);
                    if e.nn() {
                        let mut v = db.ex(e).iTable;
                        let rest = tok_rest(&t);
                        sqlite3GetInt32(Some(&rest[1.min(rest.len())..]), &mut v);
                        db.exm(e).iTable = v;
                    }
                }
            }
        }
        187 => {
            /* expr ::= expr COLLATE ID|STRING */
            set!(
                -2,
                yy454m,
                sqlite3ExprAddCollateToken(db, M!(-2).yy454(), &M!(0).yy0(), 1)
            );
        }
        188 => {
            /* expr ::= CAST LP expr AS typetoken RP */
            let e = sqlite3ExprAlloc(db, TK_CAST as i32, &M!(-1).yy0(), 1);
            sqlite3ExprAttachSubtrees(db, e, M!(-3).yy454(), N);
            set!(-5, yy454m, e);
        }
        189 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP */
            set!(
                -4,
                yy454m,
                sqlite3ExprFunction(db, M!(-1).yy14(), &M!(-4).yy0(), M!(-2).yy144())
            );
        }
        190 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP */
            let e = sqlite3ExprFunction(db, M!(-4).yy14(), &M!(-7).yy0(), M!(-5).yy144());
            sqlite3ExprAddFunctionOrderBy(db, e, M!(-1).yy14());
            set!(-7, yy454m, e);
        }
        191 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP STAR RP */
            set!(-3, yy454m, sqlite3ExprFunction(db, NL, &M!(-3).yy0(), 0));
        }
        192 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP filter_over */
            let e = sqlite3ExprFunction(db, M!(-2).yy14(), &M!(-5).yy0(), M!(-3).yy144());
            sqlite3WindowAttach(db, e, M!(0).yy211());
            set!(-5, yy454m, e);
        }
        193 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP filter_over */
            let e = sqlite3ExprFunction(db, M!(-5).yy14(), &M!(-8).yy0(), M!(-6).yy144());
            sqlite3WindowAttach(db, e, M!(0).yy211());
            sqlite3ExprAddFunctionOrderBy(db, e, M!(-2).yy14());
            set!(-8, yy454m, e);
        }
        194 => {
            /* expr ::= ID|INDEXED|JOIN_KW LP STAR RP filter_over */
            let e = sqlite3ExprFunction(db, NL, &M!(-4).yy0(), 0);
            sqlite3WindowAttach(db, e, M!(0).yy211());
            set!(-4, yy454m, e);
        }
        195 => {
            /* term ::= CTIME_KW */
            set!(0, yy454m, sqlite3ExprFunction(db, NL, &M!(0).yy0(), 0));
        }
        196 => {
            /* expr ::= LP nexprlist COMMA expr RP */
            let pList = sqlite3ExprListAppend(db, M!(-3).yy14(), M!(-1).yy454());
            let e = sqlite3PExpr(db, TK_VECTOR as i32, N, N);
            if e.nn() {
                db.exm(e).x.pList = pList;
                let n = db.el(pList).a.len();
                for i in 0..n {
                    let pe = db.el(pList).a[i].pExpr;
                    debug_assert!(pe.nn());
                    let f = db.ex(pe).flags & EP_Propagate;
                    db.exm(e).flags |= f;
                }
            } else {
                sqlite3ExprListDelete(db, pList);
            }
            set!(-4, yy454m, e);
        }
        197 => {
            /* expr ::= expr AND expr */
            set!(
                -2,
                yy454m,
                sqlite3ExprAnd(db, M!(-2).yy454(), M!(0).yy454())
            );
        }
        198 | 199 | 200 | 201 | 202 | 203 | 204 => {
            /* expr ::= expr OR expr */
            /* expr ::= expr LT|GT|GE|LE expr */
            /* expr ::= expr EQ|NE expr */
            /* expr ::= expr BITAND|BITOR|LSHIFT|RSHIFT expr */
            /* expr ::= expr PLUS|MINUS expr */
            /* expr ::= expr STAR|SLASH|REM expr */
            /* expr ::= expr CONCAT expr */
            set!(
                -2,
                yy454m,
                sqlite3PExpr(db, MAJ!(-1), M!(-2).yy454(), M!(0).yy454())
            );
        }
        205 => {
            /* likeop ::= NOT LIKE_KW|MATCH */
            let t = M!(0).yy0();
            let a = M!(-1).yy0m();
            *a = t;
            a.n |= 0x80000000; /*A-overwrite-X*/
        }
        206 => {
            /* expr ::= expr likeop expr */
            let bNot = M!(-1).yy0().n & 0x80000000;
            M!(-1).yy0m().n &= 0x7fffffff;
            let mut pList = sqlite3ExprListAppend(db, NL, M!(0).yy454());
            pList = sqlite3ExprListAppend(db, pList, M!(-2).yy454());
            let mut e = sqlite3ExprFunction(db, pList, &M!(-1).yy0(), 0);
            if bNot != 0 {
                e = sqlite3PExpr(db, TK_NOT as i32, e, N);
            }
            if e.nn() {
                db.exm(e).flags |= EP_InfixFunc;
            }
            set!(-2, yy454m, e);
        }
        207 => {
            /* expr ::= expr likeop expr ESCAPE expr */
            let bNot = M!(-3).yy0().n & 0x80000000;
            M!(-3).yy0m().n &= 0x7fffffff;
            let mut pList = sqlite3ExprListAppend(db, NL, M!(-2).yy454());
            pList = sqlite3ExprListAppend(db, pList, M!(-4).yy454());
            pList = sqlite3ExprListAppend(db, pList, M!(0).yy454());
            let mut e = sqlite3ExprFunction(db, pList, &M!(-3).yy0(), 0);
            if bNot != 0 {
                e = sqlite3PExpr(db, TK_NOT as i32, e, N);
            }
            if e.nn() {
                db.exm(e).flags |= EP_InfixFunc;
            }
            set!(-4, yy454m, e);
        }
        208 => {
            /* expr ::= expr ISNULL|NOTNULL */
            set!(-1, yy454m, sqlite3PExprIsNull(db, MAJ!(0), M!(-1).yy454()));
        }
        209 => {
            /* expr ::= expr NOT NULL */
            set!(
                -2,
                yy454m,
                sqlite3PExprIsNull(db, TK_NOTNULL as i32, M!(-2).yy454())
            );
        }
        210 => {
            /* expr ::= expr IS expr */
            set!(
                -2,
                yy454m,
                sqlite3PExprIs(db, TK_IS as i32, M!(-2).yy454(), M!(0).yy454())
            );
        }
        211 => {
            /* expr ::= expr IS NOT expr */
            set!(
                -3,
                yy454m,
                sqlite3PExprIs(db, TK_ISNOT as i32, M!(-3).yy454(), M!(0).yy454())
            );
        }
        212 => {
            /* expr ::= expr IS NOT DISTINCT FROM expr */
            set!(
                -5,
                yy454m,
                sqlite3PExprIs(db, TK_IS as i32, M!(-5).yy454(), M!(0).yy454())
            );
        }
        213 => {
            /* expr ::= expr IS DISTINCT FROM expr */
            set!(
                -4,
                yy454m,
                sqlite3PExprIs(db, TK_ISNOT as i32, M!(-4).yy454(), M!(0).yy454())
            );
        }
        214 | 215 => {
            /* expr ::= NOT expr */
            /* expr ::= BITNOT expr */
            set!(-1, yy454m, sqlite3PExpr(db, MAJ!(-1), M!(0).yy454(), N)); /*A-overwrites-B*/
        }
        216 => {
            /* expr ::= PLUS|MINUS expr */
            let p = M!(0).yy454();
            let op = (MAJ!(-1) + (TK_UPLUS as i32 - TK_PLUS as i32)) as u8;
            debug_assert!(TK_UPLUS > TK_PLUS);
            debug_assert!(TK_UMINUS == TK_MINUS + (TK_UPLUS - TK_PLUS));
            if p.nn() && db.ex(p).op == TK_UPLUS {
                db.exm(p).op = op;
                set!(-1, yy454m, p);
            } else {
                set!(-1, yy454m, sqlite3PExpr(db, op as i32, p, N));
                /*A-overwrites-B*/
            }
        }
        217 => {
            /* expr ::= expr PTR expr */
            let mut pList = sqlite3ExprListAppend(db, NL, M!(-2).yy454());
            pList = sqlite3ExprListAppend(db, pList, M!(0).yy454());
            set!(-2, yy454m, sqlite3ExprFunction(db, pList, &M!(-1).yy0(), 0));
        }
        218 | 221 => {
            /* between_op ::= BETWEEN */
            /* in_op ::= IN */
            set!(0, yy144m, 0);
        }
        220 => {
            /* expr ::= expr between_op expr AND expr */
            let mut pList = sqlite3ExprListAppend(db, NL, M!(-2).yy454());
            pList = sqlite3ExprListAppend(db, pList, M!(0).yy454());
            let mut e = sqlite3PExpr(db, TK_BETWEEN as i32, M!(-4).yy454(), N);
            if e.nn() {
                db.exm(e).x.pList = pList;
                sqlite3ExprSetHeightAndFlags(db, e);
            } else {
                sqlite3ExprListDelete(db, pList);
            }
            if M!(-3).yy144() != 0 {
                e = sqlite3PExpr(db, TK_NOT as i32, e, N);
            }
            set!(-4, yy454m, e);
        }
        223 => {
            /* expr ::= expr in_op LP exprlist RP */
            let pL = M!(-1).yy14();
            let lhs = M!(-4).yy454();
            let bNot = M!(-3).yy144();
            if pL.is_null() {
                /* Expressions of the form
                 **
                 **      expr1 IN ()
                 **      expr1 NOT IN ()
                 **
                 ** simplify to constants 0 (false) and 1 (true), respectively.
                 **
                 ** Except, do not apply this optimization if expr1 contains a function
                 ** because that function might be an aggregate (we don't know yet whether
                 ** it is or not) and if it is an aggregate, that could change the meaning
                 ** of the whole query.
                 */
                let pB = sqlite3Expr(
                    db,
                    TK_STRING as i32,
                    Some(if bNot != 0 {
                        &b"true"[..]
                    } else {
                        &b"false"[..]
                    }),
                );
                if pB.nn() {
                    sqlite3ExprIdToTrueFalse(db, pB);
                }
                let hasFunc = lhs.nn() && db.ex(lhs).HasProperty(EP_HasFunc);
                if !hasFunc {
                    sqlite3ExprUnmapAndDelete(db, lhs);
                    set!(-4, yy454m, pB);
                } else {
                    set!(
                        -4,
                        yy454m,
                        sqlite3PExpr(
                            db,
                            if bNot != 0 {
                                TK_OR as i32
                            } else {
                                TK_AND as i32
                            },
                            pB,
                            lhs
                        )
                    );
                }
            } else {
                let mut pRHS = db.el(pL).a[0].pExpr;
                let nExpr = db.el(pL).nExpr();
                let mut e;
                if nExpr == 1
                    && sqlite3ExprIsConstant(db, true, pRHS) != 0
                    && eop(db, pRHS) != TK_COLLATE
                    && eop(db, lhs) != TK_VECTOR
                {
                    db.elm(pL).a[0].pExpr = N;
                    sqlite3ExprListDelete(db, pL);
                    pRHS = sqlite3PExpr(db, TK_UPLUS as i32, pRHS, N);
                    e = sqlite3PExpr(db, TK_EQ as i32, lhs, pRHS);
                } else if nExpr == 1 && eop(db, pRHS) == TK_SELECT {
                    e = sqlite3PExpr(db, TK_IN as i32, lhs, N);
                    let s = db.ex(pRHS).x.pSelect;
                    sqlite3PExprAddSelect(db, e, s);
                    db.exm(pRHS).x.pSelect = NSEL;
                    sqlite3ExprListDelete(db, pL);
                } else {
                    e = sqlite3PExpr(db, TK_IN as i32, lhs, N);
                    if e.is_null() {
                        sqlite3ExprListDelete(db, pL);
                    } else if eop(db, db.ex(e).pLeft) == TK_VECTOR {
                        let pLeft = db.ex(e).pLeft;
                        let nE = db.el(db.ex(pLeft).x.pList).nExpr();
                        let pSelectRHS = sqlite3ExprListToValues(db, nE, pL);
                        if pSelectRHS.nn() {
                            parserDoubleLinkSelect(db, pSelectRHS);
                            sqlite3PExprAddSelect(db, e, pSelectRHS);
                        }
                    } else {
                        db.exm(e).x.pList = pL;
                        sqlite3ExprSetHeightAndFlags(db, e);
                    }
                }
                if bNot != 0 {
                    e = sqlite3PExpr(db, TK_NOT as i32, e, N);
                }
                set!(-4, yy454m, e);
            }
        }
        224 => {
            /* expr ::= LP select RP */
            let e = sqlite3PExpr(db, TK_SELECT as i32, N, N);
            sqlite3PExprAddSelect(db, e, M!(-1).yy555());
            set!(-2, yy454m, e);
        }
        225 => {
            /* expr ::= expr in_op LP select RP */
            let mut e = sqlite3PExpr(db, TK_IN as i32, M!(-4).yy454(), N);
            sqlite3PExprAddSelect(db, e, M!(-1).yy555());
            if M!(-3).yy144() != 0 {
                e = sqlite3PExpr(db, TK_NOT as i32, e, N);
            }
            set!(-4, yy454m, e);
        }
        226 => {
            /* expr ::= expr in_op nm dbnm paren_exprlist */
            let pSrc = sqlite3SrcListAppend(db, NS, Some(&M!(-2).yy0()), Some(&M!(-1).yy0()));
            let pSelect = sqlite3SelectNew(db, NL, pSrc, N, NL, N, NL, 0, N);
            let pArgs = M!(0).yy14();
            if pArgs.nn() {
                sqlite3SrcListFuncArgs(db, if pSelect.nn() { pSrc } else { NS }, pArgs);
            }
            let mut e = sqlite3PExpr(db, TK_IN as i32, M!(-4).yy454(), N);
            sqlite3PExprAddSelect(db, e, pSelect);
            if M!(-3).yy144() != 0 {
                e = sqlite3PExpr(db, TK_NOT as i32, e, N);
            }
            set!(-4, yy454m, e);
        }
        227 => {
            /* expr ::= EXISTS LP select RP */
            let p = sqlite3PExpr(db, TK_EXISTS as i32, N, N);
            set!(-3, yy454m, p);
            sqlite3PExprAddSelect(db, p, M!(-1).yy555());
        }
        228 => {
            /* expr ::= CASE case_operand case_exprlist case_else END */
            let e = sqlite3PExpr(db, TK_CASE as i32, M!(-3).yy454(), N);
            let pList = M!(-2).yy14();
            let pElse = M!(-1).yy454();
            if e.nn() {
                let l = if pElse.nn() {
                    sqlite3ExprListAppend(db, pList, pElse)
                } else {
                    pList
                };
                db.exm(e).x.pList = l;
                sqlite3ExprSetHeightAndFlags(db, e);
            } else {
                sqlite3ExprListDelete(db, pList);
                sqlite3ExprDelete(db, pElse);
            }
            set!(-4, yy454m, e);
        }
        229 => {
            /* case_exprlist ::= case_exprlist WHEN expr THEN expr */
            let mut l = sqlite3ExprListAppend(db, M!(-4).yy14(), M!(-2).yy454());
            l = sqlite3ExprListAppend(db, l, M!(0).yy454());
            set!(-4, yy14m, l);
        }
        230 => {
            /* case_exprlist ::= WHEN expr THEN expr */
            let mut l = sqlite3ExprListAppend(db, NL, M!(-2).yy454());
            l = sqlite3ExprListAppend(db, l, M!(0).yy454());
            set!(-3, yy14m, l);
        }
        235 => {
            /* nexprlist ::= nexprlist COMMA expr */
            set!(
                -2,
                yy14m,
                sqlite3ExprListAppend(db, M!(-2).yy14(), M!(0).yy454())
            );
        }
        236 => {
            /* nexprlist ::= expr */
            set!(0, yy14m, sqlite3ExprListAppend(db, NL, M!(0).yy454())); /*A-overwrites-Y*/
        }
        238 | 243 => {
            /* paren_exprlist ::= LP exprlist RP */
            /* eidlist_opt ::= LP eidlist RP */
            set!(-2, yy14m, M!(-1).yy14());
        }
        239 => {
            /* cmd ::= createkw uniqueflag INDEX ifnotexists nm dbnm ON nm LP sortlist RP where_opt */
            let tTab = M!(-4).yy0();
            let pTblName = sqlite3SrcListAppend(db, NS, Some(&tTab), None);
            sqlite3CreateIndex(
                db,
                Some(&M!(-7).yy0()),
                Some(&M!(-6).yy0()),
                pTblName,
                M!(-2).yy14(),
                M!(-10).yy144(),
                Some(&M!(-11).yy0()),
                M!(0).yy454(),
                SQLITE_SO_ASC as i32,
                M!(-8).yy144(),
                SQLITE_IDXTYPE_APPDEF,
            );
            if IN_RENAME_OBJECT!() {
                if let Some(idx) = db.ppr().pNewIndex.clone() {
                    sqlite3RenameTokenMap(db, crate::alter::rename_key_index_name(&idx), &tTab);
                }
            }
        }
        240 | 281 => {
            /* uniqueflag ::= UNIQUE */
            /* raisetype ::= ABORT */
            set!(0, yy144m, OE_Abort as i32);
        }
        241 => {
            /* uniqueflag ::= */
            set!(1, yy144m, OE_None as i32);
        }
        244 => {
            /* eidlist ::= eidlist COMMA nm collate sortorder */
            set!(
                -4,
                yy14m,
                parserAddExprIdListTerm(
                    db,
                    M!(-4).yy14(),
                    &M!(-2).yy0(),
                    M!(-1).yy144(),
                    M!(0).yy144()
                )
            );
        }
        245 => {
            /* eidlist ::= nm collate sortorder */
            set!(
                -2,
                yy14m,
                parserAddExprIdListTerm(db, NL, &M!(-2).yy0(), M!(-1).yy144(), M!(0).yy144())
            ); /*A-overwrites-Y*/
        }
        248 => {
            /* cmd ::= DROP INDEX ifexists fullname */
            sqlite3DropIndex(db, M!(0).yy203(), M!(-1).yy144());
        }
        249 => {
            /* cmd ::= VACUUM vinto */
            sqlite3Vacuum(db, None, M!(0).yy454());
        }
        250 => {
            /* cmd ::= VACUUM nm vinto */
            sqlite3Vacuum(db, Some(&M!(-1).yy0()), M!(0).yy454());
        }
        253 => {
            /* cmd ::= PRAGMA nm dbnm */
            sqlite3Pragma(db, &M!(-1).yy0(), &M!(0).yy0(), None, 0);
        }
        254 => {
            /* cmd ::= PRAGMA nm dbnm EQ nmnum */
            sqlite3Pragma(db, &M!(-3).yy0(), &M!(-2).yy0(), Some(&M!(0).yy0()), 0);
        }
        255 => {
            /* cmd ::= PRAGMA nm dbnm LP nmnum RP */
            sqlite3Pragma(db, &M!(-4).yy0(), &M!(-3).yy0(), Some(&M!(-1).yy0()), 0);
        }
        256 => {
            /* cmd ::= PRAGMA nm dbnm EQ minus_num */
            sqlite3Pragma(db, &M!(-3).yy0(), &M!(-2).yy0(), Some(&M!(0).yy0()), 1);
        }
        257 => {
            /* cmd ::= PRAGMA nm dbnm LP minus_num RP */
            sqlite3Pragma(db, &M!(-4).yy0(), &M!(-3).yy0(), Some(&M!(-1).yy0()), 1);
        }
        260 => {
            /* cmd ::= createkw trigger_decl BEGIN trigger_cmd_list END */
            let a = M!(-3).yy0();
            let e = M!(0).yy0();
            let all = Token {
                z: a.z.clone(),
                off: a.off,
                n: (e.off - a.off) + e.n,
            };
            sqlite3FinishTrigger(db, M!(-1).yy427(), &all);
        }
        261 => {
            /* trigger_decl ::= temp TRIGGER ifnotexists nm dbnm trigger_time trigger_event ON fullname foreach_clause when_clause */
            let ev = M!(-4).yy286();
            let n1 = M!(-7).yy0();
            let n2 = M!(-6).yy0();
            sqlite3BeginTrigger(
                db,
                &n1,
                &n2,
                M!(-5).yy144(),
                ev.a,
                ev.b,
                M!(-2).yy203(),
                M!(0).yy454(),
                M!(-10).yy144(),
                M!(-8).yy144(),
            );
            set!(-10, yy0m, if n2.n == 0 { n1 } else { n2 }); /*A-overwrites-T*/
        }
        262 => {
            /* trigger_time ::= BEFORE|AFTER */
            set!(0, yy144m, MAJ!(0)); /*A-overwrites-X*/
        }
        263 => {
            /* trigger_time ::= INSTEAD OF */
            set!(-1, yy144m, TK_INSTEAD as i32);
        }
        264 => {
            /* trigger_time ::= */
            set!(1, yy144m, TK_BEFORE as i32);
        }
        265 | 266 => {
            /* trigger_event ::= DELETE|INSERT */
            /* trigger_event ::= UPDATE */
            set!(
                0,
                yy286m,
                TrigEvent {
                    a: MAJ!(0),
                    b: IdListId::NULL
                }
            ); /*A-overwrites-X*/
        }
        267 => {
            /* trigger_event ::= UPDATE OF idlist */
            set!(
                -2,
                yy286m,
                TrigEvent {
                    a: TK_UPDATE as i32,
                    b: M!(0).yy132()
                }
            );
        }
        268 | 286 => {
            /* when_clause ::= */
            /* key_opt ::= */
            set!(1, yy454m, N);
        }
        269 | 287 => {
            /* when_clause ::= WHEN expr */
            /* key_opt ::= KEY expr */
            set!(-1, yy454m, M!(0).yy454());
        }
        270 => {
            /* trigger_cmd_list ::= trigger_cmd_list trigger_cmd SEMI */
            let a = M!(-2).yy427();
            let b = M!(-1).yy427();
            let last = db.arena.tstep(a).pLast;
            db.arena.tstepm(last).pNext = b;
            db.arena.tstepm(a).pLast = b;
        }
        271 => {
            /* trigger_cmd_list ::= trigger_cmd SEMI */
            let a = M!(-1).yy427();
            db.arena.tstepm(a).pLast = a;
        }
        272 => {
            /* tridxby ::= INDEXED BY nm */
            errmsg!(
                db,
                b"the INDEXED BY clause is not allowed on UPDATE or DELETE statements within triggers"
            );
        }
        273 => {
            /* tridxby ::= NOT INDEXED */
            errmsg!(
                db,
                b"the NOT INDEXED clause is not allowed on UPDATE or DELETE statements within triggers"
            );
        }
        274 => {
            /* trigger_cmd ::= UPDATE orconf xfullname tridxby SET setlist from where_opt scanpt */
            let s = sqlite3TriggerUpdateStep(
                db,
                M!(-6).yy203(),
                M!(-2).yy203(),
                M!(-3).yy14(),
                M!(-1).yy454(),
                M!(-7).yy144() as u8,
                M!(-8).yy0().off as usize,
                M!(0).yy168(),
            );
            set!(-8, yy427m, s);
        }
        275 => {
            /* trigger_cmd ::= scanpt insert_cmd INTO xfullname idlist_opt select upsert scanpt */
            let s = sqlite3TriggerInsertStep(
                db,
                M!(-4).yy203(),
                M!(-3).yy132(),
                M!(-2).yy555(),
                M!(-6).yy144() as u8,
                M!(-1).yy122(),
                M!(-7).yy168(),
                M!(0).yy168(),
            ); /*yylhsminor.yy427-overwrites-yymsp[-6].minor.yy144*/
            set!(-7, yy427m, s);
        }
        276 => {
            /* trigger_cmd ::= DELETE FROM xfullname tridxby where_opt scanpt */
            let s = sqlite3TriggerDeleteStep(
                db,
                M!(-3).yy203(),
                M!(-1).yy454(),
                M!(-5).yy0().off as usize,
                M!(0).yy168(),
            );
            set!(-5, yy427m, s);
        }
        277 => {
            /* trigger_cmd ::= scanpt select scanpt */
            let s = sqlite3TriggerSelectStep(db, M!(-1).yy555(), M!(-2).yy168(), M!(0).yy168()); /*yylhsminor.yy427-overwrites-yymsp[-1].minor.yy555*/
            set!(-2, yy427m, s);
        }
        278 => {
            /* expr ::= RAISE LP IGNORE RP */
            let e = sqlite3PExpr(db, TK_RAISE as i32, N, N);
            if e.nn() {
                db.exm(e).affExpr = OE_Ignore;
            }
            set!(-3, yy454m, e);
        }
        279 => {
            /* expr ::= RAISE LP raisetype COMMA expr RP */
            let e = sqlite3PExpr(db, TK_RAISE as i32, M!(-1).yy454(), N);
            if e.nn() {
                db.exm(e).affExpr = M!(-3).yy144() as u8;
            }
            set!(-5, yy454m, e);
        }
        280 => {
            /* raisetype ::= ROLLBACK */
            set!(0, yy144m, OE_Rollback as i32);
        }
        282 => {
            /* raisetype ::= FAIL */
            set!(0, yy144m, OE_Fail as i32);
        }
        283 => {
            /* cmd ::= DROP TRIGGER ifexists fullname */
            sqlite3DropTrigger(db, M!(0).yy203(), M!(-1).yy144());
        }
        284 => {
            /* cmd ::= ATTACH database_kw_opt expr AS expr key_opt */
            sqlite3Attach(db, M!(-3).yy454(), M!(-1).yy454(), M!(0).yy454());
        }
        285 => {
            /* cmd ::= DETACH database_kw_opt expr */
            sqlite3Detach(db, M!(0).yy454());
        }
        288 => {
            /* cmd ::= REINDEX */
            sqlite3Reindex(db, None, None);
        }
        289 => {
            /* cmd ::= REINDEX nm dbnm */
            sqlite3Reindex(db, Some(&M!(-1).yy0()), Some(&M!(0).yy0()));
        }
        290 => {
            /* cmd ::= ANALYZE */
            sqlite3Analyze(db, None, None);
        }
        291 => {
            /* cmd ::= ANALYZE nm dbnm */
            sqlite3Analyze(db, Some(&M!(-1).yy0()), Some(&M!(0).yy0()));
        }
        292 => {
            /* cmd ::= ALTER TABLE fullname RENAME TO nm */
            sqlite3AlterRenameTable(db, M!(-3).yy203(), &M!(0).yy0());
        }
        293 => {
            /* cmd ::= alter_add carglist */
            let lt = db.ppr().sLastToken.clone();
            {
                let t = M!(-1).yy0m();
                t.n = (lt.off - t.off) + lt.n;
            }
            sqlite3AlterFinishAddColumn(db, &M!(-1).yy0());
        }
        294 => {
            /* alter_add ::= ALTER TABLE fullname ADD kwcolumn_opt nm typetoken */
            disableLookaside(db);
            sqlite3AlterBeginAddColumn(db, M!(-4).yy203());
            sqlite3AddColumn(db, M!(-1).yy0(), M!(0).yy0());
            set!(-6, yy0m, M!(-1).yy0());
        }
        295 => {
            /* cmd ::= ALTER TABLE fullname DROP kwcolumn_opt nm */
            sqlite3AlterDropColumn(db, M!(-3).yy203(), &M!(0).yy0());
        }
        296 => {
            /* cmd ::= ALTER TABLE fullname RENAME kwcolumn_opt nm TO nm */
            sqlite3AlterRenameColumn(db, M!(-5).yy203(), &M!(-2).yy0(), &M!(0).yy0());
        }
        297 => {
            /* cmd ::= ALTER TABLE fullname DROP CONSTRAINT nm */
            sqlite3AlterDropConstraint(db, M!(-3).yy203(), Some(&M!(0).yy0()), None);
        }
        298 => {
            /* cmd ::= ALTER TABLE fullname ALTER kwcolumn_opt nm DROP NOT NULL */
            sqlite3AlterDropConstraint(db, M!(-6).yy203(), None, Some(&M!(-3).yy0()));
        }
        299 => {
            /* cmd ::= ALTER TABLE fullname ALTER kwcolumn_opt nm SET NOT NULL onconf */
            sqlite3AlterSetNotNull(db, M!(-7).yy203(), &M!(-4).yy0(), &M!(-2).yy0());
        }
        300 => {
            /* cmd ::= ALTER TABLE fullname ADD CONSTRAINT nm CHECK LP expr RP onconf */
            let lp = M!(-3).yy0();
            let rp = M!(-1).yy0();
            let nExpr = rp.off as i32 - lp.off as i32 - 1;
            let rest = tok_rest(&lp);
            let zExpr = &rest[1.min(rest.len())..];
            sqlite3AlterAddConstraint(
                db,
                M!(-8).yy203(),
                &M!(-6).yy0(),
                Some(&M!(-5).yy0()),
                Some(zExpr),
                nExpr,
                M!(-2).yy454(),
            );
        }
        301 => {
            /* cmd ::= ALTER TABLE fullname ADD CHECK LP expr RP onconf */
            let lp = M!(-3).yy0();
            let rp = M!(-1).yy0();
            let nExpr = rp.off as i32 - lp.off as i32 - 1;
            let rest = tok_rest(&lp);
            let zExpr = &rest[1.min(rest.len())..];
            sqlite3AlterAddConstraint(
                db,
                M!(-6).yy203(),
                &M!(-4).yy0(),
                None,
                Some(zExpr),
                nExpr,
                M!(-2).yy454(),
            );
        }
        302 => {
            /* cmd ::= create_vtab */
            sqlite3VtabFinishParse(db, None);
        }
        303 => {
            /* cmd ::= create_vtab LP vtabarglist RP */
            sqlite3VtabFinishParse(db, Some(&M!(0).yy0()));
        }
        304 => {
            /* create_vtab ::= createkw VIRTUAL TABLE ifnotexists nm dbnm USING nm */
            sqlite3VtabBeginParse(
                db,
                &M!(-3).yy0(),
                &M!(-2).yy0(),
                &M!(0).yy0(),
                M!(-4).yy144(),
            );
        }
        305 => {
            /* vtabarg ::= */
            sqlite3VtabArgInit(db);
        }
        306 | 307 | 308 => {
            /* vtabargtoken ::= ANY */
            /* vtabargtoken ::= lp anylist RP */
            /* lp ::= LP */
            sqlite3VtabArgExtend(db, &M!(0).yy0());
        }
        309 | 310 => {
            /* with ::= WITH wqlist */
            /* with ::= WITH RECURSIVE wqlist */
            sqlite3WithPush(db, M!(0).yy59(), 1);
        }
        311 => {
            /* wqas ::= AS */
            set!(0, yy462m, M10d_Any);
        }
        312 => {
            /* wqas ::= AS MATERIALIZED */
            set!(-1, yy462m, M10d_Yes);
        }
        313 => {
            /* wqas ::= AS NOT MATERIALIZED */
            set!(-2, yy462m, M10d_No);
        }
        314 => {
            /* wqitem ::= withnm eidlist_opt wqas LP select RP */
            let c = sqlite3CteNew(
                db,
                &M!(-5).yy0(),
                M!(-4).yy14(),
                M!(-1).yy555(),
                M!(-3).yy462(),
            ); /*A-overwrites-X*/
            M!(-5).set_yy67(c);
        }
        315 => {
            /* withnm ::= nm */
            db.pp().bHasWith = 1;
        }
        316 => {
            /* wqlist ::= wqitem */
            let c = M!(0).take_yy67();
            set!(0, yy59m, sqlite3WithAdd(db, WithId::NULL, c)); /*A-overwrites-X*/
        }
        317 => {
            /* wqlist ::= wqlist COMMA wqitem */
            let c = M!(0).take_yy67();
            set!(-2, yy59m, sqlite3WithAdd(db, M!(-2).yy59(), c));
        }
        318 => {
            /* windowdefn_list ::= windowdefn_list COMMA windowdefn */
            let w = M!(0).yy211();
            let l = M!(-2).yy211();
            debug_assert!(w.nn());
            sqlite3WindowChain(db, w, l);
            db.winm(w).pNextWin = l;
            set!(-2, yy211m, w);
        }
        319 => {
            /* windowdefn ::= nm AS LP window RP */
            let w = M!(-1).yy211();
            if w.nn() {
                let z = M!(-4).yy0().text().to_vec();
                db.winm(w).zName = Some(z);
            }
            set!(-4, yy211m, w);
        }
        320 => {
            /* window ::= PARTITION BY nexprlist orderby_opt frame_opt */
            set!(
                -4,
                yy211m,
                sqlite3WindowAssemble(db, M!(0).yy211(), M!(-2).yy14(), M!(-1).yy14(), None)
            );
        }
        321 => {
            /* window ::= nm PARTITION BY nexprlist orderby_opt frame_opt */
            set!(
                -5,
                yy211m,
                sqlite3WindowAssemble(
                    db,
                    M!(0).yy211(),
                    M!(-2).yy14(),
                    M!(-1).yy14(),
                    Some(&M!(-5).yy0())
                )
            );
        }
        322 => {
            /* window ::= ORDER BY sortlist frame_opt */
            set!(
                -3,
                yy211m,
                sqlite3WindowAssemble(db, M!(0).yy211(), NL, M!(-1).yy14(), None)
            );
        }
        323 => {
            /* window ::= nm ORDER BY sortlist frame_opt */
            set!(
                -4,
                yy211m,
                sqlite3WindowAssemble(db, M!(0).yy211(), NL, M!(-1).yy14(), Some(&M!(-4).yy0()))
            );
        }
        324 => {
            /* window ::= nm frame_opt */
            set!(
                -1,
                yy211m,
                sqlite3WindowAssemble(db, M!(0).yy211(), NL, NL, Some(&M!(-1).yy0()))
            );
        }
        325 => {
            /* frame_opt ::= */
            set!(
                1,
                yy211m,
                sqlite3WindowAlloc(db, 0, TK_UNBOUNDED as i32, N, TK_CURRENT as i32, N, 0)
            );
        }
        326 => {
            /* frame_opt ::= range_or_rows frame_bound_s frame_exclude_opt */
            let b = M!(-1).yy509();
            set!(
                -2,
                yy211m,
                sqlite3WindowAlloc(
                    db,
                    M!(-2).yy144(),
                    b.eType,
                    b.pExpr,
                    TK_CURRENT as i32,
                    N,
                    M!(0).yy462()
                )
            );
        }
        327 => {
            /* frame_opt ::= range_or_rows BETWEEN frame_bound_s AND frame_bound_e frame_exclude_opt */
            let s = M!(-3).yy509();
            let e = M!(-1).yy509();
            set!(
                -5,
                yy211m,
                sqlite3WindowAlloc(
                    db,
                    M!(-5).yy144(),
                    s.eType,
                    s.pExpr,
                    e.eType,
                    e.pExpr,
                    M!(0).yy462()
                )
            );
        }
        329 | 331 => {
            /* frame_bound_s ::= frame_bound */
            /* frame_bound_e ::= frame_bound */
            set!(0, yy509m, M!(0).yy509());
        }
        330 | 332 | 334 => {
            /* frame_bound_s ::= UNBOUNDED PRECEDING */
            /* frame_bound_e ::= UNBOUNDED FOLLOWING */
            /* frame_bound ::= CURRENT ROW */
            set!(
                -1,
                yy509m,
                FrameBound {
                    eType: MAJ!(-1),
                    pExpr: N
                }
            );
        }
        333 => {
            /* frame_bound ::= expr PRECEDING|FOLLOWING */
            set!(
                -1,
                yy509m,
                FrameBound {
                    eType: MAJ!(0),
                    pExpr: M!(-1).yy454()
                }
            );
        }
        335 => {
            /* frame_exclude_opt ::= */
            set!(1, yy462m, 0);
        }
        336 => {
            /* frame_exclude_opt ::= EXCLUDE frame_exclude */
            set!(-1, yy462m, M!(0).yy462());
        }
        337 | 338 => {
            /* frame_exclude ::= NO OTHERS */
            /* frame_exclude ::= CURRENT ROW */
            set!(-1, yy462m, MAJ!(-1) as u8); /*A-overwrites-X*/
        }
        339 => {
            /* frame_exclude ::= GROUP|TIES */
            set!(0, yy462m, MAJ!(0) as u8); /*A-overwrites-X*/
        }
        340 => {
            /* window_clause ::= WINDOW windowdefn_list */
            set!(-1, yy211m, M!(0).yy211());
        }
        341 => {
            /* filter_over ::= filter_clause over_clause */
            let w = M!(0).yy211();
            let f = M!(-1).yy454();
            if w.nn() {
                db.winm(w).pFilter = f;
            } else {
                sqlite3ExprDelete(db, f);
            }
            set!(-1, yy211m, w);
        }
        342 => {
            /* filter_over ::= over_clause */
            set!(0, yy211m, M!(0).yy211());
        }
        343 => {
            /* filter_over ::= filter_clause */
            let f = M!(0).yy454();
            let w = db.arena.new_window(Window {
                eFrmType: TK_FILTER,
                pFilter: f,
                ..Default::default()
            });
            set!(0, yy211m, w);
        }
        344 => {
            /* over_clause ::= OVER LP window RP */
            set!(-3, yy211m, M!(-1).yy211());
            debug_assert!(M!(-3).yy211().nn());
        }
        345 => {
            /* over_clause ::= OVER nm */
            let z = M!(0).yy0().text().to_vec();
            let w = db.arena.new_window(Window {
                zName: Some(z),
                ..Default::default()
            });
            set!(-1, yy211m, w);
        }
        346 => {
            /* filter_clause ::= FILTER LP WHERE expr RP */
            set!(-4, yy454m, M!(-1).yy454());
        }
        347 => {
            /* term ::= QNUMBER */
            let e = tokenExpr(db, MAJ!(0), M!(0).yy0());
            sqlite3DequoteNumber(db, e);
            set!(0, yy454m, e);
        }
        _ => {
            /* (348) input ::= cmdlist */
            /* (349) cmdlist ::= cmdlist ecmd */
            /* (350) cmdlist ::= ecmd (OPTIMIZED OUT) */
            /* (351) ecmd ::= SEMI */
            /* (352) ecmd ::= cmdx SEMI */
            /* (353) ecmd ::= explain cmdx SEMI (NEVER REDUCES) */
            /* (354) trans_opt ::= */
            /* (355) trans_opt ::= TRANSACTION */
            /* (356) trans_opt ::= TRANSACTION nm */
            /* (357) savepoint_opt ::= SAVEPOINT */
            /* (358) savepoint_opt ::= */
            /* (359) cmd ::= create_table create_table_args */
            /* (360) table_option_set ::= table_option (OPTIMIZED OUT) */
            /* (361) columnlist ::= columnlist COMMA columnname carglist */
            /* (362) columnlist ::= columnname carglist */
            /* (363) nm ::= ID|INDEXED|JOIN_KW */
            /* (364) nm ::= STRING */
            /* (365) typetoken ::= typename */
            /* (366) typename ::= ID|STRING */
            /* (367) signed ::= plus_num (OPTIMIZED OUT) */
            /* (368) signed ::= minus_num (OPTIMIZED OUT) */
            /* (369) carglist ::= carglist ccons */
            /* (370) carglist ::= */
            /* (371) ccons ::= NULL onconf */
            /* (372) ccons ::= GENERATED ALWAYS AS generated */
            /* (373) ccons ::= AS generated */
            /* (374) conslist_opt ::= COMMA conslist */
            /* (375) conslist ::= conslist tconscomma tcons */
            /* (376) conslist ::= tcons (OPTIMIZED OUT) */
            /* (377) tconscomma ::= */
            /* (378) defer_subclause_opt ::= defer_subclause (OPTIMIZED OUT) */
            /* (379) resolvetype ::= raisetype (OPTIMIZED OUT) */
            /* (380) selectnowith ::= oneselect (OPTIMIZED OUT) */
            /* (381) oneselect ::= values */
            /* (382) sclp ::= selcollist COMMA */
            /* (383) as ::= ID|STRING */
            /* (384) indexed_opt ::= indexed_by (OPTIMIZED OUT) */
            /* (385) returning ::= */
            /* (386) expr ::= term (OPTIMIZED OUT) */
            /* (387) likeop ::= LIKE_KW|MATCH */
            /* (388) case_operand ::= expr */
            /* (389) exprlist ::= nexprlist */
            /* (390) nmnum ::= plus_num (OPTIMIZED OUT) */
            /* (391) nmnum ::= nm (OPTIMIZED OUT) */
            /* (392) nmnum ::= ON */
            /* (393) nmnum ::= DELETE */
            /* (394) nmnum ::= DEFAULT */
            /* (395) plus_num ::= INTEGER|FLOAT */
            /* (396) foreach_clause ::= */
            /* (397) foreach_clause ::= FOR EACH ROW */
            /* (398) tridxby ::= */
            /* (399) database_kw_opt ::= DATABASE */
            /* (400) database_kw_opt ::= */
            /* (401) kwcolumn_opt ::= */
            /* (402) kwcolumn_opt ::= COLUMNKW */
            /* (403) vtabarglist ::= vtabarg */
            /* (404) vtabarglist ::= vtabarglist COMMA vtabarg */
            /* (405) vtabarg ::= vtabarg vtabargtoken */
            /* (406) anylist ::= */
            /* (407) anylist ::= anylist LP anylist RP */
            /* (408) anylist ::= anylist ANY */
            /* (409) with ::= */
            /* (410) windowdefn_list ::= windowdefn (OPTIMIZED OUT) */
            /* (411) window ::= frame_opt (OPTIMIZED OUT) */
            debug_assert!(!matches!(
                yyruleno,
                350 | 353
                    | 360
                    | 367
                    | 368
                    | 376
                    | 378
                    | 379
                    | 380
                    | 384
                    | 386
                    | 390
                    | 391
                    | 410
                    | 411
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize::sqlite3GetToken;

    /// A context without actions: exercises the automaton alone.
    struct TestCtx {
        err: Option<String>,
        overflow: bool,
        limit: i32,
        reduced: Vec<u32>,
    }

    impl YyCtx for TestCtx {
        fn yy_reduce_action(&mut self, _p: &mut yyParser, yyruleno: u32, _la: i32, _lt: &Token) {
            self.reduced.push(yyruleno);
        }
        fn yy_syntax_error(&mut self, _yymajor: i32, t: &Token) {
            if self.err.is_none() {
                self.err = Some(if t.at(0) != 0 {
                    format!(
                        "near \"{}\": syntax error",
                        String::from_utf8_lossy(t.text())
                    )
                } else {
                    "incomplete input".to_string()
                });
            }
        }
        fn yy_stack_overflow(&mut self) {
            self.overflow = true;
            if self.err.is_none() {
                self.err = Some("Recursion limit".to_string());
            }
        }
        fn yy_destructor(&mut self, _m: u16, _p: &mut YYMINORTYPE) {}
        fn yy_size_limit(&self) -> i32 {
            self.limit
        }
    }

    /// The token loop of sqlite3RunParser without a connection.
    fn check_with_limit(sql: &str, limit: i32) -> (Result<(), String>, Vec<u32>) {
        let z: Rc<[u8]> = Rc::from(sql.as_bytes());
        let mut ctx = TestCtx {
            err: None,
            overflow: false,
            limit,
            reduced: Vec::new(),
        };
        let mut p = yyParser::new();
        let mut pos = 0usize;
        let mut last: i32 = -1;
        loop {
            let mut tt = 0;
            let mut n = sqlite3GetToken(&z[pos.min(z.len())..], &mut tt);
            if tt >= TK_WINDOW as i32 {
                if tt == TK_SPACE as i32 || tt == TK_COMMENT as i32 {
                    pos += n as usize;
                    continue;
                }
                if pos >= z.len() {
                    if last == TK_SEMI as i32 {
                        tt = 0;
                    } else if last == 0 {
                        break;
                    } else {
                        tt = TK_SEMI as i32;
                    }
                    n = 0;
                } else if tt == TK_WINDOW as i32 || tt == TK_OVER as i32 || tt == TK_FILTER as i32 {
                    tt = crate::tokenize::analyze_keyword(tt, &z[pos + n as usize..], last);
                } else if tt != TK_QNUMBER as i32 {
                    return (
                        Err(format!(
                            "unrecognized token: \"{}\"",
                            String::from_utf8_lossy(&z[pos..pos + n as usize])
                        )),
                        ctx.reduced,
                    );
                }
            }
            let tok = Token {
                z: Some(z.clone()),
                off: pos as u32,
                n: n as u32,
            };
            sqlite3ParserCtx(&mut p, &mut ctx, tt, tok);
            last = tt;
            pos += n as usize;
            if ctx.err.is_some() {
                break;
            }
        }
        yy_finalize(&mut p, &mut ctx);
        (ctx.err.map_or(Ok(()), Err), ctx.reduced)
    }

    fn check(sql: &str) -> Result<(), String> {
        check_with_limit(sql, 1000).0
    }

    #[test]
    fn tables_consistent() {
        assert_eq!(yy_action.len() as i32, YY_ACTTAB_COUNT);
        assert_eq!(yy_shift_ofst.len() as i32, YY_SHIFT_COUNT + 1);
        assert_eq!(yy_reduce_ofst.len() as i32, YY_REDUCE_COUNT + 1);
        assert_eq!(yy_default.len() as i32, YYNSTATE);
        assert_eq!(yyFallback.len() as i32, YYNTOKEN);
        assert_eq!(yyRuleInfoLhs.len() as i32, YYNRULE);
        assert_eq!(yyRuleInfoNRhs.len() as i32, YYNRULE);
        assert_eq!(yyTokenName.len() as i32, YYNOCODE);
        assert_eq!(yyTokenName[TK_SELECT as usize], "SELECT");
        assert_eq!(yyTokenName[TK_ILLEGAL as usize], "ILLEGAL");
        assert_eq!(sqlite3ParserFallback(TK_EXPLAIN as i32), TK_ID as i32);
        assert_eq!(sqlite3ParserFallback(TK_SELECT as i32), 0);
    }

    #[test]
    fn valid_statements() {
        for s in [
            "SELECT 1",
            "select a, b as c, t.* from t join u using(x) where a > 1 group by b having count(*) > 2 order by 1 desc nulls last limit 10 offset 2;",
            "SELECT * FROM t1 LEFT OUTER JOIN t2 ON t1.a=t2.b;",
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c LIMIT 5) SELECT x FROM c",
            "CREATE TABLE t(a INTEGER PRIMARY KEY AUTOINCREMENT, b TEXT NOT NULL DEFAULT 'x' COLLATE nocase, c REAL CHECK(c>0), UNIQUE(b,c), FOREIGN KEY(b) REFERENCES u(x) ON DELETE CASCADE) WITHOUT ROWID, STRICT",
            "CREATE TEMP VIEW IF NOT EXISTS v(a) AS SELECT 1",
            "CREATE UNIQUE INDEX i ON t(a COLLATE nocase DESC, b) WHERE a IS NOT NULL",
            "CREATE TRIGGER tr AFTER UPDATE OF a ON t FOR EACH ROW WHEN new.a>0 BEGIN INSERT INTO u VALUES(new.a); DELETE FROM u WHERE x=old.a; END",
            "INSERT INTO t(a,b) VALUES(1,2),(3,4) ON CONFLICT(a) DO UPDATE SET b=excluded.b WHERE b<>0 RETURNING *",
            "INSERT OR REPLACE INTO t DEFAULT VALUES",
            "UPDATE t SET (a,b)=(1,2), c=3 FROM u WHERE t.x=u.x",
            "DELETE FROM t WHERE a IN (1,2,3) AND b NOT IN (SELECT x FROM u) AND c BETWEEN 1 AND 2",
            "SELECT CASE WHEN a THEN b ELSE c END, CAST(a AS VARCHAR(10)), a LIKE 'x%' ESCAPE '\\', a NOT GLOB 'y', a ->> '$.x' FROM t",
            "SELECT sum(x) OVER (PARTITION BY y ORDER BY z ROWS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE TIES) FROM t",
            "SELECT count(*) FILTER (WHERE x>1) OVER w FROM t WINDOW w AS (ORDER BY y)",
            "SELECT window, over, filter FROM t",
            "SELECT x AS over FROM t",
            "PRAGMA main.cache_size = -2000",
            "PRAGMA table_info(t)",
            "BEGIN IMMEDIATE; COMMIT; SAVEPOINT a; RELEASE a; ROLLBACK TO a",
            "ATTACH 'f.db' AS aux; DETACH aux",
            "VACUUM INTO 'x.db'",
            "ALTER TABLE t ADD COLUMN d INT DEFAULT 0",
            "ALTER TABLE t RENAME COLUMN a TO b",
            "ALTER TABLE t ADD CONSTRAINT c CHECK (a>0)",
            "CREATE VIRTUAL TABLE v USING fts5(a, b, tokenize='porter (x)')",
            "EXPLAIN QUERY PLAN SELECT 1",
            "SELECT 1_000, 0x1F, ?1, :a, @b, $c, x'00'",
            "REINDEX; ANALYZE; ANALYZE main.t",
            "DROP TABLE IF EXISTS t; DROP INDEX i; DROP VIEW v; DROP TRIGGER tr",
            "SELECT * FROM (t1, t2) AS x, (SELECT 1) y, json_each('[1]')",
            "SELECT a IS NOT DISTINCT FROM b, a IS DISTINCT FROM b, a ISNULL, a NOTNULL, a NOT NULL FROM t",
            ";;",
            "",
        ] {
            assert_eq!(check(s), Ok(()), "{}", s);
        }
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(check("SELECT"), Err("incomplete input".into()));
        assert_eq!(check("SELECT 1 FROM"), Err("incomplete input".into()));
        assert_eq!(check("SELEC 1"), Err("near \"SELEC\": syntax error".into()));
        assert_eq!(
            check("SELECT 1 FROM t WHERE"),
            Err("incomplete input".into())
        );
        assert_eq!(
            check("SELECT * FROM t t2 t3"),
            Err("near \"t3\": syntax error".into())
        );
        assert_eq!(
            check("CREATE TABLE t(a,)"),
            Err("near \")\": syntax error".into())
        );
        assert_eq!(check("SELECT 1 2"), Err("near \"2\": syntax error".into()));
        assert_eq!(
            check("SELECT 'abc"),
            Err("unrecognized token: \"'abc\"".into())
        );
        assert_eq!(check("SELECT 1 !"), Err("unrecognized token: \"!\"".into()));
        assert_eq!(
            check("INSERT INTO t VALUES(1"),
            Err("incomplete input".into())
        );
        assert_eq!(
            check("SELECT 1; SELEKT 2"),
            Err("near \"SELEKT\": syntax error".into())
        );
    }

    #[test]
    fn stack_overflow() {
        let deep = format!("SELECT {}1{}", "(".repeat(200), ")".repeat(200));
        assert_eq!(check_with_limit(&deep, 1000).0, Ok(()));
        let r = check_with_limit(&deep, 100).0;
        assert_eq!(r, Err("Recursion limit".into()));
    }

    #[test]
    fn reductions() {
        /* "SELECT 1" reduces cmd ::= select (84) and cmdx ::= cmd (2) */
        let (r, red) = check_with_limit("SELECT 1", 1000);
        assert_eq!(r, Ok(()));
        assert!(red.contains(&84));
        assert!(red.contains(&2));
        assert!(red.contains(&92));
    }
}
