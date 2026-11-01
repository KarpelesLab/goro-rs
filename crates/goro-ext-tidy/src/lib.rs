//! A from-scratch HTML cleaner and pretty printer for ext/tidy (PHP manual:
//! Tidy). It reads tag soup into a repaired tree, can clean that tree, and
//! prints it back with configurable layout, keeping the diagnostics PHP
//! exposes through `tidy::$errorBuffer`.
//!
//! Its behaviour comes from the ext/tidy `.phpt` tests' expected output and
//! the HTML specifications. Where those say nothing, the choices are this
//! crate's own, so its output on arbitrary documents is similar to, but not
//! identical with, what HTML Tidy prints.

pub mod config;
pub mod tags;

mod clean;
mod lexer;
mod parser;
mod print;

pub use config::{Config, Encoding, Kind, OPTIONS, Opt, Val, find_option};
pub use tags::{TAG_COUNT, TAGS, Tag};

use std::fmt::Write;

/// A node's index in its document.
pub type NodeId = usize;

/// Node types, numbered as the `TIDY_NODETYPE_*` constants (tests 010.phpt,
/// 012.phpt and bug77040.phpt show root 0, text 4, start tag 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Root = 0,
    Doctype = 1,
    Comment = 2,
    ProcIns = 3,
    Text = 4,
    Start = 5,
    End = 6,
    StartEnd = 7,
    Cdata = 8,
    Section = 9,
    Asp = 10,
    Jste = 11,
    Php = 12,
    XmlDecl = 13,
}

/// An attribute; a value of `None` was written without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: Option<String>,
}

/// How a tag declared through the configuration behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserKind {
    Block,
    Empty,
    Inline,
    Pre,
}

/// One node of the tree.
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeType,
    pub name: String,
    pub tag: Option<&'static Tag>,
    pub user: Option<UserKind>,
    pub attrs: Vec<Attr>,
    pub text: String,
    pub line: u32,
    pub col: u32,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Inserted by the parser rather than written in the input.
    pub implicit: bool,
}

/// Message severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
    Access,
}

/// A parsed document.
pub struct Document {
    pub config: Config,
    pub nodes: Vec<Node>,
    pub root: NodeId,
    errbuf: String,
    errors: u32,
    warnings: u32,
    access: u32,
    /// Whether the input declared or used XHTML.
    xhtml: bool,
    proprietary_seen: bool,
}

impl Document {
    /// An empty document with these options.
    pub fn new(config: Config) -> Document {
        let root = Node {
            kind: NodeType::Root,
            name: String::new(),
            tag: None,
            user: None,
            attrs: Vec::new(),
            text: String::new(),
            line: 1,
            col: 1,
            parent: None,
            children: Vec::new(),
            implicit: false,
        };
        let errbuf = config.messages.clone();
        Document {
            config,
            nodes: vec![root],
            root: 0,
            errbuf,
            errors: 0,
            warnings: 0,
            access: 0,
            xhtml: false,
            proprietary_seen: false,
        }
    }

    /// Parses the input into the tree.
    pub fn parse(&mut self, input: &[u8]) {
        parser::parse(self, input);
    }

    /// Cleans and repairs the tree (PHP manual: tidy::cleanRepair).
    pub fn clean_repair(&mut self) {
        clean::clean_repair(self);
    }

    /// Adds the document summary to the messages (PHP manual:
    /// tidy::diagnose; the wording is in test 004.phpt).
    pub fn run_diagnostics(&mut self) {
        if self.config.int("accessibility-check") > 0 {
            clean::accessibility_checks(self);
        }
        let v = self.version_name();
        self.info(&format!("Document content looks like {v}"));
        if self.config.bool("quiet") {
            return;
        }
        if self.errors == 0 && self.warnings == 0 && self.access == 0 {
            self.errbuf.push_str("No warnings or errors were found.\n");
        } else {
            let plural = |n: u32, w: &str| {
                if n == 1 {
                    format!("{n} {w}")
                } else {
                    format!("{n} {w}s")
                }
            };
            let (w, e) = (self.warnings, self.errors);
            let mut line = format!(
                "Tidy found {} and {}",
                plural(w, "warning"),
                plural(e, "error")
            );
            if self.access > 0 {
                let _ = write!(
                    line,
                    " and {}",
                    plural(self.access, "accessibility warning")
                );
            }
            line.push_str("!\n");
            self.errbuf.push_str(&line);
        }
    }

    /// The whole document as it would be saved.
    pub fn output(&self) -> Vec<u8> {
        print::document(self)
    }

    /// One node's markup.
    pub fn node_text(&self, id: NodeId) -> Vec<u8> {
        print::node(self, id)
    }

    pub fn error_buffer(&self) -> &str {
        &self.errbuf
    }

    pub fn error_count(&self) -> u32 {
        self.errors
    }

    pub fn warning_count(&self) -> u32 {
        self.warnings
    }

    pub fn access_count(&self) -> u32 {
        self.access
    }

    pub fn config_count(&self) -> u32 {
        self.config.errors
    }

    /// 2 with errors, 1 with warnings, else 0 (test tidy_error.phpt).
    pub fn status(&self) -> i64 {
        if self.errors > 0 {
            2
        } else if self.warnings > 0 {
            1
        } else {
            0
        }
    }

    pub fn is_xhtml(&self) -> bool {
        self.xhtml
    }

    pub fn is_xml(&self) -> bool {
        self.config.bool("input-xml")
    }

    /// The HTML version the document declares, as a number: 200, 320, 400,
    /// 401 for the old versions, 500 for HTML5.
    pub fn html_version(&self) -> i64 {
        match self.declared_version() {
            Some(v) if v.contains("2.0") => 200,
            Some(v) if v.contains("3.2") => 320,
            Some(v) if v.contains("4.01") => 401,
            Some(v) if v.contains("4.0") => 400,
            Some(v) if v.contains("XHTML 1.1") => 11,
            Some(v) if v.contains("XHTML") => 10,
            _ => 500,
        }
    }

    /// The doctype node.
    pub fn doctype(&self) -> Option<NodeId> {
        self.nodes[self.root]
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes[c].kind == NodeType::Doctype)
    }

    /// The `html` element.
    pub fn html(&self) -> Option<NodeId> {
        self.nodes[self.root]
            .children
            .iter()
            .copied()
            .find(|&c| self.is_element(c, "html"))
    }

    /// The `head` element.
    pub fn head(&self) -> Option<NodeId> {
        self.child_named(self.html()?, "head")
    }

    /// The `body` element (none in a frameset document; test 029.phpt).
    pub fn body(&self) -> Option<NodeId> {
        self.child_named(self.html()?, "body")
    }

    fn child_named(&self, id: NodeId, name: &str) -> Option<NodeId> {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .find(|&c| self.is_element(c, name))
    }

    /// Whether the node is that element.
    pub fn is_element(&self, id: NodeId, name: &str) -> bool {
        let n = &self.nodes[id];
        matches!(n.kind, NodeType::Start | NodeType::StartEnd) && n.name == name
    }

    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id].parent?;
        let sib = &self.nodes[p].children;
        let i = sib.iter().position(|&c| c == id)?;
        i.checked_sub(1).map(|j| sib[j])
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id].parent?;
        let sib = &self.nodes[p].children;
        let i = sib.iter().position(|&c| c == id)?;
        sib.get(i + 1).copied()
    }

    /// `tidyNode::$proprietary`.
    pub fn is_proprietary(&self, id: NodeId) -> bool {
        self.nodes[id]
            .tag
            .is_some_and(|t| t.flags & tags::PROPRIETARY != 0)
    }

    fn flags(&self, id: NodeId) -> u32 {
        let n = &self.nodes[id];
        match (n.tag, n.user) {
            (Some(t), _) => t.flags,
            (None, Some(UserKind::Block)) => tags::BLOCK,
            (None, Some(UserKind::Empty)) => tags::INLINE | tags::EMPTY,
            (None, Some(UserKind::Inline)) => tags::INLINE,
            (None, Some(UserKind::Pre)) => tags::BLOCK | tags::PRE,
            (None, None) => 0,
        }
    }

    fn new_node(&mut self, kind: NodeType, name: &str, line: u32, col: u32) -> NodeId {
        self.nodes.push(Node {
            kind,
            name: name.to_string(),
            tag: None,
            user: None,
            attrs: Vec::new(),
            text: String::new(),
            line,
            col,
            parent: None,
            children: Vec::new(),
            implicit: false,
        });
        self.nodes.len() - 1
    }

    fn append(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    fn insert_before(&mut self, sibling: NodeId, child: NodeId) {
        if let Some(p) = self.nodes[sibling].parent {
            let i = self.nodes[p]
                .children
                .iter()
                .position(|&c| c == sibling)
                .unwrap_or(0);
            self.nodes[child].parent = Some(p);
            self.nodes[p].children.insert(i, child);
        }
    }

    /// Takes the node out of its parent; its index stays valid for a
    /// `tidyNode` still holding it.
    fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.nodes[id].parent.take() {
            self.nodes[p].children.retain(|&c| c != id);
        }
    }

    fn info(&mut self, m: &str) {
        if self.config.bool("show-info") && !self.config.bool("quiet") {
            let _ = writeln!(self.errbuf, "Info: {m}");
        }
    }

    /// Records a message at a position, as test 006.phpt shows them.
    fn report(&mut self, level: Level, line: u32, col: u32, m: &str) {
        let (word, show) = match level {
            Level::Info => {
                self.info(m);
                return;
            }
            Level::Warning => {
                self.warnings += 1;
                ("Warning", self.config.bool("show-warnings"))
            }
            Level::Error => {
                self.errors += 1;
                (
                    "Error",
                    self.errors as i64 <= self.config.int("show-errors"),
                )
            }
            Level::Access => {
                self.access += 1;
                ("Access", true)
            }
        };
        if show {
            let _ = writeln!(self.errbuf, "line {line} column {col} - {word}: {m}");
        }
    }

    /// The version named by the doctype's public identifier.
    fn declared_version(&self) -> Option<String> {
        let d = &self.nodes[self.doctype()?].text;
        let start = d.find("-//W3C//DTD ")? + 12;
        let rest = &d[start..];
        let end = rest.find("//")?;
        Some(rest[..end].to_string())
    }

    /// The version as the "looks like" line names it (test 004.phpt).
    fn version_name(&self) -> String {
        if self.is_xml() {
            return "XML".to_string();
        }
        if let Some(v) = self.declared_version() {
            return v;
        }
        if self.proprietary_seen {
            return "HTML Proprietary".to_string();
        }
        "HTML5".to_string()
    }
}
