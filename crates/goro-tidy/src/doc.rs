//! The document structures of tidy-int.h and lexer.h: `TidyDocImpl`
//! (`Doc`), `Node`, `AttVal`, `Lexer`, `IStack`, and the tag dictionary
//! references.
//!
//! Porting conventions (shared by every module of this crate):
//! - Nodes and attributes live in arenas inside `Doc`; `Node*` becomes
//!   `NodeId` (or `Option<NodeId>` where C allows NULL), `AttVal*` becomes
//!   `AttId` / `Option<AttId>`. `doc[node]` / `doc[att]` index the arenas.
//!   Freeing is a no-op (the arena is dropped with the document), except
//!   where the C code observably resets state (see `lexer::free_node`).
//! - `tmbstr` / `ctmbstr` are byte strings: `Vec<u8>` / `&[u8]`
//!   (`Option<..>` where C allows NULL).
//! - Every `TY_(Name)` becomes `snake_case` `name` in the module named after
//!   the C file, taking `doc: &mut Doc` first (also where C takes `Lexer*`).
//! - `Node->tag` is an `Option<DictRef>`; the dictionary fields are read
//!   through `Doc::dict_*` (built-in tags are global, like tidy's static
//!   `tag_defs[]` that `AdjustTags` / `ResetTags` modify in place).

#![allow(dead_code)]

use std::ops::{Index, IndexMut};

use crate::access::AccessImpl;
use crate::attrdict::AttrVersion;
use crate::attrs::{AttribImpl, Attribute};
use crate::config::ConfigImpl;
use crate::enums::{AttrId, OptId, TagId};
use crate::pprint::PrintImpl;
use crate::streamio::{StreamIn, StreamOut};
use crate::tags::{CheckAttribsFn, ParserFn, TagsImpl};

/// `(~0u)`
pub const END_OF_STREAM: u32 = 0xFFFF_FFFF;

// lexer character types
pub const DIGIT: u32 = 1;
pub const LETTER: u32 = 2;
pub const NAMECHAR: u32 = 4;
pub const WHITE: u32 = 8;
pub const NEWLINE: u32 = 16;
pub const LOWERCASE: u32 = 32;
pub const UPPERCASE: u32 = 64;
pub const DIGITHEX: u32 = 128;

// content models
pub const CM_UNKNOWN: u32 = 0;
pub const CM_EMPTY: u32 = 1 << 0;
pub const CM_HTML: u32 = 1 << 1;
pub const CM_HEAD: u32 = 1 << 2;
pub const CM_BLOCK: u32 = 1 << 3;
pub const CM_INLINE: u32 = 1 << 4;
pub const CM_LIST: u32 = 1 << 5;
pub const CM_DEFLIST: u32 = 1 << 6;
pub const CM_TABLE: u32 = 1 << 7;
pub const CM_ROWGRP: u32 = 1 << 8;
pub const CM_ROW: u32 = 1 << 9;
pub const CM_FIELD: u32 = 1 << 10;
pub const CM_OBJECT: u32 = 1 << 11;
pub const CM_PARAM: u32 = 1 << 12;
pub const CM_FRAMES: u32 = 1 << 13;
pub const CM_HEADING: u32 = 1 << 14;
pub const CM_OPT: u32 = 1 << 15;
pub const CM_IMG: u32 = 1 << 16;
pub const CM_MIXED: u32 = 1 << 17;
pub const CM_NO_INDENT: u32 = 1 << 18;
pub const CM_OBSOLETE: u32 = 1 << 19;
pub const CM_NEW: u32 = 1 << 20;
pub const CM_OMITST: u32 = 1 << 21;

// HTML versions
pub const XXXX: u32 = 0;
pub const HT20: u32 = 1;
pub const HT32: u32 = 2;
pub const H40S: u32 = 4;
pub const H40T: u32 = 8;
pub const H40F: u32 = 16;
pub const H41S: u32 = 32;
pub const H41T: u32 = 64;
pub const H41F: u32 = 128;
pub const X10S: u32 = 256;
pub const X10T: u32 = 512;
pub const X10F: u32 = 1024;
pub const XH11: u32 = 2048;
pub const XB10: u32 = 4096;
pub const VERS_SUN: u32 = 8192;
pub const VERS_NETSCAPE: u32 = 16384;
pub const VERS_MICROSOFT: u32 = 32768;
pub const VERS_XML: u32 = 65536;
pub const HT50: u32 = 131072;
pub const XH50: u32 = 262144;
pub const VERS_UNKNOWN: u32 = XXXX;
pub const VERS_HTML20: u32 = HT20;
pub const VERS_HTML32: u32 = HT32;
pub const VERS_HTML40_STRICT: u32 = H40S | H41S | X10S;
pub const VERS_HTML40_LOOSE: u32 = H40T | H41T | X10T;
pub const VERS_FRAMESET: u32 = H40F | H41F | X10F;
pub const VERS_XHTML11: u32 = XH11;
pub const VERS_BASIC: u32 = XB10;
pub const VERS_HTML5: u32 = HT50 | XH50;
pub const VERS_HTML40: u32 = VERS_HTML40_STRICT | VERS_HTML40_LOOSE | VERS_FRAMESET;
pub const VERS_IFRAME: u32 = VERS_HTML40_LOOSE | VERS_FRAMESET;
pub const VERS_LOOSE: u32 = VERS_HTML20 | VERS_HTML32 | VERS_IFRAME;
pub const VERS_EVENTS: u32 = VERS_HTML40 | VERS_XHTML11;
pub const VERS_FROM32: u32 = VERS_HTML32 | VERS_HTML40 | HT50;
pub const VERS_FROM40: u32 = VERS_HTML40 | VERS_XHTML11 | VERS_BASIC | VERS_HTML5;
pub const VERS_XHTML: u32 = X10S | X10T | X10F | XH11 | XB10 | XH50;
pub const VERS_STRICT: u32 = VERS_HTML5 | VERS_HTML40_STRICT;
pub const VERS_ALL: u32 = VERS_HTML20 | VERS_HTML32 | VERS_FROM40 | XH50 | HT50;
pub const VERS_PROPRIETARY: u32 = VERS_NETSCAPE | VERS_MICROSOFT | VERS_SUN;

// character encodings (streamio.h)
pub const RAW: u32 = 0;
pub const ASCII: u32 = 1;
pub const LATIN0: u32 = 2;
pub const LATIN1: u32 = 3;
pub const UTF8: u32 = 4;
pub const ISO2022: u32 = 5;
pub const MACROMAN: u32 = 6;
pub const WIN1252: u32 = 7;
pub const IBM858: u32 = 8;
pub const UTF16LE: u32 = 9;
pub const UTF16BE: u32 = 10;
pub const UTF16: u32 = 11;
pub const BIG5: u32 = 12;
pub const SHIFTJIS: u32 = 13;

pub const CR: u32 = 0xD;
pub const LF: u32 = 0xA;

// TidyLineEnding
pub const TIDY_LF: u32 = 0;
pub const TIDY_CRLF: u32 = 1;
pub const TIDY_CR: u32 = 2;

// TidyTriState
pub const TIDY_NO_STATE: u32 = 0;
pub const TIDY_YES_STATE: u32 = 1;
pub const TIDY_AUTO_STATE: u32 = 2;

// TidyUseCustomTagsState
pub const TIDY_CUSTOM_NO: u32 = 0;
pub const TIDY_CUSTOM_BLOCKLEVEL: u32 = 1;
pub const TIDY_CUSTOM_EMPTY: u32 = 2;
pub const TIDY_CUSTOM_INLINE: u32 = 3;
pub const TIDY_CUSTOM_PRE: u32 = 4;

// TidyDoctypeModes
pub const TIDY_DOCTYPE_HTML5: u32 = 0;
pub const TIDY_DOCTYPE_OMIT: u32 = 1;
pub const TIDY_DOCTYPE_AUTO: u32 = 2;
pub const TIDY_DOCTYPE_STRICT: u32 = 3;
pub const TIDY_DOCTYPE_LOOSE: u32 = 4;
pub const TIDY_DOCTYPE_USER: u32 = 5;

// TidyDupAttrModes
pub const TIDY_KEEP_FIRST: u32 = 0;
pub const TIDY_KEEP_LAST: u32 = 1;

// TidyAttrSortStrategy
pub const TIDY_SORT_ATTR_NONE: u32 = 0;
pub const TIDY_SORT_ATTR_ALPHA: u32 = 1;

// TidyUppercase
pub const TIDY_UPPERCASE_NO: u32 = 0;
pub const TIDY_UPPERCASE_YES: u32 = 1;
pub const TIDY_UPPERCASE_PRESERVE: u32 = 2;

// TidyReportLevel
pub const TIDY_INFO: u32 = 350;
pub const TIDY_WARNING: u32 = 351;
pub const TIDY_CONFIG: u32 = 352;
pub const TIDY_ACCESS: u32 = 353;
pub const TIDY_ERROR: u32 = 354;
pub const TIDY_BAD_DOCUMENT: u32 = 355;
pub const TIDY_FATAL: u32 = 356;
pub const TIDY_DIALOGUE_SUMMARY: u32 = 357;
pub const TIDY_DIALOGUE_INFO: u32 = 358;
pub const TIDY_DIALOGUE_FOOTNOTE: u32 = 359;

// tidy-int.h flags
pub const FLG_BAD_FORM: u32 = 0x0000_0001;
pub const FLG_BAD_MAIN: u32 = 0x0000_0002;

// message.h bit fields
pub const BA_MISSING_IMAGE_ALT: u32 = 1;
pub const BA_MISSING_LINK_ALT: u32 = 2;
pub const BA_MISSING_SUMMARY: u32 = 4;
pub const BA_MISSING_IMAGE_MAP: u32 = 8;
pub const BA_USING_FRAMES: u32 = 16;
pub const BA_USING_NOFRAMES: u32 = 32;
pub const BA_INVALID_LINK_NOFRAMES: u32 = 64;
pub const BA_WAI: u32 = 1 << 31;
pub const USING_SPACER: u32 = 1;
pub const USING_LAYER: u32 = 2;
pub const USING_NOBR: u32 = 4;
pub const USING_FONT: u32 = 8;
pub const USING_BODY: u32 = 16;
pub const BC_VENDOR_SPECIFIC_CHARS: u32 = 1;
pub const BC_INVALID_SGML_CHARS: u32 = 2;
pub const BC_INVALID_UTF8: u32 = 4;
pub const BC_INVALID_UTF16: u32 = 8;
pub const BC_ENCODING_MISMATCH: u32 = 16;
pub const BC_INVALID_URI: u32 = 32;
pub const BC_INVALID_NCR: u32 = 64;
pub const FN_TRIM_EMPTY_ELEMENT: u32 = 1;
pub const REPLACED_CHAR: u32 = 0;
pub const DISCARDED_CHAR: u32 = 1;

pub const XHTML_NAMESPACE: &[u8] = b"http://www.w3.org/1999/xhtml";

/// NodeType (the order is tidy's, exposed as TIDY_NODETYPE_*)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NodeType {
    #[default]
    RootNode,
    DocTypeTag,
    CommentTag,
    ProcInsTag,
    TextNode,
    StartTag,
    EndTag,
    StartEndTag,
    CDATATag,
    SectionTag,
    AspTag,
    JsteTag,
    PhpTag,
    XmlDecl,
}

/// LexerState
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LexerState {
    #[default]
    LexContent,
    LexGt,
    LexEndTag,
    LexStartTag,
    LexComment,
    LexDocType,
    LexProcInstr,
    LexCdata,
    LexSection,
    LexAsp,
    LexJste,
    LexPhp,
    LexXmlDecl,
}

/// GetTokenMode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GetTokenMode {
    IgnoreWhitespace,
    MixedContent,
    Preformatted,
    IgnoreMarkup,
    OtherNamespace,
    CdataContent,
}

/// NodeTraversalSignal
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeTraversalSignal {
    ContinueTraversal,
    SkipChildren,
    SkipSiblings,
    SkipChildrenAndSiblings,
    VisitParent,
    ExitTraversal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub struct AttId(pub u32);

/// A `const Dict*`: a built-in tag, a user-declared tag (index into
/// `TagsImpl::declared`) or the dummy dictionary of XML tags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DictRef {
    Builtin(TagId),
    Declared(u32),
    Xml,
}

/// _Node
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    pub content: Option<NodeId>,
    pub last: Option<NodeId>,
    pub attributes: Option<AttId>,
    /// old tag when it was changed
    pub was: Option<DictRef>,
    pub tag: Option<DictRef>,
    /// name (None for text nodes)
    pub element: Option<Vec<u8>>,
    /// start of span onto the lexer's text buffer
    pub start: u32,
    /// end of span onto the lexer's text buffer
    pub end: u32,
    pub type_: NodeType,
    pub line: u32,
    pub column: u32,
    /// true if closed by explicit end tag
    pub closed: bool,
    /// true if inferred
    pub implicit: bool,
    /// true if followed by a line break
    pub linebreak: bool,
}

/// _AttVal
#[derive(Clone, Debug, Default)]
pub struct AttVal {
    pub next: Option<AttId>,
    pub dict: Option<&'static Attribute>,
    pub asp: Option<NodeId>,
    pub php: Option<NodeId>,
    pub delim: u32,
    pub attribute: Option<Vec<u8>>,
    pub value: Option<Vec<u8>>,
}

/// _IStack
#[derive(Clone, Debug, Default)]
pub struct IStack {
    pub tag: Option<DictRef>,
    pub element: Option<Vec<u8>>,
    pub attributes: Option<AttId>,
}

/// _Style (TagStyle)
#[derive(Clone, Debug, Default)]
pub struct TagStyle {
    pub tag: Vec<u8>,
    pub tag_class: Vec<u8>,
    pub properties: Vec<u8>,
}

/// _StyleProp
#[derive(Clone, Debug, Default)]
pub struct StyleProp {
    pub name: Vec<u8>,
    pub value: Option<Vec<u8>>,
}

/// _Lexer
#[derive(Clone, Debug, Default)]
pub struct Lexer {
    pub lines: u32,
    pub columns: u32,
    pub waswhite: bool,
    pub pushed: bool,
    pub insertspace: bool,
    pub exclude_blocks: bool,
    pub exiled: bool,
    pub isvoyager: bool,
    pub versions: u32,
    pub doctype: u32,
    pub version_emitted: u32,
    pub bad_doctype: bool,
    pub txtstart: u32,
    pub txtend: u32,
    pub state: LexerState,
    pub token: Option<NodeId>,
    pub itoken: Option<NodeId>,
    pub root: Option<NodeId>,
    pub parent: Option<NodeId>,
    pub seen_end_body: bool,
    pub seen_end_html: bool,
    /// lexbuf: the allocated buffer (bytes past `lexsize` keep whatever
    /// was written there, as in C)
    pub lexbuf: Vec<u8>,
    /// lexsize: the used part of `lexbuf`
    pub lexsize: u32,
    pub inode: Option<NodeId>,
    /// `IStack* insert`: index into `istack`
    pub insert: Option<usize>,
    /// `istacksize` is `istack.len()`
    pub istack: Vec<IStack>,
    pub istackbase: usize,
    /// the TagStyle linked list, in list order
    pub styles: Vec<TagStyle>,
}

impl Lexer {
    pub fn lexsize(&self) -> u32 {
        self.lexsize
    }
}

/// TidyOptionValue
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptValue {
    pub v: u64,
    pub p: Option<Vec<u8>>,
}

/// Argument of a message (the va_list of TY_(Report) and friends).
#[derive(Clone, Debug)]
pub enum RArg {
    Att(AttId),
    Str(Vec<u8>),
    Uint(u32),
    Int(i32),
    Bool(bool),
    Node(Option<NodeId>),
}

/// _TidyDocImpl
pub struct Doc {
    pub nodes: Vec<Node>,
    pub attvals: Vec<AttVal>,
    /// The Document Tree root (`&doc->root`)
    pub root: NodeId,
    pub lexer: Option<Box<Lexer>>,

    pub config: ConfigImpl,
    pub tags: TagsImpl,
    pub attribs: AttribImpl,
    pub access: AccessImpl,
    /// TidyMutedMessages
    pub muted: Vec<u32>,

    pub pprint: PrintImpl,

    pub doc_in: Option<StreamIn>,
    pub doc_out: Option<StreamOut>,
    pub errout: StreamOut,

    pub option_errors: u32,
    pub errors: u32,
    pub warnings: u32,
    pub access_errors: u32,
    pub info_messages: u32,
    pub doc_errors: u32,
    pub parse_status: i32,

    pub bad_access: u32,
    pub bad_layout: u32,
    pub bad_chars: u32,
    pub bad_form: u32,
    pub footnotes: u32,

    pub html5_mode: bool,
    pub xml_detected: bool,

    pub n_class_id: u32,
    pub input_had_bom: bool,
    pub given_doctype: Option<Vec<u8>>,

    /// Bumped whenever the node arena is reset (a new parse), so that
    /// holders of `NodeId`s can tell stale ids apart.
    pub generation: u64,
}

impl Index<NodeId> for Doc {
    type Output = Node;
    fn index(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }
}

impl IndexMut<NodeId> for Doc {
    fn index_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }
}

impl Index<AttId> for Doc {
    type Output = AttVal;
    fn index(&self, id: AttId) -> &AttVal {
        &self.attvals[id.0 as usize]
    }
}

impl IndexMut<AttId> for Doc {
    fn index_mut(&mut self, id: AttId) -> &mut AttVal {
        &mut self.attvals[id.0 as usize]
    }
}

impl Doc {
    /// Allocate a fresh node in the arena (all fields zeroed, like
    /// `TidyClearMemory`).
    pub fn alloc_node(&mut self) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::default());
        id
    }

    /// Allocate a fresh attribute in the arena.
    pub fn alloc_att(&mut self) -> AttId {
        let id = AttId(self.attvals.len() as u32);
        self.attvals.push(AttVal::default());
        id
    }

    /// The lexer (`doc->lexer`), which must exist.
    pub fn lx(&mut self) -> &mut Lexer {
        self.lexer.as_mut().expect("tidy: no lexer")
    }

    /// The lexer, read-only.
    pub fn lxr(&self) -> &Lexer {
        self.lexer.as_ref().expect("tidy: no lexer")
    }

    // ---- config (cfg / cfgBool / cfgAutoBool / cfgStr)

    pub fn cfg(&self, id: OptId) -> u32 {
        self.config.value[id as usize].v as u32
    }

    pub fn cfg_bool(&self, id: OptId) -> bool {
        self.config.value[id as usize].v != 0
    }

    pub fn cfg_auto_bool(&self, id: OptId) -> u32 {
        self.config.value[id as usize].v as u32
    }

    pub fn cfg_str(&self, id: OptId) -> Option<&[u8]> {
        self.config.value[id as usize].p.as_deref()
    }

    // ---- tags (TagId / TagIsId and the Dict fields)

    /// `TagId(node)`
    pub fn tag_id(&self, node: Option<NodeId>) -> TagId {
        match node {
            Some(n) => match self[n].tag {
                Some(t) => self.dict_id(t),
                None => TagId::UNKNOWN,
            },
            None => TagId::UNKNOWN,
        }
    }

    /// `TagIsId(node, tid)` for a non-NULL node
    pub fn is(&self, node: NodeId, tid: TagId) -> bool {
        match self[node].tag {
            Some(t) => self.dict_id(t) == tid,
            None => false,
        }
    }

    /// `TagIsId(node, tid)` for a node that may be NULL
    pub fn is_o(&self, node: Option<NodeId>, tid: TagId) -> bool {
        node.is_some_and(|n| self.is(n, tid))
    }

    pub fn dict_id(&self, d: DictRef) -> TagId {
        match d {
            DictRef::Builtin(t) => t,
            _ => TagId::UNKNOWN,
        }
    }

    pub fn dict_name(&self, d: DictRef) -> Option<&[u8]> {
        match d {
            DictRef::Builtin(t) => Some(crate::tags::builtin_def(t).name),
            DictRef::Declared(i) => self.tags.declared[i as usize]
                .as_ref()
                .map(|d| d.name.as_slice()),
            DictRef::Xml => None,
        }
    }

    pub fn dict_versions(&self, d: DictRef) -> u32 {
        match d {
            DictRef::Builtin(t) => crate::tags::builtin_def(t).versions,
            DictRef::Declared(i) => self.tags.declared[i as usize]
                .as_ref()
                .map_or(0, |d| d.versions),
            DictRef::Xml => VERS_XML,
        }
    }

    pub fn dict_model(&self, d: DictRef) -> u32 {
        match d {
            DictRef::Builtin(t) => crate::tags::builtin_model(t),
            DictRef::Declared(i) => self.tags.declared[i as usize]
                .as_ref()
                .map_or(0, |d| d.model),
            DictRef::Xml => CM_BLOCK,
        }
    }

    pub fn dict_parser(&self, d: DictRef) -> Option<ParserFn> {
        match d {
            DictRef::Builtin(t) => crate::tags::builtin_parser(t),
            DictRef::Declared(i) => self.tags.declared[i as usize]
                .as_ref()
                .and_then(|d| d.parser),
            DictRef::Xml => None,
        }
    }

    pub fn dict_chkattrs(&self, d: DictRef) -> Option<CheckAttribsFn> {
        match d {
            DictRef::Builtin(t) => crate::tags::builtin_def(t).chkattrs,
            DictRef::Declared(i) => self.tags.declared[i as usize]
                .as_ref()
                .and_then(|d| d.chkattrs),
            DictRef::Xml => None,
        }
    }

    pub fn dict_attrvers(&self, d: DictRef) -> &'static [AttrVersion] {
        match d {
            DictRef::Builtin(t) => crate::tags::builtin_def(t).attrvers,
            _ => &[],
        }
    }

    /// `node->tag->model` (0 when the node has no tag)
    pub fn model(&self, node: NodeId) -> u32 {
        self[node].tag.map_or(0, |t| self.dict_model(t))
    }

    /// `node->tag->parser`
    pub fn parser_of(&self, node: NodeId) -> Option<ParserFn> {
        self[node].tag.and_then(|t| self.dict_parser(t))
    }

    // ---- attributes (AttrIsId and friends)

    /// `AttrIsId(av, atid)`
    pub fn attr_is(&self, av: Option<AttId>, id: AttrId) -> bool {
        av.is_some_and(|a| self[a].dict.is_some_and(|d| d.id == id))
    }

    /// `av->dict->id` (UNKNOWN when there is no dict)
    pub fn attr_id(&self, av: AttId) -> AttrId {
        self[av].dict.map_or(AttrId::UNKNOWN, |d| d.id)
    }

    /// `AttrHasValue(attr)`
    pub fn attr_has_value(&self, av: Option<AttId>) -> bool {
        av.is_some_and(|a| self[a].value.is_some())
    }

    /// `AttrValueIs(attr, val)`: case-insensitive comparison
    pub fn attr_value_is(&self, av: Option<AttId>, val: &[u8]) -> bool {
        match av.and_then(|a| self[a].value.as_deref()) {
            Some(v) => crate::tmbstr::tmbstrcasecmp(v, val) == 0,
            None => false,
        }
    }

    /// `AttrContains(attr, val)`: case-insensitive substring
    pub fn attr_contains(&self, av: Option<AttId>, val: &[u8]) -> bool {
        match av.and_then(|a| self[a].value.as_deref()) {
            Some(v) => crate::tmbstr::tmbsubstr(v, val).is_some(),
            None => false,
        }
    }

    /// The attribute list of a node, in order (snapshot of the ids).
    pub fn attr_list(&self, node: NodeId) -> Vec<AttId> {
        let mut v = Vec::new();
        let mut a = self[node].attributes;
        while let Some(id) = a {
            v.push(id);
            a = self[id].next;
        }
        v
    }

    /// The text of a node: `lexbuf[start..end]`.
    pub fn node_text(&self, node: NodeId) -> &[u8] {
        let n = &self[node];
        let lb = &self.lxr().lexbuf;
        let s = (n.start as usize).min(lb.len());
        let e = (n.end as usize).min(lb.len()).max(s);
        &lb[s..e]
    }
}
