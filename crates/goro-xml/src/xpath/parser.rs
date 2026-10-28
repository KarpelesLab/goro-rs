//! XPath 1.0 expression lexer and parser (W3C XPath 1.0, sections 2, 3
//! and the lexical structure of 3.7).
//!
//! Error kinds and messages follow what PHP reports for its XPath engine
//! (observed on PHP 8.5, see [`super::XPathErrorKind`]).

use super::XPathError;
use super::XPathErrorKind as K;

/// Axes (XPath 1.0 section 2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
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

impl Axis {
    fn from_name(n: &str) -> Option<Axis> {
        Some(match n {
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
            _ => return None,
        })
    }

    /// Reverse axes count proximity positions backwards.
    pub fn is_reverse(self) -> bool {
        matches!(
            self,
            Axis::Ancestor | Axis::AncestorOrSelf | Axis::Preceding | Axis::PrecedingSibling
        )
    }
}

/// Node tests.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeTest {
    /// `node()`
    Node,
    /// `text()`
    Text,
    /// `comment()`
    Comment,
    /// `processing-instruction(literal?)`
    Pi(Option<String>),
    /// `*`
    Any,
    /// `prefix:*`
    NsAny(String),
    /// `prefix:local` or `local`
    Name(Option<String>, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

/// The core function library (XPath 1.0 section 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreFn {
    Last,
    Position,
    Count,
    Id,
    LocalName,
    NamespaceUri,
    Name,
    String,
    Concat,
    StartsWith,
    Contains,
    SubstringBefore,
    SubstringAfter,
    Substring,
    StringLength,
    NormalizeSpace,
    Translate,
    Boolean,
    Not,
    True,
    False,
    Lang,
    Number,
    Sum,
    Floor,
    Ceiling,
    Round,
}

impl CoreFn {
    fn from_name(n: &str) -> Option<CoreFn> {
        use CoreFn::*;
        Some(match n {
            "last" => Last,
            "position" => Position,
            "count" => Count,
            "id" => Id,
            "local-name" => LocalName,
            "namespace-uri" => NamespaceUri,
            "name" => Name,
            "string" => String,
            "concat" => Concat,
            "starts-with" => StartsWith,
            "contains" => Contains,
            "substring-before" => SubstringBefore,
            "substring-after" => SubstringAfter,
            "substring" => Substring,
            "string-length" => StringLength,
            "normalize-space" => NormalizeSpace,
            "translate" => Translate,
            "boolean" => Boolean,
            "not" => Not,
            "true" => True,
            "false" => False,
            "lang" => Lang,
            "number" => Number,
            "sum" => Sum,
            "floor" => Floor,
            "ceiling" => Ceiling,
            "round" => Round,
            _ => return None,
        })
    }

    /// Allowed argument counts (min, max).
    fn arity(self) -> (usize, usize) {
        use CoreFn::*;
        match self {
            Last | Position | True | False => (0, 0),
            Count | Id | Boolean | Not | Lang | Sum | Floor | Ceiling | Round => (1, 1),
            LocalName | NamespaceUri | Name | String | StringLength | NormalizeSpace | Number => {
                (0, 1)
            }
            Concat => (2, usize::MAX),
            StartsWith | Contains | SubstringBefore | SubstringAfter => (2, 2),
            Substring => (2, 3),
            Translate => (3, 3),
        }
    }
}

/// A compiled expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    Arith(ArOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Literal(String),
    Number(f64),
    Fn(CoreFn, Vec<Expr>),
    /// An extension function `prefix:local(args)`.
    ExtFn(String, String, Vec<Expr>),
    Path(Box<PathExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Start {
    /// `/...`: the root of the context node's tree.
    Root,
    /// A relative location path.
    Context,
    /// A filter expression with its predicates.
    Filter(Expr, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Step {
    pub axis: Axis,
    pub test: NodeTest,
    pub preds: Vec<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathExpr {
    pub start: Start,
    pub steps: Vec<Step>,
}

// ----- lexer -------------------------------------------------------------------

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
    Slash,
    DSlash,
    Pipe,
    Plus,
    Minus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `*` as a name test.
    Star,
    /// `*` as multiplication.
    Mul,
    And,
    Or,
    Mod,
    Div,
    Literal(String),
    Number(f64),
    Var(String),
    /// A function name (followed by `(`): prefix, local.
    FuncName(Option<String>, String),
    /// `node`, `text`, `comment`, `processing-instruction` followed by `(`.
    NodeType(String),
    /// An axis name (followed by `::`).
    AxisName(String),
    /// `prefix:*`
    NsStar(String),
    /// A QName name test.
    Name(Option<String>, String),
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '.' || c == '-' || c == '\u{B7}'
}

fn lex(s: &str) -> Result<Vec<Tok>, XPathError> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut out: Vec<Tok> = Vec::new();
    let expr_err = || XPathError::new(K::Expr);
    // XPath 1.0 section 3.7: is there a preceding token that makes `*` and
    // operator names operators?
    let operator_context = |out: &Vec<Tok>| -> bool {
        match out.last() {
            None => false,
            Some(t) => !matches!(
                t,
                Tok::At
                    | Tok::ColonColon
                    | Tok::LParen
                    | Tok::LBracket
                    | Tok::Comma
                    | Tok::Slash
                    | Tok::DSlash
                    | Tok::Pipe
                    | Tok::Plus
                    | Tok::Minus
                    | Tok::Eq
                    | Tok::Ne
                    | Tok::Lt
                    | Tok::Le
                    | Tok::Gt
                    | Tok::Ge
                    | Tok::Mul
                    | Tok::And
                    | Tok::Or
                    | Tok::Mod
                    | Tok::Div
            ),
        }
    };
    let skip_ws = |mut j: usize| {
        while j < n && matches!(chars[j], ' ' | '\t' | '\n' | '\r') {
            j += 1;
        }
        j
    };
    while i < n {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            '@' => {
                out.push(Tok::At);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '|' => {
                out.push(Tok::Pipe);
                i += 1;
            }
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '=' => {
                out.push(Tok::Eq);
                i += 1;
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Ne);
                    i += 2;
                } else {
                    return Err(expr_err());
                }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Le);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            '/' => {
                if chars.get(i + 1) == Some(&'/') {
                    out.push(Tok::DSlash);
                    i += 2;
                } else {
                    out.push(Tok::Slash);
                    i += 1;
                }
            }
            ':' => {
                if chars.get(i + 1) == Some(&':') {
                    out.push(Tok::ColonColon);
                    i += 2;
                } else {
                    return Err(expr_err());
                }
            }
            '*' => {
                if operator_context(&out) {
                    out.push(Tok::Mul);
                } else {
                    out.push(Tok::Star);
                }
                i += 1;
            }
            '"' | '\'' => {
                let q = c;
                let start = i + 1;
                let mut j = start;
                while j < n && chars[j] != q {
                    j += 1;
                }
                if j >= n {
                    return Err(XPathError::new(K::UnfinishedLiteral));
                }
                out.push(Tok::Literal(chars[start..j].iter().collect()));
                i = j + 1;
            }
            '.' => {
                if chars.get(i + 1) == Some(&'.') {
                    out.push(Tok::DotDot);
                    i += 2;
                } else if chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    let (v, j) = lex_number(&chars, i);
                    out.push(Tok::Number(v));
                    i = j;
                } else {
                    out.push(Tok::Dot);
                    i += 1;
                }
            }
            '0'..='9' => {
                let (v, j) = lex_number(&chars, i);
                out.push(Tok::Number(v));
                i = j;
            }
            '$' => {
                let mut j = i + 1;
                if j < n && is_name_start(chars[j]) {
                    while j < n && (is_name_char(chars[j]) || chars[j] == ':') {
                        j += 1;
                    }
                    let name: String = chars[i + 1..j].iter().collect();
                    out.push(Tok::Var(name));
                    i = j;
                } else {
                    return Err(XPathError::new(K::VariableRef));
                }
            }
            _ if is_name_start(c) => {
                let mut j = i;
                while j < n && is_name_char(chars[j]) {
                    j += 1;
                }
                let first: String = chars[i..j].iter().collect();
                // Operator names.
                if operator_context(&out) {
                    let t = match first.as_str() {
                        "and" => Some(Tok::And),
                        "or" => Some(Tok::Or),
                        "mod" => Some(Tok::Mod),
                        "div" => Some(Tok::Div),
                        _ => None,
                    };
                    match t {
                        Some(t) => {
                            out.push(t);
                            i = j;
                            continue;
                        }
                        None => return Err(expr_err()),
                    }
                }
                // QName / prefix:*
                let mut prefix = None;
                let mut local = first;
                if j + 1 < n && chars[j] == ':' && chars[j + 1] != ':' {
                    if chars[j + 1] == '*' {
                        out.push(Tok::NsStar(local));
                        i = j + 2;
                        continue;
                    }
                    if is_name_start(chars[j + 1]) {
                        let mut k = j + 1;
                        while k < n && is_name_char(chars[k]) {
                            k += 1;
                        }
                        prefix = Some(local);
                        local = chars[j + 1..k].iter().collect();
                        j = k;
                    } else {
                        return Err(expr_err());
                    }
                }
                let after = skip_ws(j);
                if prefix.is_none()
                    && after + 1 < n
                    && chars[after] == ':'
                    && chars[after + 1] == ':'
                {
                    out.push(Tok::AxisName(local));
                    i = j;
                    continue;
                }
                if after < n && chars[after] == '(' {
                    if prefix.is_none()
                        && matches!(
                            local.as_str(),
                            "node" | "text" | "comment" | "processing-instruction"
                        )
                    {
                        out.push(Tok::NodeType(local));
                    } else {
                        out.push(Tok::FuncName(prefix, local));
                    }
                    i = j;
                    continue;
                }
                out.push(Tok::Name(prefix, local));
                i = j;
            }
            _ => return Err(expr_err()),
        }
    }
    Ok(out)
}

/// Number ::= Digits ('.' Digits?)? | '.' Digits, plus an exponent
/// (accepted by PHP, observed on PHP 8.5: `1e5` is 100000).
fn lex_number(chars: &[char], mut i: usize) -> (f64, usize) {
    let start = i;
    let n = chars.len();
    while i < n && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < n && chars[i] == '.' {
        i += 1;
        while i < n && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < n && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < n && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < n && chars[j].is_ascii_digit() {
            while j < n && chars[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let mut s: String = chars[start..i].iter().collect();
    if s.starts_with('.') {
        s.insert(0, '0');
    }
    if s.ends_with('.') {
        s.push('0');
    }
    (s.parse().unwrap_or(f64::NAN), i)
}

// ----- parser -------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

type R<T> = Result<T, XPathError>;

fn err<T>() -> R<T> {
    Err(XPathError::new(K::Expr))
}

/// Compiles an expression.
pub(crate) fn parse(s: &str) -> R<Expr> {
    let toks = lex(s)?;
    let mut p = Parser { toks, pos: 0 };
    if p.toks.is_empty() {
        return err();
    }
    let e = p.or_expr()?;
    if p.pos != p.toks.len() {
        return err();
    }
    Ok(optimize(e))
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn or_expr(&mut self) -> R<Expr> {
        let mut l = self.and_expr()?;
        while self.eat(&Tok::Or) {
            let r = self.and_expr()?;
            l = Expr::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> R<Expr> {
        let mut l = self.eq_expr()?;
        while self.eat(&Tok::And) {
            let r = self.eq_expr()?;
            l = Expr::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn eq_expr(&mut self) -> R<Expr> {
        let mut l = self.rel_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Eq) => CmpOp::Eq,
                Some(Tok::Ne) => CmpOp::Ne,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.rel_expr()?;
            l = Expr::Cmp(op, Box::new(l), Box::new(r));
        }
    }

    fn rel_expr(&mut self) -> R<Expr> {
        let mut l = self.add_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => CmpOp::Lt,
                Some(Tok::Le) => CmpOp::Le,
                Some(Tok::Gt) => CmpOp::Gt,
                Some(Tok::Ge) => CmpOp::Ge,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.add_expr()?;
            l = Expr::Cmp(op, Box::new(l), Box::new(r));
        }
    }

    fn add_expr(&mut self) -> R<Expr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => ArOp::Add,
                Some(Tok::Minus) => ArOp::Sub,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.mul_expr()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
    }

    fn mul_expr(&mut self) -> R<Expr> {
        let mut l = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Mul) => ArOp::Mul,
                Some(Tok::Div) => ArOp::Div,
                Some(Tok::Mod) => ArOp::Mod,
                _ => return Ok(l),
            };
            self.pos += 1;
            let r = self.unary_expr()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
    }

    fn unary_expr(&mut self) -> R<Expr> {
        if self.eat(&Tok::Minus) {
            let e = self.unary_expr()?;
            return Ok(Expr::Neg(Box::new(e)));
        }
        self.union_expr()
    }

    fn union_expr(&mut self) -> R<Expr> {
        let mut l = self.path_expr()?;
        while self.eat(&Tok::Pipe) {
            let r = self.path_expr()?;
            l = Expr::Union(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn starts_primary(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Var(_))
                | Some(Tok::LParen)
                | Some(Tok::Literal(_))
                | Some(Tok::Number(_))
                | Some(Tok::FuncName(..))
        )
    }

    fn path_expr(&mut self) -> R<Expr> {
        if self.starts_primary() {
            let prim = self.primary()?;
            let mut preds = Vec::new();
            while self.peek() == Some(&Tok::LBracket) {
                preds.push(self.predicate()?);
            }
            let mut steps = Vec::new();
            match self.peek() {
                Some(Tok::Slash) => {
                    self.pos += 1;
                    self.relative_path(&mut steps)?;
                }
                Some(Tok::DSlash) => {
                    self.pos += 1;
                    steps.push(descendant_or_self());
                    self.relative_path(&mut steps)?;
                }
                _ => {}
            }
            if preds.is_empty() && steps.is_empty() {
                return Ok(prim);
            }
            return Ok(Expr::Path(Box::new(PathExpr {
                start: Start::Filter(prim, preds),
                steps,
            })));
        }
        // Location path.
        let mut steps = Vec::new();
        let start = match self.peek() {
            Some(Tok::Slash) => {
                self.pos += 1;
                if self.starts_step() {
                    self.relative_path(&mut steps)?;
                }
                Start::Root
            }
            Some(Tok::DSlash) => {
                self.pos += 1;
                steps.push(descendant_or_self());
                self.relative_path(&mut steps)?;
                Start::Root
            }
            _ => {
                if !self.starts_step() {
                    return err();
                }
                self.relative_path(&mut steps)?;
                Start::Context
            }
        };
        Ok(Expr::Path(Box::new(PathExpr { start, steps })))
    }

    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Dot)
                | Some(Tok::DotDot)
                | Some(Tok::At)
                | Some(Tok::AxisName(_))
                | Some(Tok::Star)
                | Some(Tok::NsStar(_))
                | Some(Tok::Name(..))
                | Some(Tok::NodeType(_))
        )
    }

    fn relative_path(&mut self, steps: &mut Vec<Step>) -> R<()> {
        steps.push(self.step()?);
        loop {
            match self.peek() {
                Some(Tok::Slash) => {
                    self.pos += 1;
                    steps.push(self.step()?);
                }
                Some(Tok::DSlash) => {
                    self.pos += 1;
                    steps.push(descendant_or_self());
                    steps.push(self.step()?);
                }
                _ => return Ok(()),
            }
        }
    }

    fn step(&mut self) -> R<Step> {
        match self.peek() {
            Some(Tok::Dot) => {
                self.pos += 1;
                return Ok(Step {
                    axis: Axis::SelfAxis,
                    test: NodeTest::Node,
                    preds: Vec::new(),
                });
            }
            Some(Tok::DotDot) => {
                self.pos += 1;
                return Ok(Step {
                    axis: Axis::Parent,
                    test: NodeTest::Node,
                    preds: Vec::new(),
                });
            }
            _ => {}
        }
        let axis = match self.peek() {
            Some(Tok::At) => {
                self.pos += 1;
                Axis::Attribute
            }
            Some(Tok::AxisName(a)) => {
                let Some(ax) = Axis::from_name(a) else {
                    return err();
                };
                self.pos += 1;
                if !self.eat(&Tok::ColonColon) {
                    return err();
                }
                ax
            }
            _ => Axis::Child,
        };
        let test = match self.peek().cloned() {
            Some(Tok::Star) => {
                self.pos += 1;
                NodeTest::Any
            }
            Some(Tok::NsStar(p)) => {
                self.pos += 1;
                NodeTest::NsAny(p)
            }
            Some(Tok::Name(p, l)) => {
                self.pos += 1;
                NodeTest::Name(p, l)
            }
            Some(Tok::NodeType(t)) => {
                self.pos += 1;
                if !self.eat(&Tok::LParen) {
                    return err();
                }
                let test = match t.as_str() {
                    "node" => NodeTest::Node,
                    "text" => NodeTest::Text,
                    "comment" => NodeTest::Comment,
                    _ => {
                        if let Some(Tok::Literal(l)) = self.peek().cloned() {
                            self.pos += 1;
                            NodeTest::Pi(Some(l))
                        } else if self.peek() == Some(&Tok::RParen) {
                            NodeTest::Pi(None)
                        } else {
                            return Err(XPathError::new(K::StartLiteral));
                        }
                    }
                };
                if !self.eat(&Tok::RParen) {
                    return Err(XPathError::new(K::Unclosed));
                }
                test
            }
            _ => return err(),
        };
        let mut preds = Vec::new();
        while self.peek() == Some(&Tok::LBracket) {
            preds.push(self.predicate()?);
        }
        Ok(Step { axis, test, preds })
    }

    fn predicate(&mut self) -> R<Expr> {
        self.pos += 1; // [
        let e = self.or_expr()?;
        if !self.eat(&Tok::RBracket) {
            return err();
        }
        Ok(e)
    }

    fn primary(&mut self) -> R<Expr> {
        match self.peek().cloned() {
            Some(Tok::Var(v)) => Err(XPathError::with_name(K::UndefVariable, &v)),
            Some(Tok::LParen) => {
                self.pos += 1;
                let e = self.or_expr()?;
                if !self.eat(&Tok::RParen) {
                    return err();
                }
                Ok(e)
            }
            Some(Tok::Literal(l)) => {
                self.pos += 1;
                Ok(Expr::Literal(l))
            }
            Some(Tok::Number(n)) => {
                self.pos += 1;
                Ok(Expr::Number(n))
            }
            Some(Tok::FuncName(prefix, local)) => {
                self.pos += 1;
                if !self.eat(&Tok::LParen) {
                    return err();
                }
                let mut args = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        args.push(self.or_expr()?);
                        if self.eat(&Tok::Comma) {
                            continue;
                        }
                        if self.eat(&Tok::RParen) {
                            break;
                        }
                        return err();
                    }
                }
                match prefix {
                    Some(p) => Ok(Expr::ExtFn(p, local, args)),
                    None => {
                        let Some(f) = CoreFn::from_name(&local) else {
                            return Err(XPathError::with_name(K::UnknownFunction, &local));
                        };
                        let (min, max) = f.arity();
                        if args.len() < min || args.len() > max {
                            return Err(XPathError::new(K::Arity));
                        }
                        Ok(Expr::Fn(f, args))
                    }
                }
            }
            _ => err(),
        }
    }
}

fn descendant_or_self() -> Step {
    Step {
        axis: Axis::DescendantOrSelf,
        test: NodeTest::Node,
        preds: Vec::new(),
    }
}

/// True when a predicate's value can depend on the context position or
/// size, or can be a number (so it cannot move to another axis).
fn positional(e: &Expr) -> bool {
    match e {
        Expr::Number(_) | Expr::Arith(..) | Expr::Neg(_) => true,
        Expr::Fn(f, args) => {
            matches!(
                f,
                CoreFn::Last
                    | CoreFn::Position
                    | CoreFn::Count
                    | CoreFn::Number
                    | CoreFn::Sum
                    | CoreFn::Floor
                    | CoreFn::Ceiling
                    | CoreFn::Round
                    | CoreFn::StringLength
            ) || args.iter().any(positional_inside)
        }
        Expr::ExtFn(..) => true,
        Expr::Or(a, b) | Expr::And(a, b) | Expr::Cmp(_, a, b) | Expr::Union(a, b) => {
            positional_inside(a) || positional_inside(b)
        }
        Expr::Literal(_) => false,
        Expr::Path(p) => match &p.start {
            Start::Filter(f, preds) => positional_inside(f) || preds.iter().any(positional_inside),
            _ => false,
        },
    }
}

/// True when `e` uses position() or last() of the enclosing context.
fn positional_inside(e: &Expr) -> bool {
    match e {
        Expr::Fn(CoreFn::Last | CoreFn::Position, _) => true,
        Expr::Fn(_, args) | Expr::ExtFn(_, _, args) => args.iter().any(positional_inside),
        Expr::Or(a, b)
        | Expr::And(a, b)
        | Expr::Cmp(_, a, b)
        | Expr::Union(a, b)
        | Expr::Arith(_, a, b) => positional_inside(a) || positional_inside(b),
        Expr::Neg(a) => positional_inside(a),
        Expr::Literal(_) | Expr::Number(_) => false,
        Expr::Path(p) => match &p.start {
            Start::Filter(f, preds) => positional_inside(f) || preds.iter().any(positional_inside),
            _ => false,
        },
    }
}

/// Rewrites `descendant-or-self::node()/child::X[p]` to `descendant::X[p]`
/// when the predicates do not depend on positions (same result, one pass).
fn optimize(e: Expr) -> Expr {
    match e {
        Expr::Path(mut p) => {
            let mut steps = Vec::with_capacity(p.steps.len());
            let mut it = std::mem::take(&mut p.steps).into_iter().peekable();
            while let Some(mut s) = it.next() {
                s.preds = s.preds.into_iter().map(optimize).collect();
                if s.axis == Axis::DescendantOrSelf
                    && s.test == NodeTest::Node
                    && s.preds.is_empty()
                    && let Some(next) = it.peek()
                    && next.axis == Axis::Child
                    && !next.preds.iter().any(positional)
                {
                    let mut next = it.next().unwrap();
                    next.axis = Axis::Descendant;
                    next.preds = next.preds.into_iter().map(optimize).collect();
                    steps.push(next);
                    continue;
                }
                steps.push(s);
            }
            p.steps = steps;
            if let Start::Filter(f, preds) = p.start {
                p.start = Start::Filter(optimize(f), preds.into_iter().map(optimize).collect());
            }
            Expr::Path(p)
        }
        Expr::Or(a, b) => Expr::Or(Box::new(optimize(*a)), Box::new(optimize(*b))),
        Expr::And(a, b) => Expr::And(Box::new(optimize(*a)), Box::new(optimize(*b))),
        Expr::Cmp(o, a, b) => Expr::Cmp(o, Box::new(optimize(*a)), Box::new(optimize(*b))),
        Expr::Arith(o, a, b) => Expr::Arith(o, Box::new(optimize(*a)), Box::new(optimize(*b))),
        Expr::Union(a, b) => Expr::Union(Box::new(optimize(*a)), Box::new(optimize(*b))),
        Expr::Neg(a) => Expr::Neg(Box::new(optimize(*a))),
        Expr::Fn(f, args) => Expr::Fn(f, args.into_iter().map(optimize).collect()),
        Expr::ExtFn(p, l, args) => Expr::ExtFn(p, l, args.into_iter().map(optimize).collect()),
        e => e,
    }
}
