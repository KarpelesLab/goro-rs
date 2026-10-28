//! XPath 1.0 (W3C Recommendation, 16 November 1999) over a
//! [`tree::Arena`](crate::tree::Arena), for `DOMXPath`, `Dom\XPath` and
//! `SimpleXMLElement::xpath()`.
//!
//! ```ignore
//! let expr = xpath::compile("//a[@href]")?;
//! let mut ctx = xpath::Context::new();
//! ctx.register_namespace("x", "urn:x");
//! let value = xpath::evaluate(&mut arena, &expr, context_node, &mut ctx)?;
//! ```
//!
//! The data model follows PHP's (observed on PHP 8.5): doctype and DTD
//! declaration nodes are not part of the tree; CDATA sections are text
//! nodes; adjacent text nodes stay separate; `xmlns` attributes of `Dom\`
//! documents are attribute nodes, while the namespace axis lists the
//! `xml` namespace plus the declarations held in
//! [`ElementData::ns_decls`](crate::tree::ElementData) (legacy documents).
//! Namespace nodes are created in the arena as
//! [`NodeData::Namespace`](crate::tree::NodeData) nodes whose parent is
//! their element (they are not among its children); one evaluation reuses
//! them per (element, prefix). [`Context::namespace_nodes`] lists the ones
//! created, so the caller can free them (with
//! [`Arena::free_subtree`](crate::tree::Arena::free_subtree)) once nothing
//! refers to them.

mod eval;
pub mod number;
mod parser;

use crate::tree::{Arena, NodeId};
pub use number::{number_to_string, string_to_number};

/// The kinds of XPath errors, with the libxml error codes and messages
/// PHP reports for them (observed on PHP 8.5, `libxml_get_errors()`:
/// level 2, line 0, column 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XPathErrorKind {
    /// 1202 "Unfinished literal"
    UnfinishedLiteral,
    /// 1203 "Start of literal" (`processing-instruction(1)`)
    StartLiteral,
    /// 1204 "Expected $ for variable reference"
    VariableRef,
    /// 1205 "Undefined variable: NAME"
    UndefVariable,
    /// 1207 "Invalid expression"
    Expr,
    /// 1208 "Missing closing curly brace" (`node(1)`)
    Unclosed,
    /// 1209 "Unregistered function: NAME"
    UnknownFunction,
    /// 1211 "Invalid type"
    InvalidType,
    /// 1212 "Invalid number of arguments"
    Arity,
    /// 1219 "Undefined namespace prefix: PREFIX"
    UndefPrefix,
    /// An error raised by an extension function callback; the message and
    /// payload are the callback's.
    Callback,
}

/// An XPath compilation or evaluation error.
#[derive(Debug)]
pub struct XPathError {
    /// The kind.
    pub kind: XPathErrorKind,
    /// The message PHP prints (`Warning: DOMXPath::evaluate(): MESSAGE`),
    /// without trailing newline.
    pub message: String,
    /// For [`XPathErrorKind::Callback`]: whatever the callback attached
    /// (for instance a PHP exception to rethrow).
    pub payload: Option<Box<dyn std::any::Any>>,
}

impl XPathError {
    pub(crate) fn new(kind: XPathErrorKind) -> XPathError {
        let message = match kind {
            XPathErrorKind::UnfinishedLiteral => "Unfinished literal",
            XPathErrorKind::StartLiteral => "Start of literal",
            XPathErrorKind::VariableRef => "Expected $ for variable reference",
            XPathErrorKind::UndefVariable => "Undefined variable",
            XPathErrorKind::Expr => "Invalid expression",
            XPathErrorKind::Unclosed => "Missing closing curly brace",
            XPathErrorKind::UnknownFunction => "Unregistered function",
            XPathErrorKind::InvalidType => "Invalid type",
            XPathErrorKind::Arity => "Invalid number of arguments",
            XPathErrorKind::UndefPrefix => "Undefined namespace prefix",
            XPathErrorKind::Callback => "Callback error",
        };
        XPathError {
            kind,
            message: message.into(),
            payload: None,
        }
    }

    pub(crate) fn with_name(kind: XPathErrorKind, name: &str) -> XPathError {
        let mut e = XPathError::new(kind);
        e.message = format!("{}: {name}", e.message);
        e
    }

    /// An error for an extension function callback to return.
    pub fn callback(
        message: impl Into<String>,
        payload: Option<Box<dyn std::any::Any>>,
    ) -> XPathError {
        XPathError {
            kind: XPathErrorKind::Callback,
            message: message.into(),
            payload,
        }
    }

    /// The libxml error code (`LibXMLError::$code`); 0 for callback errors.
    pub fn code(&self) -> i32 {
        match self.kind {
            XPathErrorKind::UnfinishedLiteral => 1202,
            XPathErrorKind::StartLiteral => 1203,
            XPathErrorKind::VariableRef => 1204,
            XPathErrorKind::UndefVariable => 1205,
            XPathErrorKind::Expr => 1207,
            XPathErrorKind::Unclosed => 1208,
            XPathErrorKind::UnknownFunction => 1209,
            XPathErrorKind::InvalidType => 1211,
            XPathErrorKind::Arity => 1212,
            XPathErrorKind::UndefPrefix => 1219,
            XPathErrorKind::Callback => 0,
        }
    }
}

impl std::fmt::Display for XPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// A compiled XPath expression (reusable across evaluations).
#[derive(Clone, Debug)]
pub struct XPathExpr {
    expr: parser::Expr,
}

/// Compiles an expression. Syntax errors, unknown core functions, wrong
/// argument counts and variable references are reported here; namespace
/// prefixes and extension functions are resolved by [`evaluate`].
pub fn compile(expr: &str) -> Result<XPathExpr, XPathError> {
    Ok(XPathExpr {
        expr: parser::parse(expr)?,
    })
}

/// An XPath value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A node-set, in document order without duplicates.
    NodeSet(Vec<NodeId>),
    /// A boolean.
    Boolean(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
}

impl Value {
    /// `boolean()` of the value.
    pub fn to_boolean(&self) -> bool {
        match self {
            Value::NodeSet(v) => !v.is_empty(),
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
        }
    }

    /// `string()` of the value (a node-set gives the string-value of its
    /// first node).
    pub fn to_string_value(&self, arena: &Arena) -> String {
        match self {
            Value::NodeSet(v) => v.first().map_or(String::new(), |&n| string_value(arena, n)),
            Value::Boolean(b) => if *b { "true" } else { "false" }.into(),
            Value::Number(n) => number_to_string(*n),
            Value::String(s) => s.clone(),
        }
    }

    /// `number()` of the value.
    pub fn to_number(&self, arena: &Arena) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::String(s) => string_to_number(s),
            Value::NodeSet(_) => string_to_number(&self.to_string_value(arena)),
        }
    }
}

/// The XPath string-value of a node (section 5).
pub fn string_value(arena: &Arena, n: NodeId) -> String {
    eval::string_value(arena, n)
}

/// Extension functions (`DOMXPath::registerPhpFunctions()`: the
/// `php:function()` and `php:functionString()` functions in the
/// `http://php.net/xpath` namespace, and `registerPhpFunctionNS()`
/// functions).
pub trait FunctionHandler {
    /// Whether `{ns}local` is a known function (checked before evaluation;
    /// unknown ones give "Unregistered function: prefix:local").
    fn has_function(&self, ns: &str, local: &str) -> bool;

    /// Calls the function with its evaluated arguments. `arena` is the
    /// arena being queried, free to use (and to modify) during the call.
    /// Returned node-sets need not be sorted.
    fn call(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        args: Vec<Value>,
    ) -> Result<Value, XPathError>;
}

/// The evaluation context: namespace bindings, extension functions and
/// the namespace nodes created.
#[derive(Default)]
pub struct Context<'a> {
    /// Prefix bindings (`registerNamespace()`), looked up first.
    pub namespaces: Vec<(String, String)>,
    /// Also resolve prefixes declared in scope of the context node
    /// (`$registerNodeNS = true`, the default of `query()`/`evaluate()`).
    pub register_node_ns: bool,
    /// Extension functions.
    pub functions: Option<&'a mut dyn FunctionHandler>,
    /// Namespace nodes created by the evaluations made with this context.
    pub namespace_nodes: Vec<NodeId>,
}

impl<'a> Context<'a> {
    /// A context with no bindings and `register_node_ns` on.
    pub fn new() -> Context<'a> {
        Context {
            register_node_ns: true,
            ..Default::default()
        }
    }

    /// Binds a prefix (later bindings of the same prefix win).
    pub fn register_namespace(&mut self, prefix: &str, uri: &str) {
        self.namespaces.retain(|(p, _)| p != prefix);
        self.namespaces.push((prefix.to_string(), uri.to_string()));
    }
}

/// Evaluates `expr` with `context` as the context node (position 1, size
/// 1).
pub fn evaluate(
    arena: &mut Arena,
    expr: &XPathExpr,
    context: NodeId,
    ctx: &mut Context<'_>,
) -> Result<Value, XPathError> {
    eval::evaluate(arena, &expr.expr, context, ctx)
}
