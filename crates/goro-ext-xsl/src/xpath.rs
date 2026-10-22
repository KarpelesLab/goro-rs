//! XPath 1.0 engine used by the XSLT processor: a port of libxml2's xpath.c
//! compiler (the step array with ch1/ch2 links) and its stack-based
//! evaluator (xmlXPathCompOpEval & co.), including the core function
//! library, so that results, evaluation order and error reporting follow
//! libxml2 (2.15).
//!
//! The evaluator is generic over a [`Host`] (the XSLT transformation
//! context) providing variables, extension functions and error output.

use goro_ext_xml::tree::{self, NType, NodeId, Tree, XML_XML_NAMESPACE};
use std::cell::RefCell;
use std::collections::HashMap;
type HashSet<T> = std::collections::HashSet<T, goro_core::fxhash::FxBuildHasher>;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Errors

pub const XPATH_EXPRESSION_OK: i32 = 0;
pub const XPATH_NUMBER_ERROR: i32 = 1;
pub const XPATH_UNFINISHED_LITERAL_ERROR: i32 = 2;
pub const XPATH_START_LITERAL_ERROR: i32 = 3;
pub const XPATH_VARIABLE_REF_ERROR: i32 = 4;
pub const XPATH_UNDEF_VARIABLE_ERROR: i32 = 5;
pub const XPATH_INVALID_PREDICATE_ERROR: i32 = 6;
pub const XPATH_EXPR_ERROR: i32 = 7;
pub const XPATH_UNCLOSED_ERROR: i32 = 8;
pub const XPATH_UNKNOWN_FUNC_ERROR: i32 = 9;
pub const XPATH_INVALID_OPERAND: i32 = 10;
pub const XPATH_INVALID_TYPE: i32 = 11;
pub const XPATH_INVALID_ARITY: i32 = 12;
pub const XPATH_INVALID_CTXT_SIZE: i32 = 13;
pub const XPATH_INVALID_CTXT_POSITION: i32 = 14;
pub const XPATH_MEMORY_ERROR: i32 = 15;
pub const XPATH_UNDEF_PREFIX_ERROR: i32 = 19;
pub const XPATH_INVALID_CHAR_ERROR: i32 = 21;
pub const XPATH_INVALID_CTXT: i32 = 22;
pub const XPATH_STACK_ERROR: i32 = 23;
pub const XPATH_FORBID_VARIABLE_ERROR: i32 = 24;
pub const XPATH_OP_LIMIT_EXCEEDED: i32 = 25;
pub const XPATH_RECURSION_LIMIT_EXCEEDED: i32 = 26;

const ERROR_MESSAGES: [&str; 28] = [
    "Ok",
    "Number encoding",
    "Unfinished literal",
    "Start of literal",
    "Expected $ for variable reference",
    "Undefined variable",
    "Invalid predicate",
    "Invalid expression",
    "Missing closing curly brace",
    "Unregistered function",
    "Invalid operand",
    "Invalid type",
    "Invalid number of arguments",
    "Invalid context size",
    "Invalid context position",
    "Memory allocation error",
    "Syntax error",
    "Resource error",
    "Sub resource error",
    "Undefined namespace prefix",
    "Encoding error",
    "Char out of XML range",
    "Invalid or incomplete context",
    "Stack usage error",
    "Forbidden variable",
    "Operation limit exceeded",
    "Recursion limit exceeded",
    "?? Unknown error ??",
];

pub fn error_message(code: i32) -> &'static str {
    let c = if code < 0 || code as usize >= ERROR_MESSAGES.len() {
        ERROR_MESSAGES.len() - 1
    } else {
        code as usize
    };
    ERROR_MESSAGES[c]
}

const XPATH_MAX_RECURSION_DEPTH: i32 = 5000;

// ---------------------------------------------------------------------------
// Values

/// An XPath object (xmlXPathObject). `Tree` is XPATH_XSLT_TREE: a result
/// tree fragment, holding its (fake) document node.
#[derive(Clone, Debug)]
pub enum XObj {
    NodeSet(Vec<NodeId>),
    Bool(bool),
    Num(f64),
    Str(String),
    Tree(Vec<NodeId>),
    /// XPATH_USERS: an external object (saxon:expression() results)
    External(Rc<CompExpr>),
}

impl XObj {
    pub fn is_nodeset(&self) -> bool {
        matches!(self, XObj::NodeSet(_))
    }
    pub fn is_nodeset_or_tree(&self) -> bool {
        matches!(self, XObj::NodeSet(_) | XObj::Tree(_))
    }
    pub fn nodes(&self) -> Option<&Vec<NodeId>> {
        match self {
            XObj::NodeSet(v) | XObj::Tree(v) => Some(v),
            _ => None,
        }
    }
    pub fn type_code(&self) -> i32 {
        match self {
            XObj::NodeSet(_) => 1,
            XObj::Bool(_) => 2,
            XObj::Num(_) => 3,
            XObj::Str(_) => 4,
            XObj::Tree(_) => 9,
            XObj::External(_) => 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Compiled expressions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    End,
    And,
    Or,
    Equal,
    Cmp,
    Plus,
    Mult,
    Union,
    Root,
    Node,
    Collect,
    Value,
    Variable,
    Function,
    Arg,
    Predicate,
    Filter,
    Sort,
}

pub const AXIS_ANCESTOR: i32 = 1;
pub const AXIS_ANCESTOR_OR_SELF: i32 = 2;
pub const AXIS_ATTRIBUTE: i32 = 3;
pub const AXIS_CHILD: i32 = 4;
pub const AXIS_DESCENDANT: i32 = 5;
pub const AXIS_DESCENDANT_OR_SELF: i32 = 6;
pub const AXIS_FOLLOWING: i32 = 7;
pub const AXIS_FOLLOWING_SIBLING: i32 = 8;
pub const AXIS_NAMESPACE: i32 = 9;
pub const AXIS_PARENT: i32 = 10;
pub const AXIS_PRECEDING: i32 = 11;
pub const AXIS_PRECEDING_SIBLING: i32 = 12;
pub const AXIS_SELF: i32 = 13;

pub const NODE_TEST_NONE: i32 = 0;
pub const NODE_TEST_TYPE: i32 = 1;
pub const NODE_TEST_PI: i32 = 2;
pub const NODE_TEST_ALL: i32 = 3;
pub const NODE_TEST_NAME: i32 = 5;

pub const NODE_TYPE_NODE: i32 = 0;
pub const NODE_TYPE_COMMENT: i32 = 8;
pub const NODE_TYPE_TEXT: i32 = 3;
pub const NODE_TYPE_PI: i32 = 7;

#[derive(Clone, Debug)]
pub struct StepOp {
    pub op: OpKind,
    pub ch1: i32,
    pub ch2: i32,
    pub value: i32,
    pub value2: i32,
    pub value3: i32,
    /// XPATH_OP_VALUE: the literal object
    pub obj: Option<XObj>,
    /// name (function, variable, name test)
    pub s4: Option<String>,
    /// prefix
    pub s5: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CompExpr {
    pub steps: Vec<StepOp>,
    pub last: i32,
    pub expr: String,
}

pub type CompRef = Rc<CompExpr>;

// ---------------------------------------------------------------------------
// Compiler (xmlXPathCompileExpr & co.)

struct Parser<'a> {
    s: &'a [u8],
    cur: usize,
    error: i32,
    comp: CompExpr,
    depth: i32,
    /// collected error messages
    errors: Vec<String>,
}

fn is_blank_ch(c: u8) -> bool {
    c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D
}

fn is_ascii_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_ascii_letter(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// Decode one UTF-8 character.
fn utf8_at(s: &[u8], i: usize) -> Option<(u32, usize)> {
    let c = *s.get(i)?;
    if c < 0x80 {
        return Some((c as u32, 1));
    }
    let (len, init) = if c & 0xE0 == 0xC0 {
        (2, (c & 0x1F) as u32)
    } else if c & 0xF0 == 0xE0 {
        (3, (c & 0x0F) as u32)
    } else if c & 0xF8 == 0xF0 {
        (4, (c & 0x07) as u32)
    } else {
        return None;
    };
    let mut v = init;
    for k in 1..len {
        let b = *s.get(i + k)?;
        if b & 0xC0 != 0x80 {
            return None;
        }
        v = (v << 6) | (b & 0x3F) as u32;
    }
    Some((v, len))
}

fn is_xml_char(c: u32) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// xmlScanName: end of the (NC)Name starting at `i`; Some(i) when empty.
fn scan_name(s: &[u8], mut i: usize, nc: bool) -> Option<usize> {
    let start = i;
    loop {
        let (c, l) = match utf8_at(s, i) {
            Some(x) => x,
            None => {
                if i < s.len() {
                    return None;
                }
                break;
            }
        };
        let ok = if i == start {
            goro_ext_xml::parser::is_name_start_char(c) && !(nc && c == ':' as u32)
        } else {
            goro_ext_xml::parser::is_name_char(c) && !(nc && c == ':' as u32)
        };
        if !ok {
            break;
        }
        i += l;
        if i - start > 50000 {
            return None;
        }
    }
    Some(i)
}

impl<'a> Parser<'a> {
    fn cur(&self) -> u8 {
        self.s.get(self.cur).copied().unwrap_or(0)
    }
    fn nxt(&self, n: usize) -> u8 {
        self.s.get(self.cur + n).copied().unwrap_or(0)
    }
    fn next(&mut self) {
        if self.cur < self.s.len() {
            self.cur += 1;
        }
    }
    fn skip(&mut self, n: usize) {
        self.cur = (self.cur + n).min(self.s.len());
    }
    fn skip_blanks(&mut self) {
        while is_blank_ch(self.cur()) {
            self.next();
        }
    }
    fn err(&mut self, code: i32) {
        if self.error != 0 {
            return;
        }
        self.error = code;
        self.errors.push(error_message(code).to_string());
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        ch1: i32,
        ch2: i32,
        op: OpKind,
        value: i32,
        value2: i32,
        value3: i32,
        obj: Option<XObj>,
        s4: Option<String>,
        s5: Option<String>,
    ) -> i32 {
        let idx = self.comp.steps.len() as i32;
        self.comp.last = idx;
        self.comp.steps.push(StepOp {
            op,
            ch1,
            ch2,
            value,
            value2,
            value3,
            obj,
            s4,
            s5,
        });
        idx
    }
    fn push_long(
        &mut self,
        op: OpKind,
        v: i32,
        v2: i32,
        v3: i32,
        s4: Option<String>,
        s5: Option<String>,
    ) {
        let last = self.comp.last;
        self.add(last, -1, op, v, v2, v3, None, s4, s5);
    }
    fn push_leave(&mut self, op: OpKind, v: i32, v2: i32) {
        self.add(-1, -1, op, v, v2, 0, None, None, None);
    }
    fn push_unary(&mut self, op: OpKind, ch: i32, v: i32, v2: i32) {
        self.add(ch, -1, op, v, v2, 0, None, None, None);
    }
    fn push_binary(&mut self, op: OpKind, ch1: i32, ch2: i32, v: i32, v2: i32) {
        self.add(ch1, ch2, op, v, v2, 0, None, None, None);
    }
    fn push_value(&mut self, obj: XObj) {
        let last = self.comp.last;
        let t = obj.type_code();
        self.add(last, -1, OpKind::Value, t, 0, 0, Some(obj), None, None);
    }

    fn parse_ncname(&mut self) -> Option<String> {
        match scan_name(self.s, self.cur, true) {
            None => {
                self.err(XPATH_EXPR_ERROR);
                None
            }
            Some(e) if e == self.cur => None,
            Some(e) => {
                let r = String::from_utf8_lossy(&self.s[self.cur..e]).into_owned();
                self.cur = e;
                Some(r)
            }
        }
    }

    fn parse_qname(&mut self) -> (Option<String>, Option<String>) {
        let mut prefix = None;
        let mut ret = self.parse_ncname();
        if ret.is_some() && self.cur() == b':' {
            prefix = ret;
            self.next();
            ret = self.parse_ncname();
        }
        (ret, prefix)
    }

    fn scan_name_str(&mut self) -> Option<String> {
        match scan_name(self.s, self.cur, false) {
            None => {
                self.err(XPATH_EXPR_ERROR);
                None
            }
            Some(e) if e == self.cur => None,
            Some(e) => Some(String::from_utf8_lossy(&self.s[self.cur..e]).into_owned()),
        }
    }

    fn comp_number(&mut self) {
        if self.error != 0 {
            return;
        }
        if self.cur() != b'.' && !is_ascii_digit(self.cur()) {
            self.err(XPATH_NUMBER_ERROR);
            return;
        }
        let mut ret: f64 = 0.0;
        let mut ok = false;
        while is_ascii_digit(self.cur()) {
            ret = ret * 10.0 + (self.cur() - b'0') as f64;
            ok = true;
            self.next();
        }
        if self.cur() == b'.' {
            let mut frac = 0;
            let mut fraction: f64 = 0.0;
            self.next();
            if !is_ascii_digit(self.cur()) && !ok {
                self.err(XPATH_NUMBER_ERROR);
                return;
            }
            while self.cur() == b'0' {
                frac += 1;
                self.next();
            }
            let max = frac + 20;
            while is_ascii_digit(self.cur()) && frac < max {
                fraction = fraction * 10.0 + (self.cur() - b'0') as f64;
                frac += 1;
                self.next();
            }
            fraction /= 10f64.powi(frac);
            ret += fraction;
            while is_ascii_digit(self.cur()) {
                self.next();
            }
        }
        if self.cur() == b'e' || self.cur() == b'E' {
            self.next();
            let mut neg = false;
            if self.cur() == b'-' {
                neg = true;
                self.next();
            } else if self.cur() == b'+' {
                self.next();
            }
            let mut exponent: i32 = 0;
            while is_ascii_digit(self.cur()) {
                if exponent < 1000000 {
                    exponent = exponent * 10 + (self.cur() - b'0') as i32;
                }
                self.next();
            }
            if neg {
                exponent = -exponent;
            }
            ret *= 10f64.powf(exponent as f64);
        }
        self.push_value(XObj::Num(ret));
    }

    fn parse_literal(&mut self) -> Option<String> {
        let quote = match self.cur() {
            b'"' => b'"',
            b'\'' => b'\'',
            _ => {
                self.err(XPATH_START_LITERAL_ERROR);
                return None;
            }
        };
        self.next();
        let q = self.cur;
        while self.cur() != quote {
            if self.cur() == 0 && self.cur >= self.s.len() {
                self.err(XPATH_UNFINISHED_LITERAL_ERROR);
                return None;
            }
            match utf8_at(self.s, self.cur) {
                Some((c, l)) if is_xml_char(c) => self.cur += l,
                _ => {
                    self.err(XPATH_INVALID_CHAR_ERROR);
                    return None;
                }
            }
        }
        let r = String::from_utf8_lossy(&self.s[q..self.cur]).into_owned();
        self.next();
        Some(r)
    }

    fn comp_literal(&mut self) {
        if let Some(s) = self.parse_literal() {
            self.push_value(XObj::Str(s));
        }
    }

    fn comp_variable_reference(&mut self) {
        self.skip_blanks();
        if self.cur() != b'$' {
            self.err(XPATH_VARIABLE_REF_ERROR);
            return;
        }
        self.next();
        let (name, prefix) = self.parse_qname();
        let name = match name {
            Some(n) => n,
            None => {
                self.err(XPATH_VARIABLE_REF_ERROR);
                return;
            }
        };
        self.comp.last = -1;
        self.push_long(OpKind::Variable, 0, 0, 0, Some(name), prefix);
        self.skip_blanks();
    }

    fn comp_function_call(&mut self) {
        let (name, prefix) = self.parse_qname();
        let name = match name {
            Some(n) => n,
            None => {
                self.err(XPATH_EXPR_ERROR);
                return;
            }
        };
        self.skip_blanks();
        if self.cur() != b'(' {
            self.err(XPATH_EXPR_ERROR);
            return;
        }
        self.next();
        self.skip_blanks();
        let sort = !(prefix.is_none() && name == "count");
        self.comp.last = -1;
        let mut nbargs = 0;
        if self.cur() != b')' {
            while self.cur < self.s.len() {
                let op1 = self.comp.last;
                self.comp.last = -1;
                self.compile_expr(sort);
                if self.error != XPATH_EXPRESSION_OK {
                    return;
                }
                let l = self.comp.last;
                self.push_binary(OpKind::Arg, op1, l, 0, 0);
                nbargs += 1;
                if self.cur() == b')' {
                    break;
                }
                if self.cur() != b',' {
                    self.err(XPATH_EXPR_ERROR);
                    return;
                }
                self.next();
                self.skip_blanks();
            }
        }
        self.push_long(OpKind::Function, nbargs, 0, 0, Some(name), prefix);
        self.next();
        self.skip_blanks();
    }

    fn comp_primary_expr(&mut self) {
        self.skip_blanks();
        if self.cur() == b'$' {
            self.comp_variable_reference();
        } else if self.cur() == b'(' {
            self.next();
            self.skip_blanks();
            self.compile_expr(true);
            if self.error != 0 {
                return;
            }
            if self.cur() != b')' {
                self.err(XPATH_EXPR_ERROR);
                return;
            }
            self.next();
            self.skip_blanks();
        } else if is_ascii_digit(self.cur()) || (self.cur() == b'.' && is_ascii_digit(self.nxt(1)))
        {
            self.comp_number();
        } else if self.cur() == b'\'' || self.cur() == b'"' {
            self.comp_literal();
        } else {
            self.comp_function_call();
        }
        self.skip_blanks();
    }

    fn comp_filter_expr(&mut self) {
        self.comp_primary_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'[' {
            self.comp_predicate(true);
            self.skip_blanks();
        }
    }

    fn comp_path_expr(&mut self) {
        let mut lc = true;
        self.skip_blanks();
        let c = self.cur();
        if c == b'$'
            || c == b'('
            || is_ascii_digit(c)
            || c == b'\''
            || c == b'"'
            || (c == b'.' && is_ascii_digit(self.nxt(1)))
        {
            lc = false;
        } else if c == b'*' || c == b'/' || c == b'@' || c == b'.' {
            lc = true;
        } else {
            self.skip_blanks();
            let name = self.scan_name_str();
            match name {
                Some(n) if n.contains("::") => lc = true,
                Some(n) => {
                    let mut len = n.len();
                    while self.nxt(len) != 0 {
                        let ch = self.nxt(len);
                        if ch == b'/' {
                            lc = true;
                            break;
                        } else if is_blank_ch(ch) {
                            // ignore blanks
                        } else if ch == b':' {
                            lc = true;
                            break;
                        } else if ch == b'(' {
                            lc = matches!(
                                n.as_str(),
                                "node" | "text" | "comment" | "processing-instruction"
                            );
                            break;
                        } else {
                            // '[', '<', '>', '=', and anything else: element name
                            lc = true;
                            break;
                        }
                        len += 1;
                    }
                    if self.nxt(len) == 0 {
                        lc = true;
                    }
                }
                None => {
                    self.err(XPATH_EXPR_ERROR);
                    return;
                }
            }
        }
        if lc {
            if self.cur() == b'/' {
                self.push_leave(OpKind::Root, 0, 0);
            } else {
                self.push_leave(OpKind::Node, 0, 0);
            }
            self.comp_location_path();
        } else {
            self.comp_filter_expr();
            if self.error != 0 {
                return;
            }
            if self.cur() == b'/' && self.nxt(1) == b'/' {
                self.skip(2);
                self.skip_blanks();
                self.push_long(
                    OpKind::Collect,
                    AXIS_DESCENDANT_OR_SELF,
                    NODE_TEST_TYPE,
                    NODE_TYPE_NODE,
                    None,
                    None,
                );
                self.comp_relative_location_path();
            } else if self.cur() == b'/' {
                self.comp_relative_location_path();
            }
        }
        self.skip_blanks();
    }

    fn comp_union_expr(&mut self) {
        self.comp_path_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'|' {
            let op1 = self.comp.last;
            self.push_leave(OpKind::Node, 0, 0);
            self.next();
            self.skip_blanks();
            self.comp_path_expr();
            let l = self.comp.last;
            self.push_binary(OpKind::Union, op1, l, 0, 0);
            self.skip_blanks();
        }
    }

    fn comp_unary_expr(&mut self) {
        let mut minus = false;
        let mut found = false;
        self.skip_blanks();
        while self.cur() == b'-' {
            minus = !minus;
            found = true;
            self.next();
            self.skip_blanks();
        }
        self.comp_union_expr();
        if self.error != 0 {
            return;
        }
        if found {
            let l = self.comp.last;
            if minus {
                self.push_unary(OpKind::Plus, l, 2, 0);
            } else {
                self.push_unary(OpKind::Plus, l, 3, 0);
            }
        }
    }

    fn comp_multiplicative_expr(&mut self) {
        self.comp_unary_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        loop {
            let c = self.cur();
            let op = if c == b'*' {
                0
            } else if c == b'd' && self.nxt(1) == b'i' && self.nxt(2) == b'v' {
                1
            } else if c == b'm' && self.nxt(1) == b'o' && self.nxt(2) == b'd' {
                2
            } else {
                break;
            };
            let op1 = self.comp.last;
            if op == 0 {
                self.next();
            } else {
                self.skip(3);
            }
            self.skip_blanks();
            self.comp_unary_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::Mult, op1, l, op, 0);
            self.skip_blanks();
        }
    }

    fn comp_additive_expr(&mut self) {
        self.comp_multiplicative_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'+' || self.cur() == b'-' {
            let plus = if self.cur() == b'+' { 1 } else { 0 };
            let op1 = self.comp.last;
            self.next();
            self.skip_blanks();
            self.comp_multiplicative_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::Plus, op1, l, plus, 0);
            self.skip_blanks();
        }
    }

    fn comp_relational_expr(&mut self) {
        self.comp_additive_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'<' || self.cur() == b'>' {
            let inf = if self.cur() == b'<' { 1 } else { 0 };
            let strict = if self.nxt(1) == b'=' { 0 } else { 1 };
            let op1 = self.comp.last;
            self.next();
            if strict == 0 {
                self.next();
            }
            self.skip_blanks();
            self.comp_additive_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::Cmp, op1, l, inf, strict);
            self.skip_blanks();
        }
    }

    fn comp_equality_expr(&mut self) {
        self.comp_relational_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'=' || (self.cur() == b'!' && self.nxt(1) == b'=') {
            let eq = if self.cur() == b'=' { 1 } else { 0 };
            let op1 = self.comp.last;
            self.next();
            if eq == 0 {
                self.next();
            }
            self.skip_blanks();
            self.comp_relational_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::Equal, op1, l, eq, 0);
            self.skip_blanks();
        }
    }

    fn comp_and_expr(&mut self) {
        self.comp_equality_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'a' && self.nxt(1) == b'n' && self.nxt(2) == b'd' {
            let op1 = self.comp.last;
            self.skip(3);
            self.skip_blanks();
            self.comp_equality_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::And, op1, l, 0, 0);
            self.skip_blanks();
        }
    }

    fn compile_expr(&mut self, sort: bool) {
        if self.depth >= XPATH_MAX_RECURSION_DEPTH {
            self.err(XPATH_RECURSION_LIMIT_EXCEEDED);
            return;
        }
        self.depth += 10;
        self.comp_and_expr();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'o' && self.nxt(1) == b'r' {
            let op1 = self.comp.last;
            self.skip(2);
            self.skip_blanks();
            self.comp_and_expr();
            if self.error != 0 {
                return;
            }
            let l = self.comp.last;
            self.push_binary(OpKind::Or, op1, l, 0, 0);
            self.skip_blanks();
        }
        if sort
            && self.comp.last >= 0
            && self.comp.steps[self.comp.last as usize].op != OpKind::Value
        {
            let l = self.comp.last;
            self.push_unary(OpKind::Sort, l, 0, 0);
        }
        self.depth -= 10;
    }

    fn comp_predicate(&mut self, filter: bool) {
        let op1 = self.comp.last;
        self.skip_blanks();
        if self.cur() != b'[' {
            self.err(XPATH_INVALID_PREDICATE_ERROR);
            return;
        }
        self.next();
        self.skip_blanks();
        self.comp.last = -1;
        self.compile_expr(filter);
        if self.error != 0 {
            return;
        }
        if self.cur() != b']' {
            self.err(XPATH_INVALID_PREDICATE_ERROR);
            return;
        }
        let l = self.comp.last;
        if filter {
            self.push_binary(OpKind::Filter, op1, l, 0, 0);
        } else {
            self.push_binary(OpKind::Predicate, op1, l, 0, 0);
        }
        self.next();
        self.skip_blanks();
    }

    /// Returns (test, type, prefix, name).
    fn comp_node_test(
        &mut self,
        name_in: Option<String>,
    ) -> (i32, i32, Option<String>, Option<String>) {
        self.skip_blanks();
        if name_in.is_none() && self.cur() == b'*' {
            self.next();
            return (NODE_TEST_ALL, 0, None, None);
        }
        let name = match name_in {
            Some(n) => Some(n),
            None => self.parse_ncname(),
        };
        let name = match name {
            Some(n) => n,
            None => {
                self.err(XPATH_EXPR_ERROR);
                return (0, 0, None, None);
            }
        };
        let blanks = is_blank_ch(self.cur());
        self.skip_blanks();
        if self.cur() == b'(' {
            self.next();
            let ty = match name.as_str() {
                "comment" => NODE_TYPE_COMMENT,
                "node" => NODE_TYPE_NODE,
                "processing-instruction" => NODE_TYPE_PI,
                "text" => NODE_TYPE_TEXT,
                _ => {
                    self.err(XPATH_EXPR_ERROR);
                    return (0, 0, None, None);
                }
            };
            let mut test = NODE_TEST_TYPE;
            self.skip_blanks();
            let mut rname = Some(name);
            if ty == NODE_TYPE_PI {
                rname = None;
                if self.cur() != b')' {
                    rname = self.parse_literal();
                    test = NODE_TEST_PI;
                    self.skip_blanks();
                }
            }
            if self.cur() != b')' {
                self.err(XPATH_UNCLOSED_ERROR);
                return (0, 0, None, None);
            }
            self.next();
            return (test, ty, None, rname);
        }
        let mut prefix = None;
        let mut name = Some(name);
        if !blanks && self.cur() == b':' {
            self.next();
            prefix = name;
            if self.cur() == b'*' {
                self.next();
                return (NODE_TEST_ALL, 0, prefix, None);
            }
            name = self.parse_ncname();
            if name.is_none() {
                self.err(XPATH_EXPR_ERROR);
                return (0, 0, None, None);
            }
        }
        (NODE_TEST_NAME, 0, prefix, name)
    }

    fn comp_step(&mut self) {
        self.skip_blanks();
        if self.cur() == b'.' && self.nxt(1) == b'.' {
            self.skip(2);
            self.skip_blanks();
            self.push_long(
                OpKind::Collect,
                AXIS_PARENT,
                NODE_TEST_TYPE,
                NODE_TYPE_NODE,
                None,
                None,
            );
        } else if self.cur() == b'.' {
            self.next();
            self.skip_blanks();
        } else {
            let mut name: Option<String> = None;
            let axis;
            if self.cur() == b'*' {
                axis = AXIS_CHILD;
            } else {
                name = self.parse_ncname();
                if let Some(n) = &name {
                    let a = axis_name(n);
                    if a != 0 {
                        self.skip_blanks();
                        if self.cur() == b':' && self.nxt(1) == b':' {
                            self.skip(2);
                            name = None;
                            axis = a;
                        } else {
                            axis = AXIS_CHILD;
                        }
                    } else {
                        axis = AXIS_CHILD;
                    }
                } else if self.cur() == b'@' {
                    self.next();
                    axis = AXIS_ATTRIBUTE;
                } else {
                    axis = AXIS_CHILD;
                }
            }
            if self.error != XPATH_EXPRESSION_OK {
                return;
            }
            let (test, ty, prefix, name) = self.comp_node_test(name);
            if test == 0 {
                return;
            }
            let op1 = self.comp.last;
            self.comp.last = -1;
            self.skip_blanks();
            while self.cur() == b'[' {
                self.comp_predicate(false);
                if self.error != 0 {
                    return;
                }
            }
            let l = self.comp.last;
            self.add(op1, l, OpKind::Collect, axis, test, ty, None, name, prefix);
        }
    }

    fn comp_relative_location_path(&mut self) {
        self.skip_blanks();
        if self.cur() == b'/' && self.nxt(1) == b'/' {
            self.skip(2);
            self.skip_blanks();
            self.push_long(
                OpKind::Collect,
                AXIS_DESCENDANT_OR_SELF,
                NODE_TEST_TYPE,
                NODE_TYPE_NODE,
                None,
                None,
            );
        } else if self.cur() == b'/' {
            self.next();
            self.skip_blanks();
        }
        self.comp_step();
        if self.error != 0 {
            return;
        }
        self.skip_blanks();
        while self.cur() == b'/' {
            if self.cur() == b'/' && self.nxt(1) == b'/' {
                self.skip(2);
                self.skip_blanks();
                self.push_long(
                    OpKind::Collect,
                    AXIS_DESCENDANT_OR_SELF,
                    NODE_TEST_TYPE,
                    NODE_TYPE_NODE,
                    None,
                    None,
                );
                self.comp_step();
            } else if self.cur() == b'/' {
                self.next();
                self.skip_blanks();
                self.comp_step();
            }
            if self.error != 0 {
                return;
            }
            self.skip_blanks();
        }
    }

    fn comp_location_path(&mut self) {
        self.skip_blanks();
        if self.cur() != b'/' {
            self.comp_relative_location_path();
        } else {
            while self.cur() == b'/' {
                if self.cur() == b'/' && self.nxt(1) == b'/' {
                    self.skip(2);
                    self.skip_blanks();
                    self.push_long(
                        OpKind::Collect,
                        AXIS_DESCENDANT_OR_SELF,
                        NODE_TEST_TYPE,
                        NODE_TYPE_NODE,
                        None,
                        None,
                    );
                    self.comp_relative_location_path();
                } else if self.cur() == b'/' {
                    self.next();
                    self.skip_blanks();
                    let c = self.cur();
                    if c != 0
                        && (is_ascii_letter(c)
                            || c >= 0x80
                            || c == b'_'
                            || c == b'.'
                            || c == b'@'
                            || c == b'*')
                    {
                        self.comp_relative_location_path();
                    }
                }
                if self.error != 0 {
                    return;
                }
            }
        }
    }
}

fn axis_name(name: &str) -> i32 {
    match name {
        "ancestor" => AXIS_ANCESTOR,
        "ancestor-or-self" => AXIS_ANCESTOR_OR_SELF,
        "attribute" => AXIS_ATTRIBUTE,
        "child" => AXIS_CHILD,
        "descendant" => AXIS_DESCENDANT,
        "descendant-or-self" => AXIS_DESCENDANT_OR_SELF,
        "following" => AXIS_FOLLOWING,
        "following-sibling" => AXIS_FOLLOWING_SIBLING,
        "namespace" => AXIS_NAMESPACE,
        "parent" => AXIS_PARENT,
        "preceding" => AXIS_PRECEDING,
        "preceding-sibling" => AXIS_PRECEDING_SIBLING,
        "self" => AXIS_SELF,
        _ => 0,
    }
}

/// xmlXPathCtxtCompile: compile an expression; on failure returns the
/// error messages (each to be reported followed by a newline).
pub fn compile(expr: &str) -> Result<CompExpr, Vec<String>> {
    let mut p = Parser {
        s: expr.as_bytes(),
        cur: 0,
        error: 0,
        comp: CompExpr {
            steps: Vec::new(),
            last: -1,
            expr: expr.to_string(),
        },
        depth: 0,
        errors: Vec::new(),
    };
    p.compile_expr(true);
    if p.error != XPATH_EXPRESSION_OK {
        return Err(p.errors);
    }
    if p.cur < p.s.len() {
        p.err(XPATH_EXPR_ERROR);
        return Err(p.errors);
    }
    Ok(p.comp)
}

// ---------------------------------------------------------------------------
// Evaluation context

/// xmlXPathContext: the dynamic context.
#[derive(Clone)]
pub struct XpCtx {
    pub node: Option<NodeId>,
    pub doc: Option<NodeId>,
    pub size: i32,
    pub pos: i32,
    /// in-scope namespaces (prefix, href), nearest first
    pub namespaces: Rc<Vec<(String, String)>>,
    pub depth: i32,
}

impl Default for XpCtx {
    fn default() -> Self {
        XpCtx {
            node: None,
            doc: None,
            size: 1,
            pos: 1,
            namespaces: Rc::new(Vec::new()),
            depth: 0,
        }
    }
}

/// xmlXPathParserContext: the evaluation state of one expression.
pub struct PCtx {
    pub stack: Vec<XObj>,
    pub error: i32,
    /// value stack frame of the running function call
    pub frame: usize,
}

impl PCtx {
    pub fn new() -> Self {
        PCtx {
            stack: Vec::new(),
            error: 0,
            frame: 0,
        }
    }
    pub fn pop(&mut self) -> Option<XObj> {
        if self.stack.len() <= self.frame {
            return None;
        }
        self.stack.pop()
    }
    pub fn push(&mut self, v: XObj) {
        self.stack.push(v);
    }
    pub fn top(&self) -> Option<&XObj> {
        if self.stack.len() <= self.frame {
            None
        } else {
            self.stack.last()
        }
    }
    pub fn top_mut(&mut self) -> Option<&mut XObj> {
        if self.stack.len() <= self.frame {
            None
        } else {
            self.stack.last_mut()
        }
    }
    pub fn nargs_ok(&self, n: usize) -> bool {
        self.stack.len() >= self.frame + n
    }
}

impl Default for PCtx {
    fn default() -> Self {
        Self::new()
    }
}

/// The embedding context (the XSLT transformation).
pub trait Host {
    fn xp(&mut self) -> &mut XpCtx;
    /// Variable lookup; None when undefined (the host may report its own
    /// error first).
    fn lookup_var(&mut self, name: &str, uri: Option<&str>) -> Option<XObj>;
    /// Call a function registered by the host. Returns false when there is
    /// no such function (the core library is then searched).
    fn call_function(&mut self, p: &mut PCtx, name: &str, uri: Option<&str>, nargs: usize) -> bool;
    /// Report an XPath error message (without trailing newline).
    fn xpath_error(&mut self, msg: &str);
    /// The registered namespaces (ctxt->nsHash).
    fn ns_hash_lookup(&self, _prefix: &str) -> Option<String> {
        None
    }
}

/// Raise an XPath error (xmlXPathErr): only the first error is reported.
pub fn xp_err<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, code: i32) {
    if p.error != 0 {
        return;
    }
    p.error = code;
    h.xpath_error(error_message(code));
}

pub fn xp_err_fmt<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, code: i32, msg: &str) {
    if p.error != 0 {
        return;
    }
    p.error = code;
    h.xpath_error(msg);
}

// ---------------------------------------------------------------------------
// Node helpers

fn tw<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    tree::with(f)
}

pub fn ntype(n: NodeId) -> NType {
    tw(|t| t.nodes[n].t)
}

/// xmlNodeGetContent / xmlXPathCastNodeToString
pub fn node_string(t: &Tree, n: NodeId) -> String {
    let nd = &t.nodes[n];
    match nd.t {
        NType::NamespaceDecl => nd.content.clone(),
        _ => t.node_get_content(n),
    }
}

pub fn node_string_id(n: NodeId) -> String {
    tw(|t| node_string(t, n))
}

pub fn is_doc_type(t: NType) -> bool {
    matches!(t, NType::Document | NType::HtmlDocument)
}

/// Name of the fake document node of result tree fragments.
pub const RTF_NAME: &str = " fake node libxslt";

pub fn is_rtf(t: &Tree, n: NodeId) -> bool {
    let nd = &t.nodes[n];
    is_doc_type(nd.t) && nd.name == RTF_NAME
}

// ---------------------------------------------------------------------------
// Document order

thread_local! {
    /// Document order of the nodes of documents that do not change during
    /// a transformation (index of each node, attributes included).
    static ORDER: RefCell<HashMap<NodeId, HashMap<NodeId, u32>>> = RefCell::new(HashMap::new());
}

/// Forget cached document orders (the trees may have changed).
pub fn invalidate_order() {
    ORDER.with(|o| o.borrow_mut().clear());
}

fn root_of(t: &Tree, mut n: NodeId) -> NodeId {
    while let Some(p) = t.nodes[n].parent {
        n = p;
    }
    n
}

fn build_order(t: &Tree, root: NodeId) -> HashMap<NodeId, u32> {
    let mut m = HashMap::new();
    let mut idx = 0u32;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        m.insert(n, idx);
        idx += 1;
        let nd = &t.nodes[n];
        if nd.t == NType::Element {
            let mut a = nd.properties;
            while let Some(x) = a {
                m.insert(x, idx);
                idx += 1;
                let mut c = t.nodes[x].children;
                while let Some(y) = c {
                    m.insert(y, idx);
                    idx += 1;
                    c = t.nodes[y].next;
                }
                a = t.nodes[x].next;
            }
        }
        if nd.t != NType::Attribute {
            let mut kids = Vec::new();
            let mut c = nd.children;
            while let Some(x) = c {
                kids.push(x);
                c = t.nodes[x].next;
            }
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        }
    }
    m
}

/// Sort key: (document rank, position). Namespace nodes sort right after
/// their element.
fn order_key(t: &Tree, n: NodeId, roots: &mut Vec<NodeId>) -> (usize, u64) {
    let nd = &t.nodes[n];
    let (base, sub) = if nd.t == NType::NamespaceDecl {
        match nd.parent {
            Some(p) => (p, 1u64),
            None => (n, 0),
        }
    } else {
        (n, 0)
    };
    let root = root_of(t, base);
    let rank = match roots.iter().position(|&r| r == root) {
        Some(i) => i,
        None => {
            roots.push(root);
            roots.len() - 1
        }
    };
    let pos = ORDER.with(|o| {
        let mut o = o.borrow_mut();
        let m = o.entry(root).or_insert_with(|| build_order(t, root));
        if !m.contains_key(&base) {
            *m = build_order(t, root);
        }
        m.get(&base).copied().unwrap_or(u32::MAX)
    });
    (rank, ((pos as u64) << 1) | sub)
}

/// xmlXPathNodeSetSort: sort in document order (stable).
pub fn sort_nodes(v: &mut [NodeId]) {
    if v.len() < 2 {
        return;
    }
    tw(|t| {
        let mut roots = Vec::new();
        let mut keyed: Vec<((usize, u64), usize, NodeId)> = v
            .iter()
            .enumerate()
            .map(|(i, &n)| (order_key(t, n, &mut roots), i, n))
            .collect();
        // namespace nodes of one element keep their relative order
        keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        for (i, k) in keyed.into_iter().enumerate() {
            v[i] = k.2;
        }
    });
}

/// Compare two nodes in document order: -1, 0, 1.
pub fn cmp_nodes(a: NodeId, b: NodeId) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    tw(|t| {
        let mut roots = Vec::new();
        let ka = order_key(t, a, &mut roots);
        let kb = order_key(t, b, &mut roots);
        ka.cmp(&kb)
    })
}

/// xmlXPathNodeSetMerge: append the nodes of `b` not already in `a`.
pub fn merge(a: &mut Vec<NodeId>, b: &[NodeId]) {
    if b.is_empty() {
        return;
    }
    if a.is_empty() {
        // no duplicate check against an empty first set
        a.extend_from_slice(b);
        return;
    }
    let mut seen: HashSet<NodeId> = a.iter().copied().collect();
    for &n in b {
        if seen.insert(n) {
            a.push(n);
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions

/// xmlXPathStringEvalNumber
pub fn string_eval_number(s: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && is_blank_ch(b[i]) {
        i += 1;
    }
    let mut isneg = false;
    if i < b.len() && b[i] == b'-' {
        isneg = true;
        i += 1;
    }
    let c = b.get(i).copied().unwrap_or(0);
    if c != b'.' && !c.is_ascii_digit() {
        return f64::NAN;
    }
    let mut ret: f64 = 0.0;
    let mut ok = false;
    while i < b.len() && b[i].is_ascii_digit() {
        ret = ret * 10.0 + (b[i] - b'0') as f64;
        ok = true;
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let c = b.get(i).copied().unwrap_or(0);
        if !c.is_ascii_digit() && !ok {
            return f64::NAN;
        }
        let mut frac = 0;
        let mut fraction = 0.0;
        while i < b.len() && b[i] == b'0' {
            frac += 1;
            i += 1;
        }
        let max = frac + 20;
        while i < b.len() && b[i].is_ascii_digit() && frac < max {
            fraction = fraction * 10.0 + (b[i] - b'0') as f64;
            frac += 1;
            i += 1;
        }
        fraction /= 10f64.powi(frac);
        ret += fraction;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let mut exponent: i32 = 0;
    let mut exp_neg = false;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && b[i] == b'-' {
            exp_neg = true;
            i += 1;
        } else if i < b.len() && b[i] == b'+' {
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            if exponent < 1000000 {
                exponent = exponent * 10 + (b[i] - b'0') as i32;
            }
            i += 1;
        }
    }
    while i < b.len() && is_blank_ch(b[i]) {
        i += 1;
    }
    if i != b.len() {
        return f64::NAN;
    }
    if isneg {
        ret = -ret;
    }
    if exp_neg {
        exponent = -exponent;
    }
    ret * 10f64.powf(exponent as f64)
}

/// xmlXPathCastNumberToString
pub fn number_to_string(v: f64) -> String {
    goro_ext_xml::xpath::num_to_string(v)
}

pub fn nodeset_to_string(ns: &[NodeId]) -> String {
    if ns.is_empty() {
        return String::new();
    }
    if ns.len() == 1 {
        return node_string_id(ns[0]);
    }
    let mut v = ns.to_vec();
    sort_nodes(&mut v);
    node_string_id(v[0])
}

/// xmlXPathCastToString
pub fn cast_to_string(v: &XObj) -> String {
    match v {
        XObj::NodeSet(n) | XObj::Tree(n) => nodeset_to_string(n),
        XObj::Str(s) => s.clone(),
        XObj::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        XObj::Num(n) => number_to_string(*n),
        XObj::External(_) => String::new(),
    }
}

/// xmlXPathCastToNumber
pub fn cast_to_number(v: &XObj) -> f64 {
    match v {
        XObj::NodeSet(n) | XObj::Tree(n) => string_eval_number(&nodeset_to_string(n)),
        XObj::Str(s) => string_eval_number(s),
        XObj::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        XObj::Num(n) => *n,
        XObj::External(_) => f64::NAN,
    }
}

/// xmlXPathCastToBoolean
pub fn cast_to_boolean(v: &XObj) -> bool {
    match v {
        XObj::NodeSet(n) | XObj::Tree(n) => !n.is_empty(),
        XObj::Str(s) => !s.is_empty(),
        XObj::Num(n) => !(n.is_nan() || *n == 0.0),
        XObj::Bool(b) => *b,
        XObj::External(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Stack helpers used by functions

/// CAST_TO_STRING on the top of the stack.
pub fn cast_top_string(p: &mut PCtx) {
    if let Some(v) = p.top_mut() {
        if !matches!(v, XObj::Str(_)) {
            let s = cast_to_string(v);
            *v = XObj::Str(s);
        }
    }
}

pub fn cast_top_number(p: &mut PCtx) {
    if let Some(v) = p.top_mut() {
        if !matches!(v, XObj::Num(_)) {
            let n = cast_to_number(v);
            *v = XObj::Num(n);
        }
    }
}

pub fn cast_top_boolean(p: &mut PCtx) {
    if let Some(v) = p.top_mut() {
        if !matches!(v, XObj::Bool(_)) {
            let b = cast_to_boolean(v);
            *v = XObj::Bool(b);
        }
    }
}

/// CHECK_ARITY
pub fn check_arity<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, nargs: usize, x: usize) -> bool {
    if nargs != x {
        xp_err(h, p, XPATH_INVALID_ARITY);
        return false;
    }
    if !p.nargs_ok(x) {
        xp_err(h, p, XPATH_STACK_ERROR);
        return false;
    }
    true
}

pub fn top_is_str(p: &PCtx) -> bool {
    matches!(p.top(), Some(XObj::Str(_)))
}

pub fn pop_string(p: &mut PCtx) -> String {
    match p.pop() {
        Some(XObj::Str(s)) => s,
        Some(o) => cast_to_string(&o),
        None => String::new(),
    }
}

pub fn pop_number(p: &mut PCtx) -> f64 {
    match p.pop() {
        Some(XObj::Num(n)) => n,
        Some(o) => cast_to_number(&o),
        None => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// Evaluation

/// Evaluate a compiled expression (xmlXPathCompiledEval). None on error
/// (already reported).
pub fn eval<H: Host + ?Sized>(h: &mut H, comp: &CompExpr) -> Option<XObj> {
    let mut p = PCtx::new();
    run_eval(h, &mut p, comp, false);
    if p.error == XPATH_EXPRESSION_OK {
        if p.stack.len() != 1 {
            xp_err(h, &mut p, XPATH_STACK_ERROR);
            return None;
        }
        return p.stack.pop();
    }
    None
}

/// xmlXPathCompiledEvalToBoolean: Some(bool) or None on error.
pub fn eval_to_boolean<H: Host + ?Sized>(h: &mut H, comp: &CompExpr) -> Option<bool> {
    let mut p = PCtx::new();
    let r = run_eval(h, &mut p, comp, true);
    if p.error == XPATH_EXPRESSION_OK {
        if !p.stack.is_empty() {
            xp_err(h, &mut p, XPATH_STACK_ERROR);
            return None;
        }
        if r < 0 {
            return None;
        }
        return Some(r == 1);
    }
    None
}

fn run_eval<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, comp: &CompExpr, to_bool: bool) -> i32 {
    if comp.last < 0 {
        xp_err(h, p, XPATH_STACK_ERROR);
        return -1;
    }
    let old_depth = h.xp().depth;
    let r = if to_bool {
        let r = op_eval_to_boolean(h, p, comp, comp.last as usize, false);
        if p.error != 0 { -1 } else { r }
    } else {
        op_eval(h, p, comp, comp.last as usize);
        0
    };
    h.xp().depth = old_depth;
    r
}

fn step(comp: &CompExpr, i: i32) -> &StepOp {
    &comp.steps[i as usize]
}

fn enter<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> bool {
    if h.xp().depth >= XPATH_MAX_RECURSION_DEPTH {
        xp_err(h, p, XPATH_RECURSION_LIMIT_EXCEEDED);
        return false;
    }
    h.xp().depth += 1;
    true
}

fn leave<H: Host + ?Sized>(h: &mut H) {
    h.xp().depth -= 1;
}

fn ns_lookup<H: Host + ?Sized>(h: &mut H, prefix: &str) -> Option<String> {
    if prefix == "xml" {
        return Some(XML_XML_NAMESPACE.to_string());
    }
    let ns = h.xp().namespaces.clone();
    for (p, u) in ns.iter() {
        if p == prefix {
            return Some(u.clone());
        }
    }
    h.ns_hash_lookup(prefix)
}

/// xmlXPathCompOpEval
pub fn op_eval<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, comp: &CompExpr, opi: usize) {
    if p.error != 0 {
        return;
    }
    if !enter(h, p) {
        return;
    }
    op_eval_inner(h, p, comp, opi);
    leave(h);
}

fn op_eval_inner<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, comp: &CompExpr, opi: usize) {
    let op = &comp.steps[opi];
    match op.op {
        OpKind::End => {}
        OpKind::And | OpKind::Or => {
            let is_and = op.op == OpKind::And;
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            boolean_function(h, p, 1);
            let v = match p.top() {
                Some(XObj::Bool(b)) => *b,
                _ => return,
            };
            if is_and && !v {
                return;
            }
            if !is_and && v {
                return;
            }
            let arg2 = p.pop();
            op_eval(h, p, comp, op.ch2 as usize);
            if p.error != 0 {
                return;
            }
            boolean_function(h, p, 1);
            let a2 = matches!(arg2, Some(XObj::Bool(true)));
            if let Some(XObj::Bool(b)) = p.top_mut() {
                if is_and {
                    *b &= a2;
                } else {
                    *b |= a2;
                }
            }
        }
        OpKind::Equal => {
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            op_eval(h, p, comp, op.ch2 as usize);
            if p.error != 0 {
                return;
            }
            let r = if op.value != 0 {
                equal_values(h, p)
            } else {
                not_equal_values(h, p)
            };
            p.push(XObj::Bool(r));
        }
        OpKind::Cmp => {
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            op_eval(h, p, comp, op.ch2 as usize);
            if p.error != 0 {
                return;
            }
            let r = compare_values(h, p, op.value != 0, op.value2 != 0);
            p.push(XObj::Bool(r));
        }
        OpKind::Plus => {
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            if op.ch2 != -1 {
                op_eval(h, p, comp, op.ch2 as usize);
            }
            if p.error != 0 {
                return;
            }
            match op.value {
                0 => arith(h, p, |a, b| a - b),
                1 => arith(h, p, |a, b| a + b),
                2 => {
                    cast_top_number(p);
                    match p.top_mut() {
                        Some(XObj::Num(n)) => *n = -*n,
                        _ => xp_err(h, p, XPATH_INVALID_TYPE),
                    }
                }
                _ => {
                    cast_top_number(p);
                    if !matches!(p.top(), Some(XObj::Num(_))) {
                        xp_err(h, p, XPATH_INVALID_TYPE);
                    }
                }
            }
        }
        OpKind::Mult => {
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            op_eval(h, p, comp, op.ch2 as usize);
            if p.error != 0 {
                return;
            }
            match op.value {
                0 => arith(h, p, |a, b| a * b),
                1 => arith(h, p, |a, b| a / b),
                _ => arith(h, p, |a, b| if b == 0.0 { f64::NAN } else { a % b }),
            }
        }
        OpKind::Union => {
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            op_eval(h, p, comp, op.ch2 as usize);
            if p.error != 0 {
                return;
            }
            let arg2 = p.pop();
            let arg1 = p.pop();
            match (arg1, arg2) {
                (Some(XObj::NodeSet(mut a)), Some(XObj::NodeSet(b))) => {
                    merge(&mut a, &b);
                    p.push(XObj::NodeSet(a));
                }
                _ => xp_err(h, p, XPATH_INVALID_TYPE),
            }
        }
        OpKind::Root => {
            let d = {
                let x = h.xp();
                x.doc.or_else(|| x.node.map(|n| tw(|t| root_of(t, n))))
            };
            p.push(XObj::NodeSet(d.into_iter().collect()));
        }
        OpKind::Node => {
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
            }
            if p.error != 0 {
                return;
            }
            if op.ch2 != -1 {
                op_eval(h, p, comp, op.ch2 as usize);
            }
            if p.error != 0 {
                return;
            }
            let n = h.xp().node;
            p.push(XObj::NodeSet(n.into_iter().collect()));
        }
        OpKind::Collect => {
            if op.ch1 == -1 {
                return;
            }
            op_eval(h, p, comp, op.ch1 as usize);
            if p.error != 0 {
                return;
            }
            node_collect_and_test(h, p, comp, opi, false);
        }
        OpKind::Value => {
            p.push(op.obj.clone().unwrap_or(XObj::Str(String::new())));
        }
        OpKind::Variable => {
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
            }
            let name = op.s4.clone().unwrap_or_default();
            match &op.s5 {
                None => match h.lookup_var(&name, None) {
                    Some(v) => p.push(v),
                    None => {
                        xp_err_fmt(
                            h,
                            p,
                            XPATH_UNDEF_VARIABLE_ERROR,
                            &format!("Undefined variable: {}", name),
                        );
                    }
                },
                Some(prefix) => {
                    let uri = match ns_lookup(h, prefix) {
                        Some(u) => u,
                        None => {
                            xp_err_fmt(
                                h,
                                p,
                                XPATH_UNDEF_PREFIX_ERROR,
                                &format!("Undefined namespace prefix: {}", prefix),
                            );
                            return;
                        }
                    };
                    match h.lookup_var(&name, Some(&uri)) {
                        Some(v) => p.push(v),
                        None => {
                            xp_err_fmt(
                                h,
                                p,
                                XPATH_UNDEF_VARIABLE_ERROR,
                                &format!("Undefined variable: {}:{}", prefix, name),
                            );
                        }
                    }
                }
            }
        }
        OpKind::Function => {
            let frame = p.stack.len();
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
                if p.error != XPATH_EXPRESSION_OK {
                    return;
                }
            }
            let nargs = op.value as usize;
            if p.stack.len() < frame + nargs {
                xp_err(h, p, XPATH_INVALID_OPERAND);
                return;
            }
            let name = op.s4.clone().unwrap_or_default();
            let uri = match &op.s5 {
                None => None,
                Some(prefix) => match ns_lookup(h, prefix) {
                    Some(u) => Some(u),
                    None => {
                        xp_err_fmt(
                            h,
                            p,
                            XPATH_UNDEF_PREFIX_ERROR,
                            &format!("Undefined namespace prefix: {}", prefix),
                        );
                        return;
                    }
                },
            };
            let old_frame = p.frame;
            p.frame = p.stack.len() - nargs;
            let found = if h.call_function(p, &name, uri.as_deref(), nargs) {
                true
            } else if uri.is_none() {
                call_core(h, p, &name, nargs)
            } else {
                false
            };
            p.frame = old_frame;
            if !found {
                let msg = match &op.s5 {
                    None => format!("Unregistered function: {}", name),
                    Some(pr) => format!("Unregistered function: {}:{}", pr, name),
                };
                xp_err_fmt(h, p, XPATH_UNKNOWN_FUNC_ERROR, &msg);
                return;
            }
            if p.error == XPATH_EXPRESSION_OK && p.stack.len() != frame + 1 {
                xp_err(h, p, XPATH_STACK_ERROR);
            }
        }
        OpKind::Arg => {
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
                if p.error != 0 {
                    return;
                }
            }
            if op.ch2 != -1 {
                op_eval(h, p, comp, op.ch2 as usize);
            }
        }
        OpKind::Predicate | OpKind::Filter => {
            // Optimization for ()[1]: the first node only
            if op.ch1 != -1 && op.ch2 != -1 {
                let c1 = step(comp, op.ch1);
                let c2 = step(comp, op.ch2);
                if (c1.op == OpKind::Sort || c1.op == OpKind::Filter)
                    && c2.op == OpKind::Value
                    && matches!(c2.obj, Some(XObj::Num(n)) if n == 1.0)
                {
                    op_eval(h, p, comp, op.ch1 as usize);
                    if p.error != 0 {
                        return;
                    }
                    if let Some(XObj::NodeSet(v)) = p.top_mut() {
                        if v.len() > 1 {
                            v.truncate(1);
                        }
                    }
                    return;
                }
                // ()[last()]
                if c1.op == OpKind::Sort && c2.op == OpKind::Sort && c2.ch1 != -1 {
                    let f = step(comp, c2.ch1);
                    if f.op == OpKind::Function
                        && f.s5.is_none()
                        && f.value == 0
                        && f.s4.as_deref() == Some("last")
                    {
                        op_eval(h, p, comp, op.ch1 as usize);
                        if p.error != 0 {
                            return;
                        }
                        if let Some(XObj::NodeSet(v)) = p.top_mut() {
                            if v.len() > 1 {
                                let l = *v.last().unwrap();
                                v.clear();
                                v.push(l);
                            }
                        }
                        return;
                    }
                }
            }
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
            }
            if p.error != 0 {
                return;
            }
            if op.ch2 == -1 {
                return;
            }
            if p.top().is_none() {
                return;
            }
            if !matches!(p.top(), Some(XObj::NodeSet(_))) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return;
            }
            let mut set = match p.pop() {
                Some(XObj::NodeSet(v)) => v,
                _ => Vec::new(),
            };
            let len = set.len() as i32;
            node_set_filter(h, p, comp, &mut set, op.ch2 as usize, 1, len);
            p.push(XObj::NodeSet(set));
        }
        OpKind::Sort => {
            if op.ch1 != -1 {
                op_eval(h, p, comp, op.ch1 as usize);
            }
            if p.error != 0 {
                return;
            }
            if let Some(XObj::NodeSet(v)) = p.top_mut() {
                if v.len() > 1 {
                    sort_nodes(v);
                }
            }
        }
    }
}

fn arith<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, f: impl Fn(f64, f64) -> f64) {
    let arg = match p.pop() {
        Some(a) => a,
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            return;
        }
    };
    let val = cast_to_number(&arg);
    cast_top_number(p);
    match p.top_mut() {
        Some(XObj::Num(n)) => *n = f(*n, val),
        _ => xp_err(h, p, XPATH_INVALID_TYPE),
    }
}

/// xmlXPathCompOpEvalToBoolean
fn op_eval_to_boolean<H: Host + ?Sized>(
    h: &mut H,
    p: &mut PCtx,
    comp: &CompExpr,
    mut opi: usize,
    is_predicate: bool,
) -> i32 {
    loop {
        let op = &comp.steps[opi];
        let res_obj = match op.op {
            OpKind::End => return 0,
            OpKind::Value => {
                let o = op.obj.clone().unwrap_or(XObj::Bool(false));
                if is_predicate {
                    return evaluate_predicate_result(h, &o) as i32;
                }
                return cast_to_boolean(&o) as i32;
            }
            OpKind::Sort => {
                if op.ch1 != -1 {
                    opi = op.ch1 as usize;
                    continue;
                }
                return 0;
            }
            OpKind::Collect => {
                if op.ch1 == -1 {
                    return 0;
                }
                op_eval(h, p, comp, op.ch1 as usize);
                if p.error != XPATH_EXPRESSION_OK {
                    return -1;
                }
                node_collect_and_test(h, p, comp, opi, true);
                if p.error != XPATH_EXPRESSION_OK {
                    return -1;
                }
                match p.pop() {
                    Some(o) => o,
                    None => return -1,
                }
            }
            _ => {
                op_eval(h, p, comp, opi);
                if p.error != XPATH_EXPRESSION_OK {
                    return -1;
                }
                match p.pop() {
                    Some(o) => o,
                    None => return -1,
                }
            }
        };
        return match res_obj {
            XObj::Bool(b) => b as i32,
            o if is_predicate => evaluate_predicate_result(h, &o) as i32,
            o => cast_to_boolean(&o) as i32,
        };
    }
}

fn evaluate_predicate_result<H: Host + ?Sized>(h: &mut H, res: &XObj) -> bool {
    match res {
        XObj::Bool(b) => *b,
        XObj::Num(n) => *n == h.xp().pos as f64,
        XObj::NodeSet(v) | XObj::Tree(v) => !v.is_empty(),
        XObj::Str(s) => !s.is_empty(),
        XObj::External(_) => false,
    }
}

/// xmlXPathNodeSetFilter
fn node_set_filter<H: Host + ?Sized>(
    h: &mut H,
    p: &mut PCtx,
    comp: &CompExpr,
    set: &mut Vec<NodeId>,
    filter_op: usize,
    min_pos: i32,
    max_pos: i32,
) {
    if set.is_empty() {
        return;
    }
    if (set.len() as i32) < min_pos {
        set.clear();
        return;
    }
    let (oldnode, olddoc, oldcs, oldpp) = {
        let x = h.xp();
        (x.node, x.doc, x.size, x.pos)
    };
    h.xp().size = set.len() as i32;
    let mut out = Vec::new();
    let mut pos = 1;
    let mut i = 0;
    while i < set.len() {
        let node = set[i];
        {
            let x = h.xp();
            x.node = Some(node);
            x.pos = i as i32 + 1;
        }
        let (nt, ndoc) = tw(|t| (t.nodes[node].t, t.nodes[node].doc));
        if nt != NType::NamespaceDecl {
            if let Some(d) = ndoc {
                h.xp().doc = Some(d);
            }
        }
        let res = op_eval_to_boolean(h, p, comp, filter_op, true);
        if p.error != XPATH_EXPRESSION_OK {
            break;
        }
        if res < 0 {
            xp_err(h, p, XPATH_EXPR_ERROR);
            break;
        }
        if res != 0 && pos >= min_pos && pos <= max_pos {
            out.push(node);
        }
        if res != 0 {
            if pos == max_pos {
                break;
            }
            pos += 1;
        }
        i += 1;
    }
    *set = out;
    let x = h.xp();
    x.node = oldnode;
    x.doc = olddoc;
    x.size = oldcs;
    x.pos = oldpp;
}

fn op_eval_predicate<H: Host + ?Sized>(
    h: &mut H,
    p: &mut PCtx,
    comp: &CompExpr,
    opi: usize,
    set: &mut Vec<NodeId>,
    min_pos: i32,
    max_pos: i32,
) {
    let op = &comp.steps[opi];
    if op.ch1 != -1 {
        if step(comp, op.ch1).op != OpKind::Predicate {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            return;
        }
        if !enter(h, p) {
            return;
        }
        let len = set.len() as i32;
        op_eval_predicate(h, p, comp, op.ch1 as usize, set, 1, len);
        leave(h);
        if p.error != 0 {
            return;
        }
    }
    if op.ch2 != -1 {
        node_set_filter(h, p, comp, set, op.ch2 as usize, min_pos, max_pos);
    }
}

fn is_positional_predicate(comp: &CompExpr, op: &StepOp) -> Option<i32> {
    if op.op != OpKind::Predicate && op.op != OpKind::Filter {
        return None;
    }
    if op.ch2 == -1 {
        return None;
    }
    let e = step(comp, op.ch2);
    if e.op == OpKind::Value {
        if let Some(XObj::Num(f)) = e.obj {
            if f > i32::MIN as f64 && f < i32::MAX as f64 {
                let m = f as i32;
                if f == m as f64 {
                    return Some(m);
                }
            }
        }
    }
    None
}

fn is_fake_parent(t: &Tree, p: NodeId) -> bool {
    let nd = &t.nodes[p];
    nd.t == NType::Element && (nd.name.starts_with(' ') || nd.name == "fake node libxslt")
}

/// xmlXPathNextDescendant
fn next_descendant(
    t: &Tree,
    ctx: NodeId,
    xdoc: Option<NodeId>,
    cur: Option<NodeId>,
) -> Option<NodeId> {
    let mut cur = match cur {
        None => {
            if matches!(t.nodes[ctx].t, NType::Attribute | NType::NamespaceDecl) {
                return None;
            }
            if Some(ctx) == xdoc {
                return t.nodes[ctx].children;
            }
            return t.nodes[ctx].children;
        }
        Some(c) => c,
    };
    if t.nodes[cur].t == NType::NamespaceDecl {
        return None;
    }
    if let Some(c) = t.nodes[cur].children {
        if t.nodes[c].t != NType::EntityDecl {
            cur = c;
            if t.nodes[cur].t != NType::Dtd {
                return Some(cur);
            }
        }
    }
    if cur == ctx {
        return None;
    }
    while let Some(nx) = t.nodes[cur].next {
        cur = nx;
        if !matches!(t.nodes[cur].t, NType::EntityDecl | NType::Dtd) {
            return Some(cur);
        }
    }
    loop {
        cur = t.nodes[cur].parent?;
        if cur == ctx {
            return None;
        }
        if let Some(nx) = t.nodes[cur].next {
            return Some(nx);
        }
    }
}

fn parent_like(t: &Tree, n: NodeId, xdoc: Option<NodeId>, first: bool) -> Option<NodeId> {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Attribute => nd.parent,
        NType::NamespaceDecl => match nd.parent {
            Some(p) if t.nodes[p].t != NType::NamespaceDecl => Some(p),
            _ => None,
        },
        NType::Document | NType::HtmlDocument | NType::DocumentFrag | NType::DocumentType => None,
        _ => match nd.parent {
            None => {
                if first {
                    xdoc
                } else {
                    None
                }
            }
            Some(p) => {
                if is_fake_parent(t, p) {
                    None
                } else {
                    Some(p)
                }
            }
        },
    }
}

/// xmlXPathNextAncestor
fn next_ancestor(
    t: &Tree,
    ctx: NodeId,
    xdoc: Option<NodeId>,
    cur: Option<NodeId>,
) -> Option<NodeId> {
    match cur {
        None => parent_like(t, ctx, xdoc, true),
        Some(c) => {
            if let Some(d) = xdoc {
                if t.nodes[d].children == Some(c) {
                    return Some(d);
                }
                if c == d {
                    return None;
                }
            }
            parent_like(t, c, xdoc, false)
        }
    }
}

/// xmlXPathNextFollowing
fn next_following(
    t: &Tree,
    ctx: NodeId,
    xdoc: Option<NodeId>,
    cur: Option<NodeId>,
) -> Option<NodeId> {
    if let Some(c) = cur {
        let nd = &t.nodes[c];
        if !matches!(nd.t, NType::Attribute | NType::NamespaceDecl) && nd.children.is_some() {
            return nd.children;
        }
    }
    let mut c = match cur {
        Some(c) => c,
        None => {
            let nd = &t.nodes[ctx];
            match nd.t {
                NType::Attribute => nd.parent?,
                NType::NamespaceDecl => match nd.parent {
                    Some(p) if t.nodes[p].t != NType::NamespaceDecl => p,
                    _ => return None,
                },
                _ => ctx,
            }
        }
    };
    if t.nodes[c].t == NType::Document {
        return None;
    }
    if let Some(nx) = t.nodes[c].next {
        return Some(nx);
    }
    loop {
        c = t.nodes[c].parent?;
        if Some(c) == xdoc {
            return None;
        }
        if t.nodes[c].next.is_some() && t.nodes[c].t != NType::Document {
            return t.nodes[c].next;
        }
    }
}

/// xmlXPathNextPrecedingInternal (with its ctxt->ancestor state)
fn next_preceding(
    t: &Tree,
    ctx: NodeId,
    xdoc: Option<NodeId>,
    cur: Option<NodeId>,
    ancestor: &mut Option<NodeId>,
) -> Option<NodeId> {
    let mut c = match cur {
        None => {
            let nd = &t.nodes[ctx];
            let c = match nd.t {
                NType::Attribute => nd.parent?,
                NType::NamespaceDecl => match nd.parent {
                    Some(p) if t.nodes[p].t != NType::NamespaceDecl => p,
                    _ => return None,
                },
                _ => ctx,
            };
            *ancestor = t.nodes[c].parent;
            c
        }
        Some(c) => c,
    };
    if matches!(t.nodes[c].t, NType::NamespaceDecl | NType::Document) {
        return None;
    }
    if let Some(pv) = t.nodes[c].prev {
        if t.nodes[pv].t == NType::Dtd {
            c = pv;
        }
    }
    let doc_children = xdoc.and_then(|d| t.nodes[d].children);
    while t.nodes[c].prev.is_none() {
        c = t.nodes[c].parent?;
        if Some(c) == doc_children {
            return None;
        }
        if Some(c) != *ancestor {
            return Some(c);
        }
        *ancestor = t.nodes[c].parent;
    }
    if t.nodes[c].t == NType::Document {
        return None;
    }
    c = t.nodes[c].prev?;
    while let Some(l) = t.nodes[c].last {
        c = l;
    }
    Some(c)
}

/// Axis traversal: all nodes of `axis` from `ctx` in axis order (the
/// successive results of libxml2's traversal function).
fn axis_nodes(
    t: &mut Tree,
    axis: i32,
    ctx: NodeId,
    xdoc: Option<NodeId>,
    element_only: bool,
) -> Vec<NodeId> {
    let mut out = Vec::new();
    let nt = t.nodes[ctx].t;
    let is_attr_or_ns = matches!(nt, NType::Attribute | NType::NamespaceDecl);
    match axis {
        AXIS_SELF => out.push(ctx),
        AXIS_CHILD => {
            if element_only {
                // xmlXPathNextChildElement
                let first = match nt {
                    NType::Element | NType::DocumentFrag | NType::EntityRef | NType::Entity => {
                        t.nodes[ctx].children
                    }
                    NType::Document | NType::HtmlDocument => t.root_element(ctx),
                    _ => None,
                };
                let mut c = first;
                if let Some(x) = c {
                    if t.nodes[x].t != NType::Element {
                        c = None;
                        let mut y = t.nodes[x].next;
                        while let Some(z) = y {
                            if t.nodes[z].t == NType::Element {
                                c = Some(z);
                                break;
                            }
                            y = t.nodes[z].next;
                        }
                    }
                }
                while let Some(x) = c {
                    out.push(x);
                    if !matches!(
                        t.nodes[x].t,
                        NType::Element
                            | NType::Text
                            | NType::EntityRef
                            | NType::Entity
                            | NType::CData
                            | NType::Pi
                            | NType::Comment
                    ) {
                        break;
                    }
                    let mut y = t.nodes[x].next;
                    c = None;
                    while let Some(z) = y {
                        if t.nodes[z].t == NType::Element {
                            c = Some(z);
                            break;
                        }
                        y = t.nodes[z].next;
                    }
                }
            } else {
                match nt {
                    NType::Attribute
                    | NType::NamespaceDecl
                    | NType::ElementDecl
                    | NType::AttributeDecl
                    | NType::EntityDecl => {}
                    _ => {
                        let mut c = t.nodes[ctx].children;
                        while let Some(x) = c {
                            out.push(x);
                            if is_doc_type(t.nodes[x].t) {
                                break;
                            }
                            c = t.nodes[x].next;
                        }
                    }
                }
            }
        }
        AXIS_DESCENDANT | AXIS_DESCENDANT_OR_SELF => {
            if axis == AXIS_DESCENDANT_OR_SELF {
                out.push(ctx);
                if is_attr_or_ns {
                    return out;
                }
            }
            let mut cur = None;
            while let Some(n) = next_descendant(t, ctx, xdoc, cur) {
                out.push(n);
                cur = Some(n);
            }
        }
        AXIS_PARENT => {
            if let Some(p) = parent_like(t, ctx, xdoc, true) {
                out.push(p);
            }
        }
        AXIS_ANCESTOR | AXIS_ANCESTOR_OR_SELF => {
            if axis == AXIS_ANCESTOR_OR_SELF {
                out.push(ctx);
            }
            let mut cur = None;
            while let Some(n) = next_ancestor(t, ctx, xdoc, cur) {
                out.push(n);
                cur = Some(n);
                if out.len() > 100000 {
                    break;
                }
            }
        }
        AXIS_ATTRIBUTE => {
            if nt == NType::Element && Some(ctx) != xdoc {
                let mut a = t.nodes[ctx].properties;
                while let Some(x) = a {
                    out.push(x);
                    a = t.nodes[x].next;
                }
            }
        }
        AXIS_NAMESPACE => {
            if nt == NType::Element {
                out = goro_ext_xml::xpath::namespace_nodes(t, ctx);
            }
        }
        AXIS_FOLLOWING_SIBLING => {
            if is_attr_or_ns || nt == NType::Document {
                return out;
            }
            let mut c = t.nodes[ctx].next;
            while let Some(x) = c {
                out.push(x);
                if t.nodes[x].t == NType::Document {
                    break;
                }
                c = t.nodes[x].next;
            }
        }
        AXIS_PRECEDING_SIBLING => {
            if is_attr_or_ns || nt == NType::Document {
                return out;
            }
            let mut cur = ctx;
            loop {
                if out.last().is_some() {
                    if let Some(pv) = t.nodes[cur].prev {
                        if t.nodes[pv].t == NType::Dtd {
                            cur = pv;
                        }
                    }
                }
                if t.nodes[cur].t == NType::Document {
                    break;
                }
                match t.nodes[cur].prev {
                    Some(pv) => {
                        out.push(pv);
                        cur = pv;
                    }
                    None => break,
                }
            }
        }
        AXIS_FOLLOWING => {
            let mut cur = None;
            while let Some(n) = next_following(t, ctx, xdoc, cur) {
                out.push(n);
                cur = Some(n);
            }
        }
        AXIS_PRECEDING => {
            let mut cur = None;
            let mut anc = None;
            while let Some(n) = next_preceding(t, ctx, xdoc, cur, &mut anc) {
                out.push(n);
                cur = Some(n);
            }
        }
        _ => {}
    }
    out
}

/// xmlXPathNodeCollectAndTest
fn node_collect_and_test<H: Host + ?Sized>(
    h: &mut H,
    p: &mut PCtx,
    comp: &CompExpr,
    opi: usize,
    to_bool: bool,
) {
    let op = &comp.steps[opi];
    let axis = op.value;
    let test = op.value2;
    let ty = op.value3;
    let prefix = op.s5.clone();
    let name = op.s4.clone();
    if !matches!(p.top(), Some(XObj::NodeSet(_))) {
        xp_err(h, p, XPATH_INVALID_TYPE);
        return;
    }
    let obj = match p.pop() {
        Some(XObj::NodeSet(v)) => v,
        _ => Vec::new(),
    };
    let mut uri: Option<String> = None;
    if let Some(pr) = &prefix {
        match ns_lookup(h, pr) {
            Some(u) => uri = Some(u),
            None => {
                xp_err_fmt(
                    h,
                    p,
                    XPATH_UNDEF_PREFIX_ERROR,
                    &format!("Undefined namespace prefix: {}", pr),
                );
                return;
            }
        }
    }
    if obj.is_empty() {
        p.push(XObj::NodeSet(obj));
        return;
    }
    let dedup = !matches!(
        axis,
        AXIS_ATTRIBUTE | AXIS_CHILD | AXIS_NAMESPACE | AXIS_SELF
    );
    let element_only = axis == AXIS_CHILD
        && (test == NODE_TEST_NAME || test == NODE_TEST_ALL)
        && ty == NODE_TYPE_NODE;
    let mut max_pos = 0;
    let mut pred_op: Option<usize> = None;
    let mut has_predicate_range = false;
    let mut has_axis_range = false;
    if op.ch2 != -1 {
        let po = op.ch2 as usize;
        pred_op = Some(po);
        if let Some(m) = is_positional_predicate(comp, &comp.steps[po]) {
            max_pos = m;
            let inner = comp.steps[po].ch1;
            if inner != -1 {
                pred_op = Some(inner as usize);
                has_predicate_range = true;
            } else {
                pred_op = None;
                has_axis_range = true;
            }
        }
    }
    let break_on_first_hit = to_bool && pred_op.is_none();
    let old_node = h.xp().node;
    let xdoc = h.xp().doc;
    let mut out_seq: Vec<NodeId> = Vec::new();
    let mut out_set: HashSet<NodeId> = HashSet::default();
    let uri_ref = uri.as_deref();
    'ctx: for &ctx_node in obj.iter() {
        if p.error != XPATH_EXPRESSION_OK {
            break;
        }
        h.xp().node = Some(ctx_node);
        let cands = tw(|t| axis_nodes(t, axis, ctx_node, xdoc, element_only));
        let mut seq: Vec<NodeId> = Vec::new();
        let mut pos = 0;
        let mut range_end = false;
        let mut first_hit = false;
        for cur in cands {
            let hit = tw(|t| {
                node_test(
                    t,
                    cur,
                    axis,
                    test,
                    ty,
                    prefix.is_some(),
                    uri_ref,
                    name.as_deref(),
                )
            });
            if !hit {
                continue;
            }
            if has_axis_range {
                pos += 1;
                if pos == max_pos {
                    seq.push(cur);
                    range_end = true;
                    break;
                }
            } else {
                seq.push(cur);
                if break_on_first_hit {
                    first_hit = true;
                    break;
                }
            }
        }
        if range_end || first_hit {
            add_merge(&mut out_seq, &mut out_set, seq, dedup);
            if to_bool || first_hit {
                break 'ctx;
            }
            continue;
        }
        if let Some(po) = pred_op {
            if !seq.is_empty() {
                if has_predicate_range {
                    op_eval_predicate(h, p, comp, po, &mut seq, max_pos, max_pos);
                } else {
                    let l = seq.len() as i32;
                    op_eval_predicate(h, p, comp, po, &mut seq, 1, l);
                }
                if p.error != XPATH_EXPRESSION_OK {
                    out_seq.clear();
                    break;
                }
            }
        }
        if !seq.is_empty() {
            add_merge(&mut out_seq, &mut out_set, seq, dedup);
            if to_bool {
                break;
            }
        }
    }
    p.push(XObj::NodeSet(out_seq));
    h.xp().node = old_node;
}

fn add_merge(out: &mut Vec<NodeId>, set: &mut HashSet<NodeId>, seq: Vec<NodeId>, dedup: bool) {
    if out.is_empty() && !dedup {
        for &n in &seq {
            set.insert(n);
        }
        *out = seq;
        return;
    }
    for n in seq {
        if set.insert(n) || !dedup {
            out.push(n);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn node_test(
    t: &Tree,
    cur: NodeId,
    axis: i32,
    test: i32,
    ty: i32,
    has_prefix: bool,
    uri: Option<&str>,
    name: Option<&str>,
) -> bool {
    let nd = &t.nodes[cur];
    match test {
        NODE_TEST_TYPE => {
            if ty == NODE_TYPE_NODE {
                matches!(
                    nd.t,
                    NType::Document
                        | NType::HtmlDocument
                        | NType::Element
                        | NType::Attribute
                        | NType::Pi
                        | NType::Comment
                        | NType::CData
                        | NType::Text
                        | NType::NamespaceDecl
                )
            } else if ty == NODE_TYPE_TEXT {
                matches!(nd.t, NType::Text | NType::CData)
            } else if ty == NODE_TYPE_COMMENT {
                nd.t == NType::Comment
            } else if ty == NODE_TYPE_PI {
                nd.t == NType::Pi
            } else {
                false
            }
        }
        NODE_TEST_PI => nd.t == NType::Pi && name.map(|n| n == nd.name).unwrap_or(true),
        NODE_TEST_ALL => {
            if axis == AXIS_ATTRIBUTE {
                nd.t == NType::Attribute
                    && (!has_prefix || (t.ns_href(nd.ns) == uri && nd.ns.is_some()))
            } else if axis == AXIS_NAMESPACE {
                nd.t == NType::NamespaceDecl
            } else {
                nd.t == NType::Element
                    && (!has_prefix || (nd.ns.is_some() && t.ns_href(nd.ns) == uri))
            }
        }
        NODE_TEST_NAME => {
            let want = if axis == AXIS_ATTRIBUTE {
                NType::Attribute
            } else if axis == AXIS_NAMESPACE {
                NType::NamespaceDecl
            } else {
                NType::Element
            };
            if nd.t != want {
                return false;
            }
            match nd.t {
                NType::Element => {
                    Some(nd.name.as_str()) == name
                        && if !has_prefix {
                            nd.ns.is_none()
                        } else {
                            nd.ns.is_some() && t.ns_href(nd.ns) == uri
                        }
                }
                NType::Attribute => {
                    Some(nd.name.as_str()) == name
                        && if !has_prefix {
                            nd.ns.is_none() || t.ns_prefix(nd.ns).is_none()
                        } else {
                            nd.ns.is_some() && t.ns_href(nd.ns) == uri
                        }
                }
                NType::NamespaceDecl => !nd.name.is_empty() && Some(nd.name.as_str()) == name,
                _ => false,
            }
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Comparisons

fn is_inf(v: f64) -> i32 {
    if v == f64::INFINITY {
        1
    } else if v == f64::NEG_INFINITY {
        -1
    } else {
        0
    }
}

fn equal_values_common(a: &XObj, b: &XObj) -> bool {
    match (a, b) {
        (XObj::Bool(x), XObj::Bool(y)) => x == y,
        (XObj::Bool(x), XObj::Num(n)) | (XObj::Num(n), XObj::Bool(x)) => {
            *x == !(n.is_nan() || *n == 0.0)
        }
        (XObj::Bool(x), XObj::Str(s)) | (XObj::Str(s), XObj::Bool(x)) => *x == !s.is_empty(),
        (XObj::Num(x), XObj::Num(y)) => num_eq(*x, *y),
        (XObj::Num(x), XObj::Str(s)) | (XObj::Str(s), XObj::Num(x)) => {
            num_eq(*x, string_eval_number(s))
        }
        (XObj::Str(x), XObj::Str(y)) => x == y,
        _ => false,
    }
}

fn num_eq(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() {
        return false;
    }
    let (ia, ib) = (is_inf(a), is_inf(b));
    if ia != 0 || ib != 0 {
        return ia == ib;
    }
    a == b
}

fn node_strings(v: &[NodeId]) -> Vec<String> {
    tw(|t| v.iter().map(|&n| node_string(t, n)).collect())
}

fn equal_node_sets(a: &[NodeId], b: &[NodeId], neq: bool) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if !neq {
        let sb: HashSet<NodeId> = b.iter().copied().collect();
        if a.iter().any(|n| sb.contains(n)) {
            return true;
        }
    }
    let sa = node_strings(a);
    let sb = node_strings(b);
    for x in &sa {
        for y in &sb {
            if (x == y) ^ neq {
                return true;
            }
        }
    }
    false
}

fn equal_node_set_float(a: &[NodeId], f: f64, neq: bool) -> bool {
    let mut ret = false;
    for s in node_strings(a) {
        let v = string_eval_number(&s);
        if !v.is_nan() {
            if !neq && v == f {
                return true;
            } else if neq && v != f {
                return true;
            }
        } else if neq {
            ret = true;
        }
    }
    ret
}

fn equal_node_set_string(a: &[NodeId], s: &str, neq: bool) -> bool {
    if a.is_empty() {
        return false;
    }
    for x in node_strings(a) {
        if x == s {
            if neq {
                continue;
            }
            return true;
        } else if neq {
            return true;
        }
    }
    false
}

fn eq_impl<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, neq: bool) -> bool {
    let arg2 = p.pop();
    let arg1 = p.pop();
    let (mut a1, mut a2) = match (arg1, arg2) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            return false;
        }
    };
    if a1.is_nodeset_or_tree() || a2.is_nodeset_or_tree() {
        if !a1.is_nodeset_or_tree() {
            std::mem::swap(&mut a1, &mut a2);
        }
        let ns1 = a1.nodes().cloned().unwrap_or_default();
        return match &a2 {
            XObj::NodeSet(v) | XObj::Tree(v) => equal_node_sets(&ns1, v, neq),
            XObj::Bool(b) => {
                let r = !ns1.is_empty();
                if neq { r != *b } else { r == *b }
            }
            XObj::Num(f) => equal_node_set_float(&ns1, *f, neq),
            XObj::Str(s) => equal_node_set_string(&ns1, s, neq),
            XObj::External(_) => false,
        };
    }
    let r = equal_values_common(&a1, &a2);
    if neq { !r } else { r }
}

pub fn equal_values<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> bool {
    eq_impl(h, p, false)
}

pub fn not_equal_values<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> bool {
    eq_impl(h, p, true)
}

fn cmp_num(a: f64, b: f64, inf: bool, strict: bool) -> bool {
    if a.is_nan() || b.is_nan() {
        return false;
    }
    let (a1, a2) = (is_inf(a), is_inf(b));
    if inf && strict {
        if (a1 == -1 && a2 != -1) || (a2 == 1 && a1 != 1) {
            true
        } else if a1 == 0 && a2 == 0 {
            a < b
        } else {
            false
        }
    } else if inf && !strict {
        if a1 == -1 || a2 == 1 {
            true
        } else if a1 == 0 && a2 == 0 {
            a <= b
        } else {
            false
        }
    } else if !inf && strict {
        if (a1 == 1 && a2 != 1) || (a2 == -1 && a1 != -1) {
            true
        } else if a1 == 0 && a2 == 0 {
            a > b
        } else {
            false
        }
    } else if a1 == 1 || a2 == -1 {
        true
    } else if a1 == 0 && a2 == 0 {
        a >= b
    } else {
        false
    }
}

fn raw_cmp(a: f64, b: f64, inf: bool, strict: bool) -> bool {
    if inf && strict {
        a < b
    } else if inf {
        a <= b
    } else if strict {
        a > b
    } else {
        a >= b
    }
}

fn compare_node_sets(a: &[NodeId], b: &[NodeId], inf: bool, strict: bool) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let vb: Vec<f64> = node_strings(b)
        .iter()
        .map(|s| string_eval_number(s))
        .collect();
    for s in node_strings(a) {
        let v1 = string_eval_number(&s);
        if v1.is_nan() {
            continue;
        }
        for &v2 in &vb {
            if v2.is_nan() {
                continue;
            }
            if raw_cmp(v1, v2, inf, strict) {
                return true;
            }
        }
    }
    false
}

fn compare_node_set_float(a: &[NodeId], f: f64, inf: bool, strict: bool) -> bool {
    for s in node_strings(a) {
        let v = string_eval_number(&s);
        if cmp_num(v, f, inf, strict) {
            return true;
        }
    }
    false
}

fn compare_node_set_value<H: Host + ?Sized>(
    h: &mut H,
    p: &mut PCtx,
    inf: bool,
    strict: bool,
    arg: XObj,
    val: XObj,
) -> bool {
    let ns = arg.nodes().cloned().unwrap_or_default();
    match val {
        XObj::Num(f) => compare_node_set_float(&ns, f, inf, strict),
        XObj::NodeSet(v) | XObj::Tree(v) => compare_node_sets(&ns, &v, inf, strict),
        XObj::Str(s) => compare_node_set_float(&ns, string_eval_number(&s), inf, strict),
        XObj::Bool(b) => {
            p.push(XObj::Bool(!ns.is_empty()));
            p.push(XObj::Bool(b));
            compare_values(h, p, inf, strict)
        }
        XObj::External(_) => false,
    }
}

pub fn compare_values<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, inf: bool, strict: bool) -> bool {
    let arg2 = p.pop();
    let arg1 = p.pop();
    let (a1, a2) = match (arg1, arg2) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            return false;
        }
    };
    if a1.is_nodeset_or_tree() || a2.is_nodeset_or_tree() {
        if a1.is_nodeset_or_tree() && a2.is_nodeset_or_tree() {
            let n1 = a1.nodes().cloned().unwrap_or_default();
            let n2 = a2.nodes().cloned().unwrap_or_default();
            return compare_node_sets(&n1, &n2, inf, strict);
        }
        if a1.is_nodeset_or_tree() {
            return compare_node_set_value(h, p, inf, strict, a1, a2);
        }
        return compare_node_set_value(h, p, !inf, strict, a2, a1);
    }
    let x = cast_to_number(&a1);
    let y = cast_to_number(&a2);
    cmp_num(x, y, inf, strict)
}

// ---------------------------------------------------------------------------
// Core function library

fn call_core<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "last" => {
            if check_arity(h, p, nargs, 0) {
                let s = h.xp().size;
                p.push(XObj::Num(s as f64));
            }
        }
        "position" => {
            if check_arity(h, p, nargs, 0) {
                let s = h.xp().pos;
                p.push(XObj::Num(s as f64));
            }
        }
        "count" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            match p.pop() {
                Some(XObj::NodeSet(v)) | Some(XObj::Tree(v)) => p.push(XObj::Num(v.len() as f64)),
                Some(o) => {
                    p.push(o);
                    xp_err(h, p, XPATH_INVALID_TYPE);
                }
                None => xp_err(h, p, XPATH_INVALID_TYPE),
            }
        }
        "id" => id_function(h, p, nargs),
        "local-name" | "namespace-uri" | "name" => name_functions(h, p, name, nargs),
        "string" => {
            if nargs == 0 {
                let n = h.xp().node;
                let s = n.map(node_string_id).unwrap_or_default();
                p.push(XObj::Str(s));
                return true;
            }
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_string(p);
        }
        "string-length" => {
            if nargs == 0 {
                let n = h.xp().node;
                let l = n.map(|n| node_string_id(n).chars().count()).unwrap_or(0);
                p.push(XObj::Num(l as f64));
                return true;
            }
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_string(p);
            if !top_is_str(p) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return true;
            }
            let s = pop_string(p);
            p.push(XObj::Num(s.chars().count() as f64));
        }
        "concat" => {
            if nargs < 2 {
                check_arity(h, p, nargs, 2);
                return true;
            }
            if !p.nargs_ok(nargs) {
                xp_err(h, p, XPATH_STACK_ERROR);
                return true;
            }
            let mut parts = Vec::with_capacity(nargs);
            for _ in 0..nargs {
                cast_top_string(p);
                parts.push(pop_string(p));
            }
            parts.reverse();
            p.push(XObj::Str(parts.concat()));
        }
        "contains" | "starts-with" => {
            if !check_arity(h, p, nargs, 2) {
                return true;
            }
            cast_top_string(p);
            if !top_is_str(p) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return true;
            }
            let needle = pop_string(p);
            cast_top_string(p);
            if !top_is_str(p) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return true;
            }
            let hay = pop_string(p);
            let r = if name == "contains" {
                hay.contains(&needle)
            } else {
                hay.starts_with(&needle)
            };
            p.push(XObj::Bool(r));
        }
        "substring" => substring_function(h, p, nargs),
        "substring-before" | "substring-after" => {
            if !check_arity(h, p, nargs, 2) {
                return true;
            }
            cast_top_string(p);
            let find = pop_string(p);
            cast_top_string(p);
            let s = pop_string(p);
            if p.error != 0 {
                return true;
            }
            let r = match s.find(&find) {
                None => String::new(),
                Some(i) => {
                    if name == "substring-before" {
                        s[..i].to_string()
                    } else {
                        s[i + find.len()..].to_string()
                    }
                }
            };
            p.push(XObj::Str(r));
        }
        "normalize-space" => {
            if nargs == 0 {
                let n = h.xp().node;
                p.push(XObj::Str(n.map(node_string_id).unwrap_or_default()));
                return call_core(h, p, name, 1);
            }
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_string(p);
            if !top_is_str(p) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return true;
            }
            let s = pop_string(p);
            let mut out = String::with_capacity(s.len());
            let mut blank = false;
            for c in s.trim_start_matches([' ', '\t', '\n', '\r']).chars() {
                if matches!(c, ' ' | '\t' | '\n' | '\r') {
                    blank = true;
                } else {
                    if blank {
                        out.push(' ');
                        blank = false;
                    }
                    out.push(c);
                }
            }
            p.push(XObj::Str(out));
        }
        "translate" => {
            if !check_arity(h, p, nargs, 3) {
                return true;
            }
            cast_top_string(p);
            let to: Vec<char> = pop_string(p).chars().collect();
            cast_top_string(p);
            let from: Vec<char> = pop_string(p).chars().collect();
            cast_top_string(p);
            let s = pop_string(p);
            if p.error != 0 {
                return true;
            }
            let mut out = String::new();
            for c in s.chars() {
                match from.iter().position(|x| *x == c) {
                    Some(i) => {
                        if i < to.len() {
                            out.push(to[i]);
                        }
                    }
                    None => out.push(c),
                }
            }
            p.push(XObj::Str(out));
        }
        "boolean" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            boolean_function(h, p, 1);
        }
        "not" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_boolean(p);
            match p.top_mut() {
                Some(XObj::Bool(b)) => *b = !*b,
                _ => xp_err(h, p, XPATH_INVALID_TYPE),
            }
        }
        "true" | "false" => {
            if check_arity(h, p, nargs, 0) {
                p.push(XObj::Bool(name == "true"));
            }
        }
        "lang" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_string(p);
            if !top_is_str(p) {
                xp_err(h, p, XPATH_INVALID_TYPE);
                return true;
            }
            let lang = pop_string(p);
            let node = h.xp().node;
            let the_lang = node.and_then(|n| {
                tw(|t| {
                    let mut cur = Some(n);
                    while let Some(c) = cur {
                        if t.nodes[c].t == NType::Element {
                            if let Some(a) = t.has_ns_prop(c, "lang", Some(XML_XML_NAMESPACE)) {
                                return Some(t.node_get_content(a));
                            }
                        }
                        cur = t.nodes[c].parent;
                    }
                    None
                })
            });
            let mut ret = false;
            if let Some(tl) = the_lang {
                let lb = lang.as_bytes();
                let tb = tl.as_bytes();
                let mut ok = true;
                let mut i = 0;
                while i < lb.len() {
                    if tb.get(i).map(|c| c.to_ascii_uppercase()) != Some(lb[i].to_ascii_uppercase())
                    {
                        ok = false;
                        break;
                    }
                    i += 1;
                }
                if ok && (i >= tb.len() || tb[i] == b'-') {
                    ret = true;
                }
            }
            p.push(XObj::Bool(ret));
        }
        "number" => {
            if nargs == 0 {
                let n = h.xp().node;
                let v = match n {
                    None => 0.0,
                    Some(n) => string_eval_number(&tw(|t| t.node_get_content(n))),
                };
                p.push(XObj::Num(v));
                return true;
            }
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_number(p);
        }
        "sum" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            match p.pop() {
                Some(XObj::NodeSet(v)) | Some(XObj::Tree(v)) => {
                    let mut r = 0.0;
                    for s in node_strings(&v) {
                        r += string_eval_number(&s);
                    }
                    p.push(XObj::Num(r));
                }
                Some(o) => {
                    p.push(o);
                    xp_err(h, p, XPATH_INVALID_TYPE);
                }
                None => xp_err(h, p, XPATH_INVALID_TYPE),
            }
        }
        "floor" | "ceiling" | "round" => {
            if !check_arity(h, p, nargs, 1) {
                return true;
            }
            cast_top_number(p);
            match p.top_mut() {
                Some(XObj::Num(f)) => {
                    *f = match name {
                        "floor" => f.floor(),
                        "ceiling" => f.ceil(),
                        _ => {
                            if *f >= -0.5 && *f < 0.5 {
                                *f * 0.0
                            } else {
                                let r = f.floor();
                                if *f - r >= 0.5 { r + 1.0 } else { r }
                            }
                        }
                    }
                }
                _ => xp_err(h, p, XPATH_INVALID_TYPE),
            }
        }
        "escape-uri" => escape_uri_function(h, p, nargs),
        _ => return false,
    }
    true
}

pub fn boolean_function<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, _nargs: usize) {
    match p.pop() {
        None => xp_err(h, p, XPATH_INVALID_OPERAND),
        Some(XObj::Bool(b)) => p.push(XObj::Bool(b)),
        Some(o) => {
            let b = cast_to_boolean(&o);
            p.push(XObj::Bool(b));
        }
    }
}

fn substring_function<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, nargs: usize) {
    if nargs < 2 {
        check_arity(h, p, nargs, 2);
        return;
    }
    if nargs > 3 {
        check_arity(h, p, nargs, 3);
        return;
    }
    if !p.nargs_ok(nargs) {
        xp_err(h, p, XPATH_STACK_ERROR);
        return;
    }
    let mut le = 0.0;
    if nargs == 3 {
        cast_top_number(p);
        le = pop_number(p);
    }
    cast_top_number(p);
    let inn = pop_number(p);
    cast_top_string(p);
    let s = pop_string(p);
    let mut i: i64 = 1;
    let mut j: i64 = i32::MAX as i64;
    if !(inn < i32::MAX as f64) {
        i = i32::MAX as i64;
    } else if inn >= 1.0 {
        i = inn as i64;
        if inn - inn.floor() >= 0.5 {
            i += 1;
        }
    }
    if nargs == 3 {
        let mut rin = inn.floor();
        if inn - rin >= 0.5 {
            rin += 1.0;
        }
        let mut rle = le.floor();
        if le - rle >= 0.5 {
            rle += 1.0;
        }
        let end = rin + rle;
        if !(end >= 1.0) {
            j = 1;
        } else if end < i32::MAX as f64 {
            j = end as i64;
        }
    }
    i -= 1;
    j -= 1;
    let chars: Vec<char> = s.chars().collect();
    if i < j && i < chars.len() as i64 {
        let end = (j as usize).min(chars.len());
        let r: String = chars[i as usize..end].iter().collect();
        p.push(XObj::Str(r));
    } else {
        p.push(XObj::Str(String::new()));
    }
}

fn id_function<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, nargs: usize) {
    if !check_arity(h, p, nargs, 1) {
        return;
    }
    let obj = match p.pop() {
        Some(o) => o,
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            return;
        }
    };
    let doc = h.xp().doc;
    let lookup = |ids: &str, out: &mut Vec<NodeId>| {
        let doc = match doc {
            Some(d) => d,
            None => return,
        };
        tw(|t| {
            for id in ids.split([' ', '\t', '\n', '\r']).filter(|s| !s.is_empty()) {
                if let Some(info) = t.doc_info(doc) {
                    if let Some(&a) = info.ids.get(id) {
                        let e = match t.nodes[a].t {
                            NType::Attribute => t.nodes[a].parent,
                            NType::Element => Some(a),
                            _ => None,
                        };
                        if let Some(e) = e {
                            if !out.contains(&e) {
                                out.push(e);
                            }
                        }
                    }
                }
            }
        });
    };
    let mut ret = Vec::new();
    match &obj {
        XObj::NodeSet(v) | XObj::Tree(v) => {
            for s in node_strings(v) {
                let mut part = Vec::new();
                lookup(&s, &mut part);
                merge(&mut ret, &part);
            }
        }
        o => {
            let s = cast_to_string(o);
            lookup(&s, &mut ret);
        }
    }
    p.push(XObj::NodeSet(ret));
}

fn name_functions<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, name: &str, mut nargs: usize) {
    if nargs == 0 {
        let n = h.xp().node;
        p.push(XObj::NodeSet(n.into_iter().collect()));
        nargs = 1;
    }
    if !check_arity(h, p, nargs, 1) {
        return;
    }
    let cur = match p.top() {
        Some(XObj::NodeSet(v)) | Some(XObj::Tree(v)) => v.clone(),
        _ => {
            xp_err(h, p, XPATH_INVALID_TYPE);
            return;
        }
    };
    p.pop();
    if cur.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let n = cur[0];
    let r = tw(|t| {
        let nd = &t.nodes[n];
        match name {
            "local-name" => match nd.t {
                NType::Element | NType::Attribute | NType::Pi => {
                    if nd.name.starts_with(' ') {
                        String::new()
                    } else {
                        nd.name.clone()
                    }
                }
                NType::NamespaceDecl => nd.name.clone(),
                _ => String::new(),
            },
            "namespace-uri" => match nd.t {
                NType::Element | NType::Attribute => t.ns_href(nd.ns).unwrap_or("").to_string(),
                _ => String::new(),
            },
            _ => match nd.t {
                NType::Element | NType::Attribute => {
                    if nd.name.starts_with(' ') {
                        String::new()
                    } else {
                        match t.ns_prefix(nd.ns) {
                            Some(pr) if nd.ns.is_some() => format!("{}:{}", pr, nd.name),
                            _ => nd.name.clone(),
                        }
                    }
                }
                NType::Pi => nd.name.clone(),
                NType::NamespaceDecl => nd.name.clone(),
                _ => String::new(),
            },
        }
    });
    p.push(XObj::Str(r));
}

fn escape_uri_function<H: Host + ?Sized>(h: &mut H, p: &mut PCtx, nargs: usize) {
    if !check_arity(h, p, nargs, 2) {
        return;
    }
    cast_top_boolean(p);
    let escape_reserved = matches!(p.pop(), Some(XObj::Bool(true)));
    cast_top_string(p);
    let s = pop_string(p);
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let keep = c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
            || (c == b'%'
                && i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit())
            || (!escape_reserved
                && matches!(
                    c,
                    b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b','
                ));
        if keep {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
        i += 1;
    }
    p.push(XObj::Str(out));
}

/// Is `name` a function of the core library?
pub fn is_core_function(name: &str) -> bool {
    matches!(
        name,
        "last"
            | "position"
            | "count"
            | "id"
            | "local-name"
            | "namespace-uri"
            | "name"
            | "string"
            | "string-length"
            | "concat"
            | "contains"
            | "starts-with"
            | "substring"
            | "substring-before"
            | "substring-after"
            | "normalize-space"
            | "translate"
            | "boolean"
            | "not"
            | "true"
            | "false"
            | "lang"
            | "number"
            | "sum"
            | "floor"
            | "ceiling"
            | "round"
            | "escape-uri"
    )
}

// ---------------------------------------------------------------------------
// xpathInternals.h stack helpers (with their error reporting)

/// xmlXPathPopNodeSet
pub fn x_pop_node_set<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> Option<Vec<NodeId>> {
    match p.top() {
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            None
        }
        Some(XObj::NodeSet(_)) | Some(XObj::Tree(_)) => match p.pop() {
            Some(XObj::NodeSet(v)) | Some(XObj::Tree(v)) => Some(v),
            _ => None,
        },
        Some(_) => {
            xp_err(h, p, XPATH_INVALID_TYPE);
            None
        }
    }
}

/// xmlXPathPopNumber
pub fn x_pop_number<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> Option<f64> {
    match p.pop() {
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            None
        }
        Some(o) => Some(cast_to_number(&o)),
    }
}

/// xmlXPathPopString
pub fn x_pop_string<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> Option<String> {
    match p.pop() {
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            None
        }
        Some(o) => Some(cast_to_string(&o)),
    }
}

/// xmlXPathPopBoolean
pub fn x_pop_boolean<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) -> Option<bool> {
    match p.pop() {
        None => {
            xp_err(h, p, XPATH_INVALID_OPERAND);
            None
        }
        Some(o) => Some(cast_to_boolean(&o)),
    }
}

/// xmlXPathSetArityError
pub fn x_arity_error<H: Host + ?Sized>(h: &mut H, p: &mut PCtx) {
    xp_err(h, p, XPATH_INVALID_ARITY);
}
