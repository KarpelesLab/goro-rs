//! XPath 1.0 engine over the tree in `tree.rs` (semantics and error
//! messages follow libxml2's xpath.c).

use crate::tree::{self, NType, NodeId, Tree};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum XValue {
    NodeSet(Vec<NodeId>),
    Bool(bool),
    Num(f64),
    Str(String),
}

impl XValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            XValue::NodeSet(_) => "nodeset",
            XValue::Bool(_) => "bool",
            XValue::Num(_) => "number",
            XValue::Str(_) => "string",
        }
    }
}

/// Error codes (index into xmlXPathErrorMessages)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XErr {
    NumberEncoding = 1,
    UnfinishedLiteral = 2,
    StartLiteral = 3,
    VarRef = 4,
    UndefVariable = 5,
    InvalidPredicate = 6,
    InvalidExpr = 7,
    UnknownFunc = 9,
    InvalidOperand = 10,
    InvalidType = 11,
    InvalidArity = 12,
    InvalidCtxtSize = 13,
    InvalidCtxtPosition = 14,
    Syntax = 16,
    UndefPrefix = 19,
    InvalidChar = 21,
    InvalidCtxt = 22,
    StackError = 23,
    /// Error already reported by an extension function (e.g. a PHP exception)
    Ext = 100,
}

impl XErr {
    pub fn message(self) -> &'static str {
        match self {
            XErr::NumberEncoding => "Number encoding",
            XErr::UnfinishedLiteral => "Unfinished literal",
            XErr::StartLiteral => "Start of literal",
            XErr::VarRef => "Expected $ for variable reference",
            XErr::UndefVariable => "Undefined variable",
            XErr::InvalidPredicate => "Invalid predicate",
            XErr::InvalidExpr => "Invalid expression",
            XErr::UnknownFunc => "Unregistered function",
            XErr::InvalidOperand => "Invalid operand",
            XErr::InvalidType => "Invalid type",
            XErr::InvalidArity => "Invalid number of arguments",
            XErr::InvalidCtxtSize => "Invalid context size",
            XErr::InvalidCtxtPosition => "Invalid context position",
            XErr::Syntax => "Syntax error",
            XErr::UndefPrefix => "Undefined namespace prefix",
            XErr::InvalidChar => "Char out of XML range",
            XErr::InvalidCtxt => "Invalid or incomplete context",
            XErr::StackError => "Stack usage error",
            XErr::Ext => "",
        }
    }
}

/// Emit the PHP warning for an XPath error (PHP shows just the message).
pub fn report_error(vm: &mut goro_core::vm::Vm, fname: &str, e: &XErr, _expr: &str) {
    if *e == XErr::Ext || vm.current_exception.is_some() {
        return;
    }
    vm.emit_warning(&format!("{}(): {}", fname, e.message()));
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    DotDot,
    At,
    Comma,
    ColonColon,
    /// NameTest: (prefix, local) where local may be "*"
    NameTest(Option<String>, String),
    NodeType(String),
    Op(&'static str),
    Func(Option<String>, String),
    Axis(String),
    Literal(String),
    Number(f64),
    Var(Option<String>, String),
    Slash,
    SlashSlash,
    Pipe,
}

fn is_ncname_start(c: char) -> bool {
    c == '_' || c.is_alphabetic() || crate::parser::is_name_start_char(c as u32) && c != ':'
}

fn is_ncname_char(c: char) -> bool {
    crate::parser::is_name_char(c as u32) && c != ':'
}

fn lex(s: &str) -> Result<Vec<Tok>, XErr> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut toks: Vec<Tok> = Vec::new();
    let n = chars.len();
    let prev_allows_operator = |toks: &Vec<Tok>| -> bool {
        !matches!(
            toks.last(),
            None | Some(Tok::At)
                | Some(Tok::ColonColon)
                | Some(Tok::LParen)
                | Some(Tok::LBracket)
                | Some(Tok::Comma)
                | Some(Tok::Op(_))
                | Some(Tok::Slash)
                | Some(Tok::SlashSlash)
                | Some(Tok::Pipe)
        )
    };
    while i < n {
        let c = chars[i];
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                toks.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                toks.push(Tok::RBracket);
                i += 1;
            }
            '@' => {
                toks.push(Tok::At);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '|' => {
                toks.push(Tok::Pipe);
                i += 1;
            }
            '/' => {
                if i + 1 < n && chars[i + 1] == '/' {
                    toks.push(Tok::SlashSlash);
                    i += 2;
                } else {
                    toks.push(Tok::Slash);
                    i += 1;
                }
            }
            ':' => {
                if i + 1 < n && chars[i + 1] == ':' {
                    toks.push(Tok::ColonColon);
                    i += 2;
                } else {
                    return Err(XErr::InvalidExpr);
                }
            }
            '.' => {
                if i + 1 < n && chars[i + 1] == '.' {
                    toks.push(Tok::DotDot);
                    i += 2;
                } else if i + 1 < n && chars[i + 1].is_ascii_digit() {
                    let (v, end) = scan_number(&chars, i).unwrap_or((f64::NAN, i + 1));
                    i = end;
                    toks.push(Tok::Number(v));
                } else {
                    toks.push(Tok::Dot);
                    i += 1;
                }
            }
            '"' | '\'' => {
                let q = c;
                let st = i + 1;
                i += 1;
                while i < n && chars[i] != q {
                    i += 1;
                }
                if i >= n {
                    return Err(XErr::UnfinishedLiteral);
                }
                toks.push(Tok::Literal(chars[st..i].iter().collect()));
                i += 1;
            }
            '0'..='9' => {
                let (v, end) = scan_number(&chars, i).unwrap_or((f64::NAN, i + 1));
                i = end;
                toks.push(Tok::Number(v));
            }
            '=' => {
                toks.push(Tok::Op("="));
                i += 1;
            }
            '!' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    toks.push(Tok::Op("!="));
                    i += 2;
                } else {
                    return Err(XErr::InvalidExpr);
                }
            }
            '<' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    toks.push(Tok::Op("<="));
                    i += 2;
                } else {
                    toks.push(Tok::Op("<"));
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    toks.push(Tok::Op(">="));
                    i += 2;
                } else {
                    toks.push(Tok::Op(">"));
                    i += 1;
                }
            }
            '+' => {
                toks.push(Tok::Op("+"));
                i += 1;
            }
            '-' => {
                toks.push(Tok::Op("-"));
                i += 1;
            }
            '*' => {
                if prev_allows_operator(&toks) {
                    toks.push(Tok::Op("*"));
                } else {
                    toks.push(Tok::NameTest(None, "*".into()));
                }
                i += 1;
            }
            '$' => {
                i += 1;
                let st = i;
                if i >= n || !is_ncname_start(chars[i]) {
                    return Err(XErr::VarRef);
                }
                while i < n && is_ncname_char(chars[i]) {
                    i += 1;
                }
                let first: String = chars[st..i].iter().collect();
                if i + 1 < n && chars[i] == ':' && is_ncname_start(chars[i + 1]) {
                    i += 1;
                    let st2 = i;
                    while i < n && is_ncname_char(chars[i]) {
                        i += 1;
                    }
                    toks.push(Tok::Var(Some(first), chars[st2..i].iter().collect()));
                } else {
                    toks.push(Tok::Var(None, first));
                }
            }
            c if is_ncname_start(c) => {
                let st = i;
                while i < n && is_ncname_char(chars[i]) {
                    i += 1;
                }
                let first: String = chars[st..i].iter().collect();
                if prev_allows_operator(&toks) {
                    match first.as_str() {
                        "and" | "or" | "mod" | "div" => {
                            toks.push(Tok::Op(match first.as_str() {
                                "and" => "and",
                                "or" => "or",
                                "mod" => "mod",
                                _ => "div",
                            }));
                            continue;
                        }
                        _ => return Err(XErr::InvalidExpr),
                    }
                }
                let mut prefix = None;
                let mut local = first.clone();
                if i < n && chars[i] == ':' && !(i + 1 < n && chars[i + 1] == ':') {
                    if i + 1 < n && chars[i + 1] == '*' {
                        toks.push(Tok::NameTest(Some(first), "*".into()));
                        i += 2;
                        continue;
                    }
                    if i + 1 < n && is_ncname_start(chars[i + 1]) {
                        i += 1;
                        let st2 = i;
                        while i < n && is_ncname_char(chars[i]) {
                            i += 1;
                        }
                        prefix = Some(first.clone());
                        local = chars[st2..i].iter().collect();
                    } else {
                        return Err(XErr::InvalidExpr);
                    }
                }
                // lookahead
                let mut j = i;
                while j < n
                    && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\n' || chars[j] == '\r')
                {
                    j += 1;
                }
                if prefix.is_none() && j + 1 < n && chars[j] == ':' && chars[j + 1] == ':' {
                    toks.push(Tok::Axis(local));
                    toks.push(Tok::ColonColon);
                    i = j + 2;
                    continue;
                }
                if j < n && chars[j] == '(' {
                    if prefix.is_none()
                        && matches!(
                            local.as_str(),
                            "comment" | "text" | "processing-instruction" | "node"
                        )
                    {
                        toks.push(Tok::NodeType(local));
                    } else {
                        toks.push(Tok::Func(prefix, local));
                    }
                    continue;
                }
                toks.push(Tok::NameTest(prefix, local));
            }
            _ => return Err(XErr::InvalidExpr),
        }
    }
    Ok(toks)
}

// ---------------------------------------------------------------------------
// AST

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Ancestor,
    AncestorOrSelf,
    Attribute,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Namespace,
    Parent,
    Preceding,
    PrecedingSibling,
    SelfAxis,
}

#[derive(Clone, Debug)]
enum NodeTest {
    Name(Option<String>, String),
    Node,
    Text,
    Comment,
    Pi(Option<String>),
}

#[derive(Clone, Debug)]
struct Step {
    axis: Axis,
    test: NodeTest,
    preds: Vec<Expr>,
}

#[derive(Clone, Debug)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Arith(&'static str, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Path { start: PathStart, steps: Vec<Step> },
    Filter(Box<Expr>, Vec<Expr>),
    Literal(String),
    Number(f64),
    Var(Option<String>, String),
    Func(Option<String>, String, Vec<Expr>),
}

#[derive(Clone, Debug)]
enum PathStart {
    Context,
    Root,
    Expr(Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    fn expr(&mut self) -> Result<Expr, XErr> {
        self.or_expr()
    }
    fn or_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.and_expr()?;
        while self.is_op("or") {
            self.pos += 1;
            let r = self.and_expr()?;
            l = Expr::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn and_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.eq_expr()?;
        while self.is_op("and") {
            self.pos += 1;
            let r = self.eq_expr()?;
            l = Expr::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn eq_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.rel_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("=")) => "=",
                Some(Tok::Op("!=")) => "!=",
                _ => break,
            };
            self.pos += 1;
            let r = self.rel_expr()?;
            l = Expr::Cmp(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn rel_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.add_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("<")) => "<",
                Some(Tok::Op("<=")) => "<=",
                Some(Tok::Op(">")) => ">",
                Some(Tok::Op(">=")) => ">=",
                _ => break,
            };
            self.pos += 1;
            let r = self.add_expr()?;
            l = Expr::Cmp(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn add_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("+")) => "+",
                Some(Tok::Op("-")) => "-",
                _ => break,
            };
            self.pos += 1;
            let r = self.mul_expr()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn mul_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("*")) => "*",
                Some(Tok::Op("div")) => "div",
                Some(Tok::Op("mod")) => "mod",
                _ => break,
            };
            self.pos += 1;
            let r = self.unary_expr()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn unary_expr(&mut self) -> Result<Expr, XErr> {
        if self.is_op("-") {
            self.pos += 1;
            let e = self.unary_expr()?;
            return Ok(Expr::Neg(Box::new(e)));
        }
        self.union_expr()
    }
    fn union_expr(&mut self) -> Result<Expr, XErr> {
        let mut l = self.path_expr()?;
        while matches!(self.peek(), Some(Tok::Pipe)) {
            self.pos += 1;
            let r = self.path_expr()?;
            l = Expr::Union(Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::NameTest(..))
                | Some(Tok::NodeType(_))
                | Some(Tok::Axis(_))
                | Some(Tok::At)
                | Some(Tok::Dot)
                | Some(Tok::DotDot)
        )
    }
    fn path_expr(&mut self) -> Result<Expr, XErr> {
        match self.peek() {
            Some(Tok::Slash) => {
                self.pos += 1;
                let mut steps = Vec::new();
                if self.starts_step() {
                    self.rel_path(&mut steps)?;
                }
                Ok(Expr::Path {
                    start: PathStart::Root,
                    steps,
                })
            }
            Some(Tok::SlashSlash) => {
                self.pos += 1;
                let mut steps = vec![Step {
                    axis: Axis::DescendantOrSelf,
                    test: NodeTest::Node,
                    preds: vec![],
                }];
                if !self.starts_step() {
                    return Err(XErr::InvalidExpr);
                }
                self.rel_path(&mut steps)?;
                Ok(Expr::Path {
                    start: PathStart::Root,
                    steps,
                })
            }
            Some(Tok::LParen)
            | Some(Tok::Literal(_))
            | Some(Tok::Number(_))
            | Some(Tok::Var(..))
            | Some(Tok::Func(..)) => {
                let prim = self.primary()?;
                let mut preds = Vec::new();
                while matches!(self.peek(), Some(Tok::LBracket)) {
                    preds.push(self.predicate()?);
                }
                let filter = if preds.is_empty() {
                    prim
                } else {
                    Expr::Filter(Box::new(prim), preds)
                };
                match self.peek() {
                    Some(Tok::Slash) => {
                        self.pos += 1;
                        let mut steps = Vec::new();
                        self.rel_path(&mut steps)?;
                        Ok(Expr::Path {
                            start: PathStart::Expr(Box::new(filter)),
                            steps,
                        })
                    }
                    Some(Tok::SlashSlash) => {
                        self.pos += 1;
                        let mut steps = vec![Step {
                            axis: Axis::DescendantOrSelf,
                            test: NodeTest::Node,
                            preds: vec![],
                        }];
                        self.rel_path(&mut steps)?;
                        Ok(Expr::Path {
                            start: PathStart::Expr(Box::new(filter)),
                            steps,
                        })
                    }
                    _ => Ok(filter),
                }
            }
            _ => {
                if !self.starts_step() {
                    return Err(XErr::InvalidExpr);
                }
                let mut steps = Vec::new();
                self.rel_path(&mut steps)?;
                Ok(Expr::Path {
                    start: PathStart::Context,
                    steps,
                })
            }
        }
    }
    fn rel_path(&mut self, steps: &mut Vec<Step>) -> Result<(), XErr> {
        steps.push(self.step()?);
        loop {
            match self.peek() {
                Some(Tok::Slash) => {
                    self.pos += 1;
                    steps.push(self.step()?);
                }
                Some(Tok::SlashSlash) => {
                    self.pos += 1;
                    steps.push(Step {
                        axis: Axis::DescendantOrSelf,
                        test: NodeTest::Node,
                        preds: vec![],
                    });
                    steps.push(self.step()?);
                }
                _ => return Ok(()),
            }
        }
    }
    fn step(&mut self) -> Result<Step, XErr> {
        match self.peek() {
            Some(Tok::Dot) => {
                self.pos += 1;
                return Ok(Step {
                    axis: Axis::SelfAxis,
                    test: NodeTest::Node,
                    preds: vec![],
                });
            }
            Some(Tok::DotDot) => {
                self.pos += 1;
                return Ok(Step {
                    axis: Axis::Parent,
                    test: NodeTest::Node,
                    preds: vec![],
                });
            }
            _ => {}
        }
        let mut axis = Axis::Child;
        match self.peek().cloned() {
            Some(Tok::At) => {
                self.pos += 1;
                axis = Axis::Attribute;
            }
            Some(Tok::Axis(name)) => {
                self.pos += 1;
                if !matches!(self.next(), Some(Tok::ColonColon)) {
                    return Err(XErr::InvalidExpr);
                }
                axis = match name.as_str() {
                    "ancestor" => Axis::Ancestor,
                    "ancestor-or-self" => Axis::AncestorOrSelf,
                    "attribute" => Axis::Attribute,
                    "child" => Axis::Child,
                    "descendant" => Axis::Descendant,
                    "descendant-or-self" => Axis::DescendantOrSelf,
                    "following" => Axis::Following,
                    "following-sibling" => Axis::FollowingSibling,
                    "namespace" => Axis::Namespace,
                    "parent" => Axis::Parent,
                    "preceding" => Axis::Preceding,
                    "preceding-sibling" => Axis::PrecedingSibling,
                    "self" => Axis::SelfAxis,
                    _ => return Err(XErr::InvalidExpr),
                };
            }
            _ => {}
        }
        let test = match self.next() {
            Some(Tok::NameTest(p, l)) => NodeTest::Name(p, l),
            Some(Tok::NodeType(t)) => {
                if !matches!(self.next(), Some(Tok::LParen)) {
                    return Err(XErr::InvalidExpr);
                }
                let nt = match t.as_str() {
                    "node" => NodeTest::Node,
                    "text" => NodeTest::Text,
                    "comment" => NodeTest::Comment,
                    _ => {
                        if let Some(Tok::Literal(l)) = self.peek().cloned() {
                            self.pos += 1;
                            NodeTest::Pi(Some(l))
                        } else {
                            NodeTest::Pi(None)
                        }
                    }
                };
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err(XErr::InvalidExpr);
                }
                nt
            }
            _ => return Err(XErr::InvalidExpr),
        };
        let mut preds = Vec::new();
        while matches!(self.peek(), Some(Tok::LBracket)) {
            preds.push(self.predicate()?);
        }
        Ok(Step { axis, test, preds })
    }
    fn predicate(&mut self) -> Result<Expr, XErr> {
        self.pos += 1; // [
        if matches!(self.peek(), Some(Tok::RBracket)) {
            return Err(XErr::InvalidExpr);
        }
        let e = self.expr()?;
        if !matches!(self.next(), Some(Tok::RBracket)) {
            return Err(XErr::InvalidPredicate);
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, XErr> {
        match self.next() {
            Some(Tok::LParen) => {
                let e = self.expr()?;
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err(XErr::InvalidExpr);
                }
                Ok(e)
            }
            Some(Tok::Literal(s)) => Ok(Expr::Literal(s)),
            Some(Tok::Number(n)) => Ok(Expr::Number(n)),
            Some(Tok::Var(p, l)) => Ok(Expr::Var(p, l)),
            Some(Tok::Func(p, l)) => {
                if !matches!(self.next(), Some(Tok::LParen)) {
                    return Err(XErr::InvalidExpr);
                }
                let mut args = Vec::new();
                if !matches!(self.peek(), Some(Tok::RParen)) {
                    loop {
                        args.push(self.expr()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            _ => return Err(XErr::InvalidExpr),
                        }
                    }
                } else {
                    self.pos += 1;
                }
                Ok(Expr::Func(p, l, args))
            }
            _ => Err(XErr::InvalidExpr),
        }
    }
}

fn compile(s: &str) -> Result<Expr, XErr> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err(XErr::InvalidExpr);
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(XErr::InvalidExpr);
    }
    Ok(e)
}

// ---------------------------------------------------------------------------
// Evaluation

/// Extension function hook: (namespace uri, local name, args) -> result.
/// Return None for "unknown function".
pub type ExtFn<'a> =
    dyn FnMut(Option<&str>, &str, Vec<XValue>) -> Option<Result<XValue, XErr>> + 'a;

pub struct Ctx<'a> {
    pub ns: &'a [(String, String)],
    pub vars: &'a [(String, XValue)],
    pub ext: Option<&'a mut ExtFn<'a>>,
    /// Document root (for absolute paths when the node is detached).
    pub doc: Option<NodeId>,
}

struct Focus {
    node: NodeId,
    pos: usize,
    size: usize,
}

fn tw<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    tree::with(f)
}

/// In-scope namespaces of a node (xmlGetNsList order), for SimpleXML.
pub fn ns_list(t: &Tree, node: NodeId) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut cur = Some(node);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            for &ns in &t.nodes[c].ns_def {
                let p = t.nss[ns].prefix.clone().unwrap_or_default();
                if p.is_empty() {
                    continue;
                }
                if !out.iter().any(|(x, _)| *x == p) {
                    out.push((p, t.nss[ns].href.clone()));
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    out
}

pub fn eval(
    expr: &str,
    node: NodeId,
    ns: &[(String, String)],
    vars: &[(String, XValue)],
) -> Result<XValue, XErr> {
    let mut ctx = Ctx {
        ns,
        vars,
        ext: None,
        doc: None,
    };
    eval_ctx(expr, node, &mut ctx)
}

pub fn eval_ctx(expr: &str, node: NodeId, ctx: &mut Ctx) -> Result<XValue, XErr> {
    let e = compile(expr)?;
    let f = Focus {
        node,
        pos: 1,
        size: 1,
    };
    let v = eval_expr(&e, &f, ctx)?;
    Ok(match v {
        XValue::NodeSet(mut s) => {
            sort_dedup(&mut s);
            XValue::NodeSet(s)
        }
        o => o,
    })
}

/// Compile-only check (for error reporting before evaluation).
pub fn check(expr: &str) -> Result<(), XErr> {
    compile(expr).map(|_| ())
}

fn resolve_prefix(ctx: &Ctx, p: &str) -> Result<String, XErr> {
    if p == "xml" {
        return Ok(tree::XML_XML_NAMESPACE.to_string());
    }
    for (pp, u) in ctx.ns.iter().rev() {
        if pp == p {
            return Ok(u.clone());
        }
    }
    Err(XErr::UndefPrefix)
}

fn eval_expr(e: &Expr, f: &Focus, ctx: &mut Ctx) -> Result<XValue, XErr> {
    match e {
        Expr::Literal(s) => Ok(XValue::Str(s.clone())),
        Expr::Number(n) => Ok(XValue::Num(*n)),
        Expr::Var(p, l) => {
            let name = match p {
                Some(p) => format!("{}:{}", p, l),
                None => l.clone(),
            };
            for (n, v) in ctx.vars.iter() {
                if *n == name {
                    return Ok(v.clone());
                }
            }
            Err(XErr::UndefVariable)
        }
        Expr::Or(a, b) => {
            let x = to_bool(&eval_expr(a, f, ctx)?);
            if x {
                return Ok(XValue::Bool(true));
            }
            Ok(XValue::Bool(to_bool(&eval_expr(b, f, ctx)?)))
        }
        Expr::And(a, b) => {
            let x = to_bool(&eval_expr(a, f, ctx)?);
            if !x {
                return Ok(XValue::Bool(false));
            }
            Ok(XValue::Bool(to_bool(&eval_expr(b, f, ctx)?)))
        }
        Expr::Cmp(op, a, b) => {
            let x = eval_expr(a, f, ctx)?;
            let y = eval_expr(b, f, ctx)?;
            Ok(XValue::Bool(compare(op, &x, &y)))
        }
        Expr::Arith(op, a, b) => {
            let x = to_num(&eval_expr(a, f, ctx)?);
            let y = to_num(&eval_expr(b, f, ctx)?);
            Ok(XValue::Num(match *op {
                "+" => x + y,
                "-" => x - y,
                "*" => x * y,
                "div" => x / y,
                _ => {
                    if y == 0.0 {
                        f64::NAN
                    } else {
                        x % y
                    }
                }
            }))
        }
        Expr::Neg(a) => Ok(XValue::Num(-to_num(&eval_expr(a, f, ctx)?))),
        Expr::Union(a, b) => {
            let x = eval_expr(a, f, ctx)?;
            let y = eval_expr(b, f, ctx)?;
            match (x, y) {
                (XValue::NodeSet(mut s1), XValue::NodeSet(s2)) => {
                    s1.extend(s2);
                    sort_dedup(&mut s1);
                    Ok(XValue::NodeSet(s1))
                }
                _ => Err(XErr::InvalidType),
            }
        }
        Expr::Filter(prim, preds) => {
            let v = eval_expr(prim, f, ctx)?;
            let mut set = match v {
                XValue::NodeSet(s) => s,
                _ => return Err(XErr::InvalidType),
            };
            sort_dedup(&mut set);
            for p in preds {
                set = apply_predicate(&set, p, ctx, false)?;
            }
            Ok(XValue::NodeSet(set))
        }
        Expr::Path { start, steps } => {
            let mut set: Vec<NodeId> = match start {
                PathStart::Context => vec![f.node],
                PathStart::Root => {
                    let root = tw(|t| {
                        let mut cur = f.node;
                        while let Some(p) = t.nodes[cur].parent {
                            cur = p;
                        }
                        cur
                    });
                    vec![root]
                }
                PathStart::Expr(e) => match eval_expr(e, f, ctx)? {
                    XValue::NodeSet(mut s) => {
                        sort_dedup(&mut s);
                        s
                    }
                    _ => return Err(XErr::InvalidType),
                },
            };
            for st in steps {
                set = eval_step(&set, st, ctx)?;
            }
            Ok(XValue::NodeSet(set))
        }
        Expr::Func(p, l, args) => call_function(p.as_deref(), l, args, f, ctx),
    }
}

fn apply_predicate(
    set: &[NodeId],
    p: &Expr,
    ctx: &mut Ctx,
    reverse: bool,
) -> Result<Vec<NodeId>, XErr> {
    let size = set.len();
    let mut out = Vec::new();
    for (i, &n) in set.iter().enumerate() {
        let pos = if reverse { size - i } else { i + 1 };
        let fc = Focus { node: n, pos, size };
        let v = eval_expr(p, &fc, ctx)?;
        let keep = match v {
            XValue::Num(x) => x == pos as f64,
            other => to_bool(&other),
        };
        if keep {
            out.push(n);
        }
    }
    Ok(out)
}

fn eval_step(input: &[NodeId], st: &Step, ctx: &mut Ctx) -> Result<Vec<NodeId>, XErr> {
    // resolve the name test prefix once
    let test_ns: Option<String> = match &st.test {
        NodeTest::Name(Some(p), _) => Some(resolve_prefix(ctx, p)?),
        _ => None,
    };
    let reverse = matches!(
        st.axis,
        Axis::Ancestor | Axis::AncestorOrSelf | Axis::Preceding | Axis::PrecedingSibling
    );
    let mut out: Vec<NodeId> = Vec::new();
    for &n in input {
        let mut cand = axis_nodes(n, st.axis);
        cand.retain(|&c| tw(|t| node_test(t, c, &st.test, test_ns.as_deref(), st.axis)));
        // predicates see proximity positions: reverse axes count from the
        // nearest node
        if reverse {
            cand.reverse();
        }
        for p in &st.preds {
            cand = apply_predicate(&cand, p, ctx, false)?;
        }
        out.extend(cand);
    }
    let _ = reverse;
    sort_dedup(&mut out);
    Ok(out)
}

fn node_test(t: &Tree, c: NodeId, test: &NodeTest, ns: Option<&str>, axis: Axis) -> bool {
    let n = &t.nodes[c];
    match test {
        NodeTest::Node => true,
        NodeTest::Text => matches!(n.t, NType::Text | NType::CData),
        NodeTest::Comment => n.t == NType::Comment,
        NodeTest::Pi(lit) => n.t == NType::Pi && lit.as_ref().map(|l| *l == n.name).unwrap_or(true),
        NodeTest::Name(prefix, local) => {
            let principal = match axis {
                Axis::Attribute => NType::Attribute,
                Axis::Namespace => NType::NamespaceDecl,
                _ => NType::Element,
            };
            if n.t != principal {
                return false;
            }
            if principal == NType::NamespaceDecl {
                return local == "*" || n.name == *local;
            }
            if local != "*" && n.name != *local {
                return false;
            }
            let node_ns = t.ns_href(n.ns);
            match prefix {
                None => {
                    if local == "*" {
                        true
                    } else {
                        node_ns.is_none()
                    }
                }
                Some(_) => node_ns == ns,
            }
        }
    }
}

fn axis_nodes(n: NodeId, axis: Axis) -> Vec<NodeId> {
    tw(|t| {
        let nt = t.nodes[n].t;
        let mut v = Vec::new();
        match axis {
            Axis::SelfAxis => v.push(n),
            Axis::Child => {
                if !matches!(
                    nt,
                    NType::Attribute | NType::NamespaceDecl | NType::EntityRef
                ) {
                    let mut c = t.nodes[n].children;
                    while let Some(x) = c {
                        if !matches!(t.nodes[x].t, NType::Dtd | NType::EntityDecl) {
                            v.push(x);
                        }
                        c = t.nodes[x].next;
                    }
                }
            }
            Axis::Descendant | Axis::DescendantOrSelf => {
                if axis == Axis::DescendantOrSelf {
                    v.push(n);
                }
                if !matches!(nt, NType::Attribute | NType::NamespaceDecl) {
                    for d in t.descendants(n) {
                        if !matches!(t.nodes[d].t, NType::Dtd | NType::EntityDecl) {
                            v.push(d);
                        }
                    }
                }
            }
            Axis::Parent => {
                if nt == NType::NamespaceDecl {
                    if let Some(p) = t.nodes[n].parent {
                        v.push(p);
                    }
                } else if let Some(p) = t.nodes[n].parent {
                    if t.nodes[p].t != NType::EntityDecl {
                        v.push(p);
                    }
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if axis == Axis::AncestorOrSelf {
                    v.push(n);
                }
                let mut cur = t.nodes[n].parent;
                while let Some(p) = cur {
                    v.push(p);
                    cur = t.nodes[p].parent;
                }
                v.reverse();
            }
            Axis::Attribute => {
                if nt == NType::Element {
                    let mut a = t.nodes[n].properties;
                    while let Some(x) = a {
                        // xmlns attributes are not attributes in XPath
                        v.push(x);
                        a = t.nodes[x].next;
                    }
                }
            }
            Axis::Namespace => {
                if nt == NType::Element {
                    // in-scope namespaces, nearest first; xml namespace first
                    let mut seen: Vec<Option<String>> = Vec::new();
                    let mut list: Vec<tree::NsId> = Vec::new();
                    let mut cur = Some(n);
                    while let Some(c) = cur {
                        if t.nodes[c].t == NType::Element {
                            for &ns in &t.nodes[c].ns_def {
                                let p = t.nss[ns].prefix.clone();
                                if !seen.contains(&p) {
                                    seen.push(p);
                                    list.push(ns);
                                }
                            }
                        }
                        cur = t.nodes[c].parent;
                    }
                    let _ = list;
                }
            }
            Axis::FollowingSibling => {
                if !matches!(nt, NType::Attribute | NType::NamespaceDecl) {
                    let mut c = t.nodes[n].next;
                    while let Some(x) = c {
                        v.push(x);
                        c = t.nodes[x].next;
                    }
                }
            }
            Axis::PrecedingSibling => {
                if !matches!(nt, NType::Attribute | NType::NamespaceDecl) {
                    let mut c = t.nodes[n].prev;
                    while let Some(x) = c {
                        v.push(x);
                        c = t.nodes[x].prev;
                    }
                    v.reverse();
                }
            }
            Axis::Following => {
                let mut cur = n;
                if matches!(nt, NType::Attribute | NType::NamespaceDecl) {
                    if let Some(p) = t.nodes[n].parent {
                        cur = p;
                        for d in t.descendants(p) {
                            v.push(d);
                        }
                    }
                }
                loop {
                    let mut s = t.nodes[cur].next;
                    while let Some(x) = s {
                        v.push(x);
                        for d in t.descendants(x) {
                            v.push(d);
                        }
                        s = t.nodes[x].next;
                    }
                    match t.nodes[cur].parent {
                        Some(p) => cur = p,
                        None => break,
                    }
                }
            }
            Axis::Preceding => {
                let mut cur = n;
                if matches!(nt, NType::Attribute | NType::NamespaceDecl) {
                    if let Some(p) = t.nodes[n].parent {
                        cur = p;
                    }
                }
                let mut ancestors = Vec::new();
                let mut a = t.nodes[cur].parent;
                while let Some(p) = a {
                    ancestors.push(p);
                    a = t.nodes[p].parent;
                }
                loop {
                    let mut s = t.nodes[cur].prev;
                    while let Some(x) = s {
                        let mut sub = vec![x];
                        sub.extend(t.descendants(x));
                        for d in sub.into_iter().rev() {
                            v.push(d);
                        }
                        s = t.nodes[x].prev;
                    }
                    match t.nodes[cur].parent {
                        Some(p) => cur = p,
                        None => break,
                    }
                }
                v.reverse();
                let _ = ancestors;
            }
        }
        if axis == Axis::Namespace && nt == NType::Element {
            v = namespace_nodes(t, n);
        }
        v
    })
}

/// Namespace nodes of an element (pseudo nodes cached in the arena).
pub fn namespace_nodes(t: &mut Tree, n: NodeId) -> Vec<NodeId> {
    let mut seen: Vec<Option<String>> = Vec::new();
    let mut list: Vec<tree::NsId> = Vec::new();
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            for &ns in &t.nodes[c].ns_def {
                let p = t.nss[ns].prefix.clone();
                if !seen.contains(&p) {
                    seen.push(p);
                    list.push(ns);
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    let mut out = Vec::new();
    let doc = t.nodes[n].doc;
    let xml = t.xml_ns(doc);
    out.push(ns_node(t, n, xml));
    // xmlXPathNextNamespace walks xmlGetNsList() from the end
    for ns in list.into_iter().rev() {
        // a default namespace undeclared with xmlns="" is not in scope
        if t.nss[ns].prefix.is_none() && t.nss[ns].href.is_empty() {
            continue;
        }
        out.push(ns_node(t, n, ns));
    }
    out
}

thread_local! {
    static NS_NODES: std::cell::RefCell<HashMap<(NodeId, tree::NsId), NodeId>> = std::cell::RefCell::new(HashMap::new());
}

pub fn ns_node(t: &mut Tree, elem: NodeId, ns: tree::NsId) -> NodeId {
    if let Some(id) = NS_NODES.with(|m| m.borrow().get(&(elem, ns)).copied()) {
        return id;
    }
    let doc = t.nodes[elem].doc;
    let name = t.nss[ns].prefix.clone().unwrap_or_default();
    let id = t.new_node(doc, NType::NamespaceDecl, &name);
    t.nodes[id].content = t.nss[ns].href.clone();
    t.nodes[id].ns = Some(ns);
    t.nodes[id].parent = Some(elem);
    NS_NODES.with(|m| m.borrow_mut().insert((elem, ns), id));
    id
}

// ---------------------------------------------------------------------------
// Ordering

/// Key for document order: path of (rank, index) from the root.
fn order_key(t: &Tree, n: NodeId) -> Vec<(u8, usize)> {
    let mut key = Vec::new();
    let mut cur = n;
    loop {
        let nd = &t.nodes[cur];
        let parent = match nd.parent {
            Some(p) => p,
            None => {
                key.push((0, cur));
                break;
            }
        };
        let (rank, idx) = match nd.t {
            NType::NamespaceDecl => (0u8, cur),
            NType::Attribute => {
                let mut i = 0;
                let mut p = nd.prev;
                while let Some(x) = p {
                    i += 1;
                    p = t.nodes[x].prev;
                }
                (1u8, i)
            }
            _ => {
                let mut i = 0;
                let mut p = nd.prev;
                while let Some(x) = p {
                    i += 1;
                    p = t.nodes[x].prev;
                }
                (2u8, i)
            }
        };
        key.push((rank, idx));
        cur = parent;
    }
    key.reverse();
    key
}

pub fn sort_dedup(v: &mut Vec<NodeId>) {
    if v.len() <= 1 {
        return;
    }
    // quick check: already strictly increasing by id and unique is not
    // sufficient (ids are allocation order), so compute keys
    let keys: Vec<(Vec<(u8, usize)>, NodeId)> =
        tw(|t| v.iter().map(|&n| (order_key(t, n), n)).collect());
    let mut keys = keys;
    keys.sort_by(|a, b| match a.0.cmp(&b.0) {
        Ordering::Equal => a.1.cmp(&b.1),
        o => o,
    });
    keys.dedup_by(|a, b| a.1 == b.1);
    *v = keys.into_iter().map(|(_, n)| n).collect();
}

// ---------------------------------------------------------------------------
// Conversions

pub fn string_value(t: &Tree, n: NodeId) -> String {
    let nd = &t.nodes[n];
    match nd.t {
        NType::NamespaceDecl => nd.content.clone(),
        NType::Text | NType::CData | NType::Comment | NType::Pi => nd.content.clone(),
        NType::Attribute => t.node_get_content(n),
        _ => t.node_get_content(n),
    }
}

pub fn to_bool(v: &XValue) -> bool {
    match v {
        XValue::Bool(b) => *b,
        XValue::Num(n) => *n != 0.0 && !n.is_nan(),
        XValue::Str(s) => !s.is_empty(),
        XValue::NodeSet(s) => !s.is_empty(),
    }
}

pub fn str_to_num(s: &str) -> f64 {
    // xmlXPathStringEvalNumber()
    let blank = |c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r';
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() && blank(chars[i]) {
        i += 1;
    }
    let neg = i < chars.len() && chars[i] == '-';
    if neg {
        i += 1;
    }
    let Some((v, mut end)) = scan_number(&chars, i) else {
        return f64::NAN;
    };
    while end < chars.len() && blank(chars[end]) {
        end += 1;
    }
    if end != chars.len() {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// libxml2's number scanner (xmlXPathCompNumber / xmlXPathStringEvalNumber):
/// digits, an optional fraction (at most MAX_FRAC significant digits) and an
/// optional exponent; None when there is no digit before or after the '.'.
fn scan_number(c: &[char], mut i: usize) -> Option<(f64, usize)> {
    const MAX_FRAC: i32 = 20;
    let digit = |i: usize| c.get(i).and_then(|ch| ch.to_digit(10));
    if c.get(i) != Some(&'.') && digit(i).is_none() {
        return None;
    }
    let mut ret = 0.0f64;
    let mut ok = false;
    while let Some(d) = digit(i) {
        ret = ret * 10.0 + d as f64;
        ok = true;
        i += 1;
    }
    if c.get(i) == Some(&'.') {
        i += 1;
        if digit(i).is_none() && !ok {
            return None;
        }
        let mut frac = 0i32;
        let mut fraction = 0.0f64;
        while c.get(i) == Some(&'0') {
            frac += 1;
            i += 1;
        }
        let max = frac + MAX_FRAC;
        while let Some(d) = digit(i) {
            if frac >= max {
                break;
            }
            fraction = fraction * 10.0 + d as f64;
            frac += 1;
            i += 1;
        }
        fraction /= 10f64.powf(frac as f64);
        ret += fraction;
        while digit(i).is_some() {
            i += 1;
        }
    }
    if matches!(c.get(i), Some('e') | Some('E')) {
        i += 1;
        let mut neg = false;
        match c.get(i) {
            Some('-') => {
                neg = true;
                i += 1;
            }
            Some('+') => i += 1,
            _ => {}
        }
        let mut exponent = 0i32;
        while let Some(d) = digit(i) {
            if exponent < 1_000_000 {
                exponent = exponent * 10 + d as i32;
            }
            i += 1;
        }
        if neg {
            exponent = -exponent;
        }
        ret *= 10f64.powf(exponent as f64);
    }
    Some((ret, i))
}

pub fn to_num(v: &XValue) -> f64 {
    match v {
        XValue::Num(n) => *n,
        XValue::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        XValue::Str(s) => str_to_num(s),
        XValue::NodeSet(_) => str_to_num(&to_str(v)),
    }
}

pub fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if n == 0.0 {
        return "0".into();
    }
    if n > i32::MIN as f64 && n < i32::MAX as f64 && n == (n as i32) as f64 {
        return format!("{}", n as i32);
    }
    let abs = n.abs();
    let mut work = if abs > 1e9 || abs < 1e-5 {
        // %*.*e with 14 fraction digits
        let s = format!("{:.14e}", n);
        // Rust prints e.g. 1.23400000000000e10 -> convert exponent to C style e+10
        let (mant, exp) = s.split_once('e').unwrap();
        let expn: i32 = exp.parse().unwrap_or(0);
        let mant = mant.trim_end_matches('0');
        let mant = mant.trim_end_matches('.');
        return format!(
            "{}e{}{:02}",
            mant,
            if expn < 0 { '-' } else { '+' },
            expn.abs()
        );
    } else {
        let integer_place = abs.log10() as i32;
        let fraction_place = if integer_place > 0 {
            15 - integer_place - 1
        } else {
            15 - integer_place
        };
        format!("{:.*}", fraction_place.max(0) as usize, n)
    };
    if work.contains('.') {
        while work.ends_with('0') {
            work.pop();
        }
        if work.ends_with('.') {
            work.pop();
        }
    }
    work
}

pub fn to_str(v: &XValue) -> String {
    match v {
        XValue::Str(s) => s.clone(),
        XValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        XValue::Num(n) => num_to_string(*n),
        XValue::NodeSet(s) => {
            if s.is_empty() {
                String::new()
            } else {
                let mut s2 = s.clone();
                sort_dedup(&mut s2);
                tw(|t| string_value(t, s2[0]))
            }
        }
    }
}

fn compare(op: &str, x: &XValue, y: &XValue) -> bool {
    match (x, y) {
        (XValue::NodeSet(a), XValue::NodeSet(b)) => {
            let sa: Vec<String> = tw(|t| a.iter().map(|&n| string_value(t, n)).collect());
            let sb: Vec<String> = tw(|t| b.iter().map(|&n| string_value(t, n)).collect());
            for p in &sa {
                for q in &sb {
                    if cmp_atomic(op, &XValue::Str(p.clone()), &XValue::Str(q.clone())) {
                        return true;
                    }
                }
            }
            false
        }
        (XValue::NodeSet(a), other) | (other, XValue::NodeSet(a)) => {
            let swapped = matches!(y, XValue::NodeSet(_)) && !matches!(x, XValue::NodeSet(_));
            if let XValue::Bool(b) = other {
                let nb = !a.is_empty();
                let (l, r) = if swapped {
                    (XValue::Bool(*b), XValue::Bool(nb))
                } else {
                    (XValue::Bool(nb), XValue::Bool(*b))
                };
                return cmp_atomic(op, &l, &r);
            }
            let vals: Vec<String> = tw(|t| a.iter().map(|&n| string_value(t, n)).collect());
            for s in vals {
                let nv = match other {
                    XValue::Num(_) => XValue::Num(str_to_num(&s)),
                    _ => XValue::Str(s),
                };
                let res = if swapped {
                    cmp_atomic(op, other, &nv)
                } else {
                    cmp_atomic(op, &nv, other)
                };
                if res {
                    return true;
                }
            }
            false
        }
        _ => cmp_atomic(op, x, y),
    }
}

fn cmp_atomic(op: &str, x: &XValue, y: &XValue) -> bool {
    match op {
        "=" | "!=" => {
            let eq = if matches!(x, XValue::Bool(_)) || matches!(y, XValue::Bool(_)) {
                to_bool(x) == to_bool(y)
            } else if matches!(x, XValue::Num(_)) || matches!(y, XValue::Num(_)) {
                to_num(x) == to_num(y)
            } else {
                to_str(x) == to_str(y)
            };
            if op == "=" { eq } else { !eq }
        }
        _ => {
            let a = to_num(x);
            let b = to_num(y);
            match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Core function library

fn arg_count(args: &[Expr], n: usize) -> Result<(), XErr> {
    if args.len() != n {
        Err(XErr::InvalidArity)
    } else {
        Ok(())
    }
}

fn call_function(
    prefix: Option<&str>,
    name: &str,
    args: &[Expr],
    f: &Focus,
    ctx: &mut Ctx,
) -> Result<XValue, XErr> {
    if let Some(p) = prefix {
        let uri = resolve_prefix(ctx, p)?;
        let mut vals = Vec::new();
        for a in args {
            vals.push(eval_expr(a, f, ctx)?);
        }
        if let Some(ext) = ctx.ext.as_mut() {
            if let Some(r) = ext(Some(&uri), name, vals) {
                return r;
            }
        }
        return Err(XErr::UnknownFunc);
    }
    let ctx_str = |n: NodeId| tw(|t| string_value(t, n));
    match name {
        "last" => {
            arg_count(args, 0)?;
            Ok(XValue::Num(f.size as f64))
        }
        "position" => {
            arg_count(args, 0)?;
            Ok(XValue::Num(f.pos as f64))
        }
        "count" => {
            arg_count(args, 1)?;
            match eval_expr(&args[0], f, ctx)? {
                XValue::NodeSet(s) => Ok(XValue::Num(s.len() as f64)),
                _ => Err(XErr::InvalidType),
            }
        }
        "id" => {
            arg_count(args, 1)?;
            let v = eval_expr(&args[0], f, ctx)?;
            let ids: Vec<String> = match &v {
                XValue::NodeSet(s) => {
                    let mut all = Vec::new();
                    for &n in s {
                        for tok in ctx_str(n).split_whitespace() {
                            all.push(tok.to_string());
                        }
                    }
                    all
                }
                other => to_str(other)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect(),
            };
            let mut out = Vec::new();
            tw(|t| {
                if let Some(d) = t.nodes[f.node].doc {
                    if let Some(info) = t.doc_info(d) {
                        for id in &ids {
                            if let Some(&a) = info.ids.get(id) {
                                if let Some(p) = t.nodes[a].parent {
                                    out.push(p);
                                }
                            }
                        }
                    }
                }
            });
            sort_dedup(&mut out);
            Ok(XValue::NodeSet(out))
        }
        "local-name" | "namespace-uri" | "name" => {
            let node = if args.is_empty() {
                Some(f.node)
            } else {
                arg_count(args, 1)?;
                match eval_expr(&args[0], f, ctx)? {
                    XValue::NodeSet(mut s) => {
                        sort_dedup(&mut s);
                        s.first().copied()
                    }
                    _ => return Err(XErr::InvalidType),
                }
            };
            let r = match node {
                None => String::new(),
                Some(n) => tw(|t| {
                    let nd = &t.nodes[n];
                    match name {
                        "local-name" => match nd.t {
                            NType::Element
                            | NType::Attribute
                            | NType::Pi
                            | NType::NamespaceDecl => nd.name.clone(),
                            _ => String::new(),
                        },
                        "namespace-uri" => match nd.t {
                            NType::Element | NType::Attribute => {
                                t.ns_href(nd.ns).unwrap_or("").to_string()
                            }
                            _ => String::new(),
                        },
                        _ => match nd.t {
                            NType::Element | NType::Attribute => t.qname(n),
                            NType::Pi | NType::NamespaceDecl => nd.name.clone(),
                            _ => String::new(),
                        },
                    }
                }),
            };
            Ok(XValue::Str(r))
        }
        "string" => {
            if args.is_empty() {
                return Ok(XValue::Str(ctx_str(f.node)));
            }
            arg_count(args, 1)?;
            Ok(XValue::Str(to_str(&eval_expr(&args[0], f, ctx)?)))
        }
        "concat" => {
            if args.len() < 2 {
                return Err(XErr::InvalidArity);
            }
            let mut s = String::new();
            for a in args {
                s.push_str(&to_str(&eval_expr(a, f, ctx)?));
            }
            Ok(XValue::Str(s))
        }
        "starts-with" | "contains" | "substring-before" | "substring-after" => {
            arg_count(args, 2)?;
            let a = to_str(&eval_expr(&args[0], f, ctx)?);
            let b = to_str(&eval_expr(&args[1], f, ctx)?);
            Ok(match name {
                "starts-with" => XValue::Bool(a.starts_with(&b)),
                "contains" => XValue::Bool(a.contains(&b)),
                "substring-before" => {
                    XValue::Str(a.find(&b).map(|i| a[..i].to_string()).unwrap_or_default())
                }
                _ => XValue::Str(
                    a.find(&b)
                        .map(|i| a[i + b.len()..].to_string())
                        .unwrap_or_default(),
                ),
            })
        }
        "substring" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(XErr::InvalidArity);
            }
            let s = to_str(&eval_expr(&args[0], f, ctx)?);
            let start = to_num(&eval_expr(&args[1], f, ctx)?);
            let len = if args.len() == 3 {
                Some(to_num(&eval_expr(&args[2], f, ctx)?))
            } else {
                None
            };
            let chars: Vec<char> = s.chars().collect();
            let rs = round_half(start);
            let mut out = String::new();
            for (i, c) in chars.iter().enumerate() {
                let p = (i + 1) as f64;
                let ok = match len {
                    None => p >= rs,
                    Some(l) => p >= rs && p < rs + round_half(l),
                };
                if ok {
                    out.push(*c);
                }
            }
            Ok(XValue::Str(out))
        }
        "string-length" => {
            let s = if args.is_empty() {
                ctx_str(f.node)
            } else {
                arg_count(args, 1)?;
                to_str(&eval_expr(&args[0], f, ctx)?)
            };
            Ok(XValue::Num(s.chars().count() as f64))
        }
        "normalize-space" => {
            let s = if args.is_empty() {
                ctx_str(f.node)
            } else {
                arg_count(args, 1)?;
                to_str(&eval_expr(&args[0], f, ctx)?)
            };
            Ok(XValue::Str(
                s.split([' ', '\t', '\n', '\r'])
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
        }
        "translate" => {
            arg_count(args, 3)?;
            let s = to_str(&eval_expr(&args[0], f, ctx)?);
            let from: Vec<char> = to_str(&eval_expr(&args[1], f, ctx)?).chars().collect();
            let to: Vec<char> = to_str(&eval_expr(&args[2], f, ctx)?).chars().collect();
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
            Ok(XValue::Str(out))
        }
        "boolean" => {
            arg_count(args, 1)?;
            Ok(XValue::Bool(to_bool(&eval_expr(&args[0], f, ctx)?)))
        }
        "not" => {
            arg_count(args, 1)?;
            Ok(XValue::Bool(!to_bool(&eval_expr(&args[0], f, ctx)?)))
        }
        "true" => {
            arg_count(args, 0)?;
            Ok(XValue::Bool(true))
        }
        "false" => {
            arg_count(args, 0)?;
            Ok(XValue::Bool(false))
        }
        "lang" => {
            arg_count(args, 1)?;
            let want = to_str(&eval_expr(&args[0], f, ctx)?).to_ascii_lowercase();
            let lang = tw(|t| {
                let mut cur = Some(f.node);
                while let Some(c) = cur {
                    if t.nodes[c].t == NType::Element {
                        for a in t.attrs_of(c) {
                            if t.nodes[a].name == "lang"
                                && t.ns_href(t.nodes[a].ns) == Some(tree::XML_XML_NAMESPACE)
                            {
                                return Some(t.node_get_content(a));
                            }
                        }
                    }
                    cur = t.nodes[c].parent;
                }
                None
            });
            Ok(XValue::Bool(match lang {
                None => false,
                Some(l) => {
                    let l = l.to_ascii_lowercase();
                    l == want
                        || (l.starts_with(&want) && l.as_bytes().get(want.len()) == Some(&b'-'))
                }
            }))
        }
        "number" => {
            if args.is_empty() {
                return Ok(XValue::Num(str_to_num(&ctx_str(f.node))));
            }
            arg_count(args, 1)?;
            Ok(XValue::Num(to_num(&eval_expr(&args[0], f, ctx)?)))
        }
        "sum" => {
            arg_count(args, 1)?;
            match eval_expr(&args[0], f, ctx)? {
                XValue::NodeSet(s) => {
                    let mut total = 0.0;
                    for n in s {
                        total += str_to_num(&ctx_str(n));
                    }
                    Ok(XValue::Num(total))
                }
                _ => Err(XErr::InvalidType),
            }
        }
        "floor" => {
            arg_count(args, 1)?;
            Ok(XValue::Num(to_num(&eval_expr(&args[0], f, ctx)?).floor()))
        }
        "ceiling" => {
            arg_count(args, 1)?;
            Ok(XValue::Num(to_num(&eval_expr(&args[0], f, ctx)?).ceil()))
        }
        "round" => {
            arg_count(args, 1)?;
            let x = to_num(&eval_expr(&args[0], f, ctx)?);
            Ok(XValue::Num(round_half(x)))
        }
        "escape-uri" | "escape" => Err(XErr::UnknownFunc),
        _ => {
            let mut vals = Vec::new();
            for a in args {
                vals.push(eval_expr(a, f, ctx)?);
            }
            if let Some(ext) = ctx.ext.as_mut() {
                if let Some(r) = ext(None, name, vals) {
                    return r;
                }
            }
            Err(XErr::UnknownFunc)
        }
    }
}

fn round_half(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    (x + 0.5).floor()
}
