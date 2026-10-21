//! ext/xmlreader: the XMLReader class, a port of php-src's php_xmlreader.c
//! on top of a port of libxml2's xmlTextReader (xmlreader.c).
//!
//! As in libxml2, the document is fed in chunks to the push parser which
//! builds the tree (TreeBuilder); the reader walks the tree while it is
//! being built and unlinks the nodes it is done with. Parser errors thus
//! surface at the read() that makes the parser reach them.

use crate::dom::{self, Args};
use crate::libxml::{self, Style};
use crate::parser::{self, Parser, ParserState, PushState, SaxAttr, Sink, XmlError};
use crate::sax2::TreeBuilder;
use crate::tree::{self, NType, NodeId, NsId, Tree, XML_XML_NAMESPACE};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::mem::ManuallyDrop;
use std::rc::Rc;

type Obj = Rc<RefCell<PhpObject>>;

const CHUNK_SIZE: usize = 512;
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

// XMLReader node types
const T_NONE: i64 = 0;
const T_ELEMENT: i64 = 1;
const T_ATTRIBUTE: i64 = 2;
const T_TEXT: i64 = 3;
const T_CDATA: i64 = 4;
const T_ENTITY_REF: i64 = 5;
const T_ENTITY: i64 = 6;
const T_PI: i64 = 7;
const T_COMMENT: i64 = 8;
const T_DOC: i64 = 9;
const T_DOC_TYPE: i64 = 10;
const T_DOC_FRAGMENT: i64 = 11;
const T_NOTATION: i64 = 12;
const T_WHITESPACE: i64 = 13;
const T_SIGNIFICANT_WHITESPACE: i64 = 14;
const T_END_ELEMENT: i64 = 15;

// xmlParserProperties
const P_LOADDTD: i64 = 1;
const P_DEFAULTATTRS: i64 = 2;
const P_VALIDATE: i64 = 3;
const P_SUBST_ENTITIES: i64 = 4;

// ctxt->loadsubset bits
const XML_DETECT_IDS: u8 = 2;
const XML_COMPLETE_ATTRS: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Initial,
    Interactive,
    Error,
    Eof,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    None,
    Element,
    End,
    Backtrack,
    Done,
    Error,
}

/// reader->curnode: an attribute, or a namespace declaration (index in the
/// element's nsDef list).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cur {
    None,
    Attr(NodeId),
    Ns(usize),
}

// ---------------------------------------------------------------------------
// SAX sink: the SAX2 tree builder plus the reader's start-element hooks

struct Shared {
    tb: TreeBuilder,
    /// xmlTextReaderStartElement set reader->state = ELEMENT.
    elem_started: bool,
    /// Elements written as `<x/>` (NODE_IS_EMPTY).
    empty: HashSet<NodeId>,
}

struct ReaderSink {
    sh: Rc<RefCell<Shared>>,
}

impl Sink for ReaderSink {
    fn builds_tree(&self) -> bool {
        true
    }
    fn start_document(&mut self, version: &str, encoding: Option<&str>, standalone: i32) {
        self.sh
            .borrow_mut()
            .tb
            .start_document(version, encoding, standalone)
    }
    fn end_document(&mut self, encoding: Option<&str>) {
        self.sh.borrow_mut().tb.end_document(encoding)
    }
    fn internal_subset(&mut self, name: Option<&str>, ext_id: Option<&str>, sys_id: Option<&str>) {
        self.sh
            .borrow_mut()
            .tb
            .internal_subset(name, ext_id, sys_id)
    }
    fn external_subset(&mut self, name: Option<&str>, ext_id: Option<&str>, sys_id: Option<&str>) {
        self.sh
            .borrow_mut()
            .tb
            .external_subset(name, ext_id, sys_id)
    }
    fn entity_decl(
        &mut self,
        name: &str,
        kind: parser::EntKind,
        public: Option<&str>,
        system: Option<&str>,
        content: Option<&str>,
        orig: Option<&str>,
    ) {
        self.sh
            .borrow_mut()
            .tb
            .entity_decl(name, kind, public, system, content, orig)
    }
    fn unparsed_entity_decl(
        &mut self,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        notation: &str,
    ) {
        self.sh
            .borrow_mut()
            .tb
            .unparsed_entity_decl(name, public, system, notation)
    }
    fn notation_decl(&mut self, name: &str, public: Option<&str>, system: Option<&str>) {
        self.sh.borrow_mut().tb.notation_decl(name, public, system)
    }
    fn element_decl(&mut self, decl: &parser::ElementDecl) {
        self.sh.borrow_mut().tb.element_decl(decl)
    }
    fn attribute_decl(&mut self, decl: &parser::AttrDeclInfo) {
        self.sh.borrow_mut().tb.attribute_decl(decl)
    }
    fn start_element(
        &mut self,
        p: &mut ParserState,
        localname: &str,
        prefix: Option<&str>,
        uri: Option<&str>,
        nsdecls: &[(Option<String>, String)],
        attrs: &[SaxAttr],
        nb_defaulted: usize,
        line: u32,
    ) {
        let mut sh = self.sh.borrow_mut();
        let before = sh.tb.stack.len();
        sh.tb.start_element(
            p,
            localname,
            prefix,
            uri,
            nsdecls,
            attrs,
            nb_defaulted,
            line,
        );
        if sh.tb.stack.len() > before {
            if let Some(&n) = sh.tb.stack.last() {
                let empty = match p.inputs.last() {
                    Some(i) => {
                        i.data.get(i.pos) == Some(&b'/') && i.data.get(i.pos + 1) == Some(&b'>')
                    }
                    None => false,
                };
                if empty {
                    sh.empty.insert(n);
                }
            }
        }
        sh.elem_started = true;
    }
    fn end_element(
        &mut self,
        p: &mut ParserState,
        localname: &str,
        prefix: Option<&str>,
        uri: Option<&str>,
    ) {
        self.sh
            .borrow_mut()
            .tb
            .end_element(p, localname, prefix, uri)
    }
    fn characters(&mut self, p: &mut ParserState, text: &str) {
        self.sh.borrow_mut().tb.characters(p, text)
    }
    fn ignorable_whitespace(&mut self, p: &mut ParserState, text: &str) {
        self.sh.borrow_mut().tb.ignorable_whitespace(p, text)
    }
    fn cdata_block(&mut self, p: &mut ParserState, text: &str) {
        self.sh.borrow_mut().tb.cdata_block(p, text)
    }
    fn comment(&mut self, p: &mut ParserState, text: &str) {
        self.sh.borrow_mut().tb.comment(p, text)
    }
    fn pi(&mut self, p: &mut ParserState, target: &str, data: Option<&str>) {
        self.sh.borrow_mut().tb.pi(p, target, data)
    }
    fn reference(&mut self, p: &mut ParserState, name: &str) {
        self.sh.borrow_mut().tb.reference(p, name)
    }
    fn begin_entity_content(&mut self, name: &str) -> bool {
        self.sh.borrow_mut().tb.begin_entity_content(name)
    }
    fn end_entity_content(&mut self, name: &str) {
        self.sh.borrow_mut().tb.end_entity_content(name)
    }
    fn are_blanks_tree(&self, next_lt: bool, next_lt_slash: bool, elem_type: Option<u8>) -> bool {
        self.sh
            .borrow()
            .tb
            .are_blanks_tree(next_lt, next_lt_slash, elem_type)
    }
    fn current_element(&self) -> Option<(String, Option<String>)> {
        self.sh.borrow().tb.current_element()
    }
    fn error(&mut self, err: &XmlError) {
        self.sh.borrow_mut().tb.error(err)
    }
}

/// The push parser context (owns its leaked sink).
struct Engine {
    parser: ManuallyDrop<Parser<'static, ReaderSink>>,
    sink: *mut ReaderSink,
}

impl Engine {
    fn new(sh: Rc<RefCell<Shared>>, initial: Vec<u8>, url: Option<String>, options: i64) -> Engine {
        let sink: &'static mut ReaderSink = Box::leak(Box::new(ReaderSink { sh }));
        let ptr = sink as *mut ReaderSink;
        let p = Parser::new(sink, initial, url, options);
        Engine {
            parser: ManuallyDrop::new(p),
            sink: ptr,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the parser (the only user of the sink reference) is
        // dropped first; the sink was leaked from a Box in Engine::new.
        unsafe {
            ManuallyDrop::drop(&mut self.parser);
            drop(Box::from_raw(self.sink));
        }
    }
}

/// xmlParserInputBuffer read side.
enum Source {
    /// Everything is in the buffer already.
    Memory,
    /// A file read in full; handed out progressively.
    Data(Vec<u8>, usize),
    /// A PHP stream (fromStream), read with fread().
    Stream(Value),
    /// The read callback reported end of input or an error (endOfInput).
    Done,
}

// ---------------------------------------------------------------------------
// The text reader

struct Core {
    eng: Engine,
    sh: Rc<RefCell<Shared>>,
    /// The input buffer and the position up to which it was pushed.
    buf: Vec<u8>,
    cur: usize,
    src: Source,
    mode: Mode,
    state: St,
    node: Option<NodeId>,
    curnode: Cur,
    depth: i64,
    ents: Vec<NodeId>,
    /// reader->validate (DTD)
    validate: bool,
    /// ctxt->validate
    ctxt_validate: bool,
    loadsubset: u8,
    /// Nodes the reader unlinked, with their parent and previous sibling,
    /// while DTD validation is on (relinked to validate the whole document).
    freed: Vec<(NodeId, Option<NodeId>, Option<NodeId>)>,
    validated: bool,
    errors: Vec<XmlError>,
}

impl Core {
    fn new(
        src: Source,
        mut buf: Vec<u8>,
        url: Option<String>,
        encoding: Option<&str>,
        options: i64,
        vm: &mut Vm,
    ) -> Core {
        let options = options | parser::XML_PARSE_COMPACT;
        let mut tb = TreeBuilder::new(options);
        tb.url = url.clone();
        let sh = Rc::new(RefCell::new(Shared {
            tb,
            elem_started: false,
            empty: HashSet::new(),
        }));
        let mut src = src;
        // xmlTextReaderSetup: make sure 4 bytes are available
        if buf.len() < 4 {
            let mut tmp = Core::read_source(vm, &mut src, 4);
            if let Ok(d) = &mut tmp {
                buf.append(d);
            }
        }
        let (initial, cur) = if buf.len() >= 4 {
            (buf[..4].to_vec(), 4)
        } else {
            (Vec::new(), 0)
        };
        let mut eng = Engine::new(sh.clone(), initial, url, options);
        let mut loadsubset = 0;
        if options & parser::XML_PARSE_DTDLOAD != 0 {
            loadsubset |= XML_DETECT_IDS;
        }
        if options & parser::XML_PARSE_DTDATTR != 0 {
            loadsubset |= XML_COMPLETE_ATTRS;
        }
        if let Some(e) = encoding {
            eng.parser.switch_encoding(e);
        }
        let validate = options & parser::XML_PARSE_DTDVALID != 0;
        Core {
            eng,
            sh,
            buf,
            cur,
            src,
            mode: Mode::Initial,
            state: St::None,
            node: None,
            curnode: Cur::None,
            depth: 0,
            ents: Vec::new(),
            validate,
            ctxt_validate: validate,
            loadsubset,
            freed: Vec::new(),
            validated: false,
            errors: Vec::new(),
        }
    }

    /// The read callback: Ok(bytes) (empty at the end), Err on I/O error.
    fn read_source(vm: &mut Vm, src: &mut Source, len: usize) -> Result<Vec<u8>, ()> {
        let r = match src {
            Source::Memory | Source::Done => return Ok(Vec::new()),
            Source::Data(d, pos) => {
                let n = len.min(d.len() - *pos);
                let v = d[*pos..*pos + n].to_vec();
                *pos += n;
                Ok(v)
            }
            Source::Stream(h) => {
                let h = h.clone();
                stream_read(vm, &h, len)
            }
        };
        if !matches!(r, Ok(ref v) if !v.is_empty()) {
            *src = Source::Done;
        }
        r
    }

    fn input_read(&mut self, vm: &mut Vm, len: usize) -> i64 {
        match Core::read_source(vm, &mut self.src, len) {
            Ok(v) => {
                let n = v.len() as i64;
                self.buf.extend_from_slice(&v);
                n
            }
            Err(()) => -1,
        }
    }

    fn st(&mut self) -> &mut ParserState {
        &mut self.eng.parser.st
    }

    fn well_formed(&self) -> bool {
        self.eng.parser.st.well_formed
    }

    /// xmlParseChunk on buf[from..to]
    fn parse(&mut self, from: usize, to: usize, terminate: bool) -> i32 {
        let chunk = self.buf[from..to].to_vec();
        let r = self.eng.parser.parse_chunk(&chunk, terminate);
        let errs = std::mem::take(&mut self.eng.parser.st.errors);
        self.errors.extend(errs);
        if terminate {
            self.finish_validation();
        }
        r
    }

    /// DTD validation of the complete document (the walked nodes were kept).
    fn finish_validation(&mut self) {
        if !self.validate || !self.ctxt_validate || self.validated {
            return;
        }
        self.validated = true;
        if !self.well_formed() {
            return;
        }
        let doc = self.sh.borrow().tb.doc;
        let freed = std::mem::take(&mut self.freed);
        if let Some(d) = doc {
            // relink what the reader already released, validate, unlink again
            tree::with(|t| {
                for &(n, parent, prev) in freed.iter().rev() {
                    match (prev, parent) {
                        (Some(p), _) => t.insert_after_raw(p, n),
                        (None, Some(par)) => match t.nodes[par].children {
                            Some(first) => t.insert_before_raw(first, n),
                            None => t.link_child_raw(par, n),
                        },
                        (None, None) => {}
                    }
                }
            });
            let errs = crate::valid::validate_document(d);
            tree::with(|t| {
                for &(n, _, _) in freed.iter() {
                    t.unlink(n);
                }
            });
            if errs.iter().any(|e| e.level >= parser::ERR_ERROR) {
                self.st().valid = false;
            }
            self.errors.extend(errs);
        }
    }

    fn ctxt_node(&self) -> Option<NodeId> {
        self.sh.borrow().tb.stack.last().copied()
    }

    fn node_nr(&self) -> usize {
        self.sh.borrow().tb.stack.len()
    }

    fn my_doc(&self) -> Option<NodeId> {
        self.sh.borrow().tb.doc
    }

    fn stopped(&self) -> bool {
        let st = &self.eng.parser.st;
        st.stop || st.disable_sax > 1
    }

    fn is_empty_node(&self, n: NodeId) -> bool {
        self.sh.borrow().empty.contains(&n)
    }

    fn io_error(&mut self) {
        if let Some(e) = self.st().report(
            parser::FROM_IO,
            1500,
            parser::ERR_FATAL,
            "Unknown IO error\n".into(),
        ) {
            let _ = e;
        }
        let errs = std::mem::take(&mut self.eng.parser.st.errors);
        self.errors.extend(errs);
    }

    /// xmlTextReaderPushData
    fn push_data(&mut self, vm: &mut Vm) -> i32 {
        let oldstate = self.state;
        self.state = St::None;
        self.sh.borrow_mut().elem_started = false;
        while self.state == St::None {
            if self.buf.len() < self.cur + CHUNK_SIZE {
                if self.mode != Mode::Eof {
                    let val = self.input_read(vm, 4096);
                    if val == 0 {
                        if self.buf.len() == self.cur {
                            self.mode = Mode::Eof;
                            break;
                        }
                    } else if val < 0 {
                        self.io_error();
                        self.mode = Mode::Error;
                        self.state = St::Error;
                        return -1;
                    }
                } else {
                    break;
                }
            }
            if self.buf.len() >= self.cur + CHUNK_SIZE {
                let val = self.parse(self.cur, self.cur + CHUNK_SIZE, false);
                self.cur += CHUNK_SIZE;
                if val != 0 {
                    self.st().well_formed = false;
                }
                if !self.well_formed() {
                    break;
                }
            } else {
                let end = self.buf.len();
                let val = self.parse(self.cur, end, false);
                self.cur = end;
                if val != 0 {
                    self.st().well_formed = false;
                }
                break;
            }
            if self.sh.borrow().elem_started {
                self.state = St::Element;
            }
        }
        self.state = oldstate;
        // Discard the consumed input when needed and possible
        if self.mode == Mode::Interactive {
            if !matches!(self.src, Source::Memory)
                && self.cur >= 4096
                && self.buf.len() - self.cur <= CHUNK_SIZE
            {
                self.buf.drain(..self.cur);
                self.cur = 0;
            }
        } else if self.mode == Mode::Eof && self.state != St::Done {
            let end = self.buf.len();
            let val = self.parse(self.cur, end, true);
            self.cur = end;
            self.state = St::Done;
            if val != 0 {
                if self.well_formed() {
                    self.st().well_formed = false;
                } else {
                    return -1;
                }
            }
        }
        if !self.well_formed() {
            self.mode = Mode::Eof;
            return -1;
        }
        0
    }

    /// Unlink a node the reader is done with (xmlTextReaderFreeNode).
    fn free_node(&mut self, n: NodeId) {
        let validating = self.validate && !self.validated;
        let mut empty = self.sh.borrow_mut();
        tree::with(|t| {
            if validating {
                self.freed.push((n, t.nodes[n].parent, t.nodes[n].prev));
                t.unlink(n);
                return;
            }
            t.unlink(n);
            // Nothing refers to the released subtree any more: give its
            // slots back (xmlTextReaderFreeNode).
            let mut stack = vec![n];
            while let Some(x) = stack.pop() {
                let ty = t.nodes[x].t;
                if matches!(ty, NType::Dtd | NType::EntityDecl | NType::Entity) {
                    continue;
                }
                empty.empty.remove(&x);
                if ty != NType::EntityRef {
                    let mut c = t.nodes[x].children;
                    let mut p = t.nodes[x].properties;
                    while let Some(y) = c {
                        stack.push(y);
                        c = t.nodes[y].next;
                    }
                    while let Some(y) = p {
                        stack.push(y);
                        p = t.nodes[y].next;
                    }
                }
                if ty == NType::Attribute && t.nodes[x].atype == 2 {
                    // the value text is still there (children go after)
                    let key = t.node_get_content(x);
                    if let Some(d) = t.nodes[x].doc {
                        if let Some(di) = t.doc_info_mut(d) {
                            if di.ids.get(&key) == Some(&x) {
                                di.ids.remove(&key);
                            }
                        }
                    }
                }
                t.release(x);
            }
        });
    }

    /// xmlTextReaderRead
    fn read(&mut self, vm: &mut Vm) -> i32 {
        if self.state == St::Error {
            return -1;
        }
        self.curnode = Cur::None;
        if self.mode == Mode::Initial {
            self.mode = Mode::Interactive;
            loop {
                let val = self.push_data(vm);
                if val < 0 {
                    self.mode = Mode::Error;
                    self.state = St::Error;
                    return -1;
                }
                if !(self.ctxt_node().is_none() && self.mode != Mode::Eof && self.state != St::Done)
                {
                    break;
                }
            }
            let doc_first = self
                .my_doc()
                .and_then(|d| tree::with(|t| t.nodes[d].children));
            if self.ctxt_node().is_none() {
                if self.my_doc().is_some() {
                    self.node = doc_first;
                }
                if self.node.is_none() {
                    self.mode = Mode::Error;
                    self.state = St::Error;
                    return -1;
                }
                self.state = St::Element;
            } else {
                if self.my_doc().is_some() {
                    self.node = doc_first;
                }
                if self.node.is_none() {
                    self.node = self.sh.borrow().tb.stack.first().copied();
                }
                self.state = St::Element;
            }
            self.depth = 0;
            return self.node_found(vm);
        }
        let oldstate = self.state;
        let olddepth = self.node_nr();
        let mut oldnode = self.node;
        self.get_next_node(vm, oldstate, olddepth, &mut oldnode)
    }

    fn get_next_node(
        &mut self,
        vm: &mut Vm,
        oldstate: St,
        olddepth: usize,
        oldnode: &mut Option<NodeId>,
    ) -> i32 {
        if self.node.is_none() {
            if self.mode == Mode::Eof {
                return 0;
            }
            self.mode = Mode::Error;
            self.state = St::Error;
            return -1;
        }
        while let Some(n) = self.node {
            let cond = tree::with(|t| {
                let nd = &t.nodes[n];
                if nd.next.is_some() {
                    return false;
                }
                oldstate == St::Backtrack
                    || nd.children.is_none()
                    || nd.t == NType::EntityRef
                    || (nd.children.is_some()
                        && t.nodes[nd.children.unwrap()].t == NType::Text
                        && t.nodes[nd.children.unwrap()].next.is_none())
                    || matches!(nd.t, NType::Dtd | NType::Document | NType::HtmlDocument)
            });
            if !cond || self.node_nr() != olddepth {
                break;
            }
            let cn = self.ctxt_node();
            let parent = tree::with(|t| t.nodes[n].parent);
            if !(cn.is_none() || cn == Some(n) || cn == parent) {
                break;
            }
            if self.eng.parser.push_state() == PushState::Eof || self.stopped() {
                break;
            }
            let val = self.push_data(vm);
            if val < 0 {
                self.mode = Mode::Error;
                self.state = St::Error;
                return -1;
            }
            if self.node.is_none() {
                self.state = St::Done;
                return 0;
            }
        }
        let n = self.node.unwrap();
        let (ntype, children, next, parent) = tree::with(|t| {
            let nd = &t.nodes[n];
            (nd.t, nd.children, nd.next, nd.parent)
        });
        if oldstate != St::Backtrack {
            if let Some(c) = children {
                if ntype != NType::EntityRef && ntype != NType::Dtd {
                    self.node = Some(c);
                    self.depth += 1;
                    self.state = St::Element;
                    return self.node_found(vm);
                }
            }
        }
        if let Some(nx) = next {
            if oldstate == St::Element
                && ntype == NType::Element
                && children.is_none()
                && !self.is_empty_node(n)
            {
                self.state = St::End;
                return self.node_found(vm);
            }
            self.node = Some(nx);
            self.state = St::Element;
            // Cleanup of the old node
            if self.ents.is_empty() {
                let prev = tree::with(|t| t.nodes[nx].prev);
                if let Some(p) = prev {
                    if tree::with(|t| t.nodes[p].t) != NType::Dtd {
                        if *oldnode == Some(p) {
                            *oldnode = None;
                        }
                        self.free_node(p);
                    }
                }
            }
            return self.node_found(vm);
        }
        if oldstate == St::Element
            && ntype == NType::Element
            && children.is_none()
            && !self.is_empty_node(n)
        {
            self.state = St::End;
            return self.node_found(vm);
        }
        self.node = parent;
        let ptype = parent.map(|p| tree::with(|t| t.nodes[p].t));
        if parent.is_none() || matches!(ptype, Some(NType::Document) | Some(NType::HtmlDocument)) {
            if self.mode != Mode::Eof {
                let end = self.buf.len();
                let val = self.parse(end, end, true);
                self.state = St::Done;
                if val != 0 {
                    self.mode = Mode::Error;
                    self.state = St::Error;
                    return -1;
                }
            }
            self.node = None;
            self.depth = -1;
            // Cleanup of the old node
            if let Some(o) = *oldnode {
                if self.ents.is_empty() && tree::with(|t| t.nodes[o].t) != NType::Dtd {
                    self.free_node(o);
                }
            }
            self.state = St::Done;
            return 0;
        }
        let p = parent.unwrap();
        if self.ents.is_empty() {
            if let Some(last) = tree::with(|t| t.nodes[p].last) {
                self.free_node(last);
            }
        }
        self.depth -= 1;
        self.state = St::Backtrack;
        self.node_found(vm)
    }

    fn node_found(&mut self, vm: &mut Vm) -> i32 {
        // If we are in the middle of a piece of CDATA make sure it's finished
        if let Some(n) = self.node {
            let (t_, next) = tree::with(|t| (t.nodes[n].t, t.nodes[n].next));
            if next.is_none()
                && matches!(t_, NType::Text | NType::CData)
                && self.expand(vm).is_none()
            {
                return -1;
            }
        }
        // Handle entities enter and exit when in entity replacement mode
        if let Some(n) = self.node {
            let (t_, children) = tree::with(|t| (t.nodes[n].t, t.nodes[n].children));
            if t_ == NType::EntityRef && self.eng.parser.st.replace_entities {
                if let Some(ent) = children {
                    let (et, ec) = tree::with(|t| (t.nodes[ent].t, t.nodes[ent].children));
                    if et == NType::EntityDecl {
                        if let Some(first) = ec {
                            self.ents.push(n);
                            self.node = Some(first);
                        }
                    }
                }
            }
        }
        if let Some(n) = self.node {
            let t_ = tree::with(|t| t.nodes[n].t);
            if t_ == NType::EntityDecl {
                if let Some(&ent) = self.ents.last() {
                    if tree::with(|t| t.nodes[ent].children) == Some(n) {
                        self.node = self.ents.pop();
                        self.depth += 1;
                        let oldstate = self.state;
                        let olddepth = self.node_nr();
                        let mut oldnode = self.node;
                        return self.get_next_node(vm, oldstate, olddepth, &mut oldnode);
                    }
                }
            }
        }
        1
    }

    fn successor(t: &Tree, mut cur: NodeId) -> Option<NodeId> {
        if let Some(n) = t.nodes[cur].next {
            return Some(n);
        }
        loop {
            cur = t.nodes[cur].parent?;
            if let Some(n) = t.nodes[cur].next {
                return Some(n);
            }
        }
    }

    /// xmlTextReaderDoExpand
    fn do_expand(&mut self, vm: &mut Vm) -> i32 {
        let node = match self.node {
            Some(n) => n,
            None => return -1,
        };
        loop {
            if self.stopped() {
                return 1;
            }
            if tree::with(|t| Core::successor(t, node)).is_some() {
                return 1;
            }
            if (self.node_nr() as i64) < self.depth {
                return 1;
            }
            if self.mode == Mode::Eof {
                return 1;
            }
            let val = self.push_data(vm);
            if val < 0 {
                self.mode = Mode::Error;
                self.state = St::Error;
                return -1;
            }
            if self.mode == Mode::Eof {
                return 1;
            }
        }
    }

    /// xmlTextReaderExpand
    fn expand(&mut self, vm: &mut Vm) -> Option<NodeId> {
        self.node?;
        if self.do_expand(vm) < 0 {
            return None;
        }
        self.node
    }

    /// xmlTextReaderNext
    fn next(&mut self, vm: &mut Vm) -> i32 {
        let cur = match self.node {
            Some(n) if tree::with(|t| t.nodes[n].t) == NType::Element => n,
            _ => return self.read(vm),
        };
        if self.state == St::End || self.state == St::Backtrack {
            return self.read(vm);
        }
        if self.is_empty_node(cur) {
            return self.read(vm);
        }
        loop {
            let ret = self.read(vm);
            if ret != 1 {
                return ret;
            }
            if self.node == Some(cur) {
                break;
            }
        }
        self.read(vm)
    }

    // ------------------------------------------------------------------
    // Accessors

    /// The node the accessors look at: the attribute cursor or the node.
    fn cur_target(&self) -> Option<Target> {
        let n = self.node?;
        Some(match self.curnode {
            Cur::None => Target::Node(n),
            Cur::Attr(a) => Target::Node(a),
            Cur::Ns(i) => Target::Ns(tree::with(|t| t.nodes[n].ns_def[i])),
        })
    }

    fn attribute_count(&self) -> i64 {
        let n = match self.cur_target() {
            Some(Target::Node(n)) => n,
            _ => return 0,
        };
        tree::with(|t| {
            if t.nodes[n].t != NType::Element {
                return 0;
            }
            if self.state == St::End || self.state == St::Backtrack {
                return 0;
            }
            (t.attrs_of(n).len() + t.nodes[n].ns_def.len()) as i64
        })
    }

    fn node_type(&self) -> i64 {
        let tg = match self.cur_target() {
            Some(t) => t,
            None => return T_NONE,
        };
        let n = match tg {
            Target::Ns(_) => return T_ATTRIBUTE,
            Target::Node(n) => n,
        };
        let node = self.node.unwrap();
        tree::with(|t| match t.nodes[n].t {
            NType::Element => {
                if self.state == St::End || self.state == St::Backtrack {
                    T_END_ELEMENT
                } else {
                    T_ELEMENT
                }
            }
            NType::NamespaceDecl | NType::Attribute => T_ATTRIBUTE,
            NType::Text => {
                if is_blank_node(t, node) {
                    if space_preserve(t, node) != 0 {
                        T_SIGNIFICANT_WHITESPACE
                    } else {
                        T_WHITESPACE
                    }
                } else {
                    T_TEXT
                }
            }
            NType::CData => T_CDATA,
            NType::EntityRef => T_ENTITY_REF,
            NType::Entity => T_ENTITY,
            NType::Pi => T_PI,
            NType::Comment => T_COMMENT,
            NType::Document | NType::HtmlDocument => T_DOC,
            NType::DocumentFrag => T_DOC_FRAGMENT,
            NType::Notation => T_NOTATION,
            NType::DocumentType | NType::Dtd => T_DOC_TYPE,
            NType::ElementDecl | NType::AttributeDecl | NType::EntityDecl => T_NONE,
        })
    }

    fn is_empty_element(&self) -> i64 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        let (t_, children) = tree::with(|t| (t.nodes[n].t, t.nodes[n].children));
        if t_ != NType::Element
            || self.curnode != Cur::None
            || children.is_some()
            || self.state == St::End
        {
            return 0;
        }
        self.is_empty_node(n) as i64
    }

    fn local_name(&self) -> Option<String> {
        match self.cur_target()? {
            Target::Ns(ns) => {
                Some(tree::with(|t| t.nss[ns].prefix.clone()).unwrap_or_else(|| "xmlns".into()))
            }
            Target::Node(n) => {
                let (t_, name) = tree::with(|t| (t.nodes[n].t, t.nodes[n].name.clone()));
                if t_ != NType::Element && t_ != NType::Attribute {
                    return self.name();
                }
                Some(name)
            }
        }
    }

    fn name(&self) -> Option<String> {
        match self.cur_target()? {
            Target::Ns(ns) => Some(match tree::with(|t| t.nss[ns].prefix.clone()) {
                None => "xmlns".into(),
                Some(p) => format!("xmlns:{}", p),
            }),
            Target::Node(n) => tree::with(|t| {
                let nd = &t.nodes[n];
                Some(match nd.t {
                    NType::Element | NType::Attribute => match t.ns_prefix(nd.ns) {
                        Some(p) => format!("{}:{}", p, nd.name),
                        None => nd.name.clone(),
                    },
                    NType::Text => "#text".into(),
                    NType::CData => "#cdata-section".into(),
                    NType::Entity
                    | NType::EntityRef
                    | NType::Pi
                    | NType::Notation
                    | NType::DocumentType
                    | NType::Dtd => nd.name.clone(),
                    NType::Comment => "#comment".into(),
                    NType::Document | NType::HtmlDocument => "#document".into(),
                    NType::DocumentFrag => "#document-fragment".into(),
                    _ => return None,
                })
            }),
        }
    }

    fn prefix(&self) -> Option<String> {
        match self.cur_target()? {
            Target::Ns(ns) => {
                tree::with(|t| t.nss[ns].prefix.as_ref().map(|_| "xmlns".to_string()))
            }
            Target::Node(n) => tree::with(|t| {
                let nd = &t.nodes[n];
                if nd.t != NType::Element && nd.t != NType::Attribute {
                    return None;
                }
                t.ns_prefix(nd.ns).map(|s| s.to_string())
            }),
        }
    }

    fn namespace_uri(&self) -> Option<String> {
        match self.cur_target()? {
            Target::Ns(_) => Some(XMLNS_NS.into()),
            Target::Node(n) => tree::with(|t| {
                let nd = &t.nodes[n];
                if nd.t != NType::Element && nd.t != NType::Attribute {
                    return None;
                }
                t.ns_href(nd.ns).map(|s| s.to_string())
            }),
        }
    }

    fn base_uri(&self) -> Option<String> {
        let n = self.node?;
        tree::with(|t| dom::node::node_get_base(t, n))
    }

    fn depth(&self) -> i64 {
        if self.node.is_none() {
            return 0;
        }
        match self.curnode {
            Cur::None => self.depth,
            _ => self.depth + 1,
        }
    }

    fn has_attributes(&self) -> bool {
        match self.cur_target() {
            Some(Target::Node(n)) => tree::with(|t| {
                t.nodes[n].t == NType::Element
                    && (t.nodes[n].properties.is_some() || !t.nodes[n].ns_def.is_empty())
            }),
            _ => false,
        }
    }

    fn has_value(&self) -> bool {
        match self.cur_target() {
            None => false,
            Some(Target::Ns(_)) => true,
            Some(Target::Node(n)) => tree::with(|t| {
                matches!(
                    t.nodes[n].t,
                    NType::Attribute | NType::Text | NType::CData | NType::Pi | NType::Comment
                )
            }),
        }
    }

    fn value(&self) -> Option<String> {
        match self.cur_target()? {
            Target::Ns(ns) => Some(tree::with(|t| t.nss[ns].href.clone())),
            Target::Node(n) => tree::with(|t| {
                let nd = &t.nodes[n];
                match nd.t {
                    NType::Attribute => {
                        let c = nd.children;
                        if let Some(c) = c {
                            if t.nodes[c].t == NType::Text && t.nodes[c].next.is_none() {
                                return Some(t.nodes[c].content.clone());
                            }
                        }
                        Some(t.node_get_content(n))
                    }
                    NType::Text | NType::CData | NType::Pi | NType::Comment => {
                        Some(nd.content.clone())
                    }
                    _ => None,
                }
            }),
        }
    }

    fn xml_lang(&self) -> Option<String> {
        let n = self.node?;
        tree::with(|t| {
            let mut cur = Some(n);
            while let Some(c) = cur {
                if t.nodes[c].t == NType::Element {
                    for a in t.attrs_of(c) {
                        if t.nodes[a].name == "lang"
                            && t.ns_href(t.nodes[a].ns) == Some(XML_XML_NAMESPACE)
                        {
                            return Some(t.node_get_content(a));
                        }
                    }
                }
                cur = t.nodes[c].parent;
            }
            None
        })
    }

    /// The element whose attributes the get/move methods look at.
    fn element(&self) -> Option<NodeId> {
        let n = self.node?;
        if tree::with(|t| t.nodes[n].t) != NType::Element {
            return None;
        }
        Some(n)
    }

    fn get_attribute_no(&self, no: i64) -> Option<String> {
        // C int argument; negative indexes behave like 0
        let no = (no as i32).max(0) as i64;
        if self.curnode != Cur::None {
            return None;
        }
        let n = self.element()?;
        tree::with(|t| {
            let nsd = &t.nodes[n].ns_def;
            if no >= 0 && (no as usize) < nsd.len() {
                return Some(t.nss[nsd[no as usize]].href.clone());
            }
            let i = no.max(nsd.len() as i64) - nsd.len() as i64;
            let a = *t.attrs_of(n).get(i as usize)?;
            let c = t.nodes[a].children?;
            Some(t.list_get_string(Some(c), true))
        })
    }

    fn get_attribute(&self, name: &str) -> Option<String> {
        if self.curnode != Cur::None {
            return None;
        }
        let n = self.element()?;
        let (prefix, local) = split_qname(name);
        tree::with(|t| match prefix {
            None => {
                if name == "xmlns" {
                    return t.nodes[n]
                        .ns_def
                        .iter()
                        .find(|&&ns| t.nss[ns].prefix.is_none())
                        .map(|&ns| t.nss[ns].href.clone());
                }
                attr_value(t, n, name, None)
            }
            Some("xmlns") => t.nodes[n]
                .ns_def
                .iter()
                .find(|&&ns| t.nss[ns].prefix.as_deref() == Some(local))
                .map(|&ns| t.nss[ns].href.clone()),
            Some(p) => {
                let ns = t.search_ns(n, Some(p))?;
                let href = t.nss[ns].href.clone();
                attr_value(t, n, local, Some(&href))
            }
        })
    }

    fn get_attribute_ns(&self, local: &str, uri: &str) -> Option<String> {
        if self.curnode != Cur::None {
            return None;
        }
        let n = self.element()?;
        tree::with(|t| {
            if uri == XMLNS_NS {
                let prefix = if local == "xmlns" { None } else { Some(local) };
                return t.nodes[n]
                    .ns_def
                    .iter()
                    .find(|&&ns| {
                        (prefix.is_none() && t.nss[ns].prefix.is_none())
                            || t.nss[ns].prefix.as_deref() == Some(local)
                    })
                    .map(|&ns| t.nss[ns].href.clone());
            }
            attr_value(t, n, local, Some(uri))
        })
    }

    fn lookup_namespace(&self, prefix: &str) -> Option<String> {
        let n = self.node?;
        tree::with(|t| {
            let ns = t.search_ns(n, Some(prefix))?;
            Some(t.nss[ns].href.clone())
        })
    }

    fn move_to_attribute_no(&mut self, no: i64) -> i32 {
        let no = (no as i32).max(0) as i64;
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        if tree::with(|t| t.nodes[n].t) != NType::Element {
            return -1;
        }
        self.curnode = Cur::None;
        let r = tree::with(|t| {
            let nsl = t.nodes[n].ns_def.len();
            if no >= 0 && (no as usize) < nsl {
                return Some(Cur::Ns(no as usize));
            }
            let i = no.max(nsl as i64) - nsl as i64;
            t.attrs_of(n).get(i as usize).map(|&a| Cur::Attr(a))
        });
        match r {
            Some(c) => {
                self.curnode = c;
                1
            }
            None => 0,
        }
    }

    fn move_to_attribute(&mut self, name: &str) -> i32 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        if tree::with(|t| t.nodes[n].t) != NType::Element {
            return 0;
        }
        let (prefix, local) = split_qname(name);
        let r = tree::with(|t| {
            let nsd = &t.nodes[n].ns_def;
            match prefix {
                None => {
                    if name == "xmlns" {
                        return nsd
                            .iter()
                            .position(|&ns| t.nss[ns].prefix.is_none())
                            .map(Cur::Ns);
                    }
                    t.attrs_of(n)
                        .into_iter()
                        .find(|&a| t.nodes[a].name == name && t.ns_prefix(t.nodes[a].ns).is_none())
                        .map(Cur::Attr)
                }
                Some("xmlns") => nsd
                    .iter()
                    .position(|&ns| t.nss[ns].prefix.as_deref() == Some(local))
                    .map(Cur::Ns),
                Some(p) => t
                    .attrs_of(n)
                    .into_iter()
                    .find(|&a| t.nodes[a].name == local && t.ns_prefix(t.nodes[a].ns) == Some(p))
                    .map(Cur::Attr),
            }
        });
        match r {
            Some(c) => {
                self.curnode = c;
                1
            }
            None => 0,
        }
    }

    fn move_to_attribute_ns(&mut self, local: &str, uri: &str) -> i32 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        if tree::with(|t| t.nodes[n].t) != NType::Element {
            return 0;
        }
        let r = tree::with(|t| {
            if uri == XMLNS_NS {
                let prefix = if local == "xmlns" { None } else { Some(local) };
                return t.nodes[n]
                    .ns_def
                    .iter()
                    .position(|&ns| {
                        (prefix.is_none() && t.nss[ns].prefix.is_none())
                            || t.nss[ns].prefix.as_deref() == Some(local)
                    })
                    .map(Cur::Ns);
            }
            t.attrs_of(n)
                .into_iter()
                .find(|&a| t.nodes[a].name == local && t.ns_href(t.nodes[a].ns) == Some(uri))
                .map(Cur::Attr)
        });
        match r {
            Some(c) => {
                self.curnode = c;
                1
            }
            None => 0,
        }
    }

    fn move_to_first_attribute(&mut self) -> i32 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        let (t_, nsl, props) =
            tree::with(|t| (t.nodes[n].t, t.nodes[n].ns_def.len(), t.nodes[n].properties));
        if t_ != NType::Element {
            return 0;
        }
        if nsl > 0 {
            self.curnode = Cur::Ns(0);
            return 1;
        }
        if let Some(p) = props {
            self.curnode = Cur::Attr(p);
            return 1;
        }
        0
    }

    fn move_to_next_attribute(&mut self) -> i32 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        let (t_, nsl, props) =
            tree::with(|t| (t.nodes[n].t, t.nodes[n].ns_def.len(), t.nodes[n].properties));
        if t_ != NType::Element {
            return 0;
        }
        match self.curnode {
            Cur::None => self.move_to_first_attribute(),
            Cur::Ns(i) => {
                if i + 1 < nsl {
                    self.curnode = Cur::Ns(i + 1);
                    return 1;
                }
                if let Some(p) = props {
                    self.curnode = Cur::Attr(p);
                    return 1;
                }
                0
            }
            Cur::Attr(a) => match tree::with(|t| t.nodes[a].next) {
                Some(nx) => {
                    self.curnode = Cur::Attr(nx);
                    1
                }
                None => 0,
            },
        }
    }

    fn move_to_element(&mut self) -> i32 {
        let n = match self.node {
            Some(n) => n,
            None => return -1,
        };
        if tree::with(|t| t.nodes[n].t) != NType::Element {
            return 0;
        }
        if self.curnode != Cur::None {
            self.curnode = Cur::None;
            return 1;
        }
        0
    }

    /// xmlTextReaderDumpCopy
    fn dump_copy(t: &mut Tree, out: &mut Vec<u8>, n: NodeId) {
        match t.nodes[n].t {
            NType::Dtd | NType::ElementDecl | NType::AttributeDecl | NType::EntityDecl => {}
            NType::Document | NType::HtmlDocument => {
                out.extend(crate::serialize::node_to_bytes(t, n, None, false, false));
            }
            _ => {
                let doc = t.nodes[n].doc;
                let copy = t.copy_node(n, doc, 1, None);
                out.extend(crate::serialize::node_to_bytes(t, copy, None, false, false));
            }
        }
    }

    fn read_inner_xml(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        self.expand(vm)?;
        let n = self.node?;
        Some(tree::with(|t| {
            let mut out = Vec::new();
            let mut c = if t.nodes[n].t == NType::EntityRef {
                None
            } else {
                t.nodes[n].children
            };
            while let Some(x) = c {
                Core::dump_copy(t, &mut out, x);
                c = t.nodes[x].next;
            }
            out
        }))
    }

    fn read_outer_xml(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        self.expand(vm)?;
        let n = self.node?;
        Some(tree::with(|t| {
            let mut out = Vec::new();
            Core::dump_copy(t, &mut out, n);
            out
        }))
    }

    fn read_string(&mut self, vm: &mut Vm) -> Option<String> {
        self.node?;
        let node = match self.curnode {
            Cur::Attr(a) => a,
            Cur::Ns(_) => return None,
            Cur::None => self.node.unwrap(),
        };
        match tree::with(|t| t.nodes[node].t) {
            NType::Text | NType::CData => {}
            NType::Element => {
                if self.do_expand(vm) == -1 || tree::with(|t| t.nodes[node].children).is_none() {
                    return None;
                }
            }
            _ => return None,
        }
        Some(tree::with(|t| {
            let mut s = String::new();
            let mut cur = node;
            loop {
                match t.nodes[cur].t {
                    NType::Text | NType::CData => s.push_str(&t.nodes[cur].content),
                    NType::Element => {
                        if let Some(c) = t.nodes[cur].children {
                            cur = c;
                            continue;
                        }
                    }
                    _ => {}
                }
                if cur == node {
                    break;
                }
                let mut done = false;
                while t.nodes[cur].next.is_none() {
                    match t.nodes[cur].parent {
                        Some(p) => cur = p,
                        None => {
                            done = true;
                            break;
                        }
                    }
                    if cur == node {
                        done = true;
                        break;
                    }
                }
                if done {
                    break;
                }
                cur = t.nodes[cur].next.unwrap();
            }
            s
        }))
    }

    fn set_parser_prop(&mut self, prop: i64, value: bool) -> i32 {
        let opt = |c: &mut Core, flag: i64, on: bool| {
            let st = &mut c.eng.parser.st;
            if on {
                st.options |= flag;
            } else {
                st.options &= !flag;
            }
            let o = st.options;
            c.sh.borrow_mut().tb.options = o;
        };
        match prop {
            P_LOADDTD => {
                if value {
                    if self.loadsubset == 0 {
                        if self.mode != Mode::Initial {
                            return -1;
                        }
                        opt(self, parser::XML_PARSE_DTDLOAD, true);
                        self.loadsubset |= XML_DETECT_IDS;
                    }
                } else {
                    opt(self, parser::XML_PARSE_DTDLOAD, false);
                    self.loadsubset &= !XML_DETECT_IDS;
                }
                0
            }
            P_DEFAULTATTRS => {
                opt(self, parser::XML_PARSE_DTDATTR, value);
                if value {
                    self.loadsubset |= XML_COMPLETE_ATTRS;
                } else {
                    self.loadsubset &= !XML_COMPLETE_ATTRS;
                }
                0
            }
            P_VALIDATE => {
                opt(self, parser::XML_PARSE_DTDVALID, value);
                self.ctxt_validate = value;
                if value {
                    self.validate = true;
                }
                0
            }
            P_SUBST_ENTITIES => {
                opt(self, parser::XML_PARSE_NOENT, value);
                self.eng.parser.st.replace_entities = value;
                0
            }
            _ => -1,
        }
    }

    fn get_parser_prop(&self, prop: i64) -> i32 {
        match prop {
            P_LOADDTD => (self.loadsubset != 0 || self.ctxt_validate) as i32,
            P_DEFAULTATTRS => (self.loadsubset & XML_COMPLETE_ATTRS != 0) as i32,
            P_VALIDATE => self.validate as i32,
            P_SUBST_ENTITIES => self.eng.parser.st.replace_entities as i32,
            _ => -1,
        }
    }

    fn is_valid(&self) -> bool {
        if self.ctxt_validate {
            return self.eng.parser.st.valid;
        }
        false
    }
}

#[derive(Clone, Copy)]
enum Target {
    Node(NodeId),
    Ns(NsId),
}

fn split_qname(name: &str) -> (Option<&str>, &str) {
    match name.find(':') {
        Some(i) if i > 0 && i + 1 < name.len() => (Some(&name[..i]), &name[i + 1..]),
        _ => (None, name),
    }
}

/// xmlNodeGetAttrValue (no DTD lookup)
fn attr_value(t: &Tree, el: NodeId, name: &str, ns: Option<&str>) -> Option<String> {
    for a in t.attrs_of(el) {
        if t.nodes[a].name != name {
            continue;
        }
        let ok = match ns {
            None => t.nodes[a].ns.is_none(),
            Some(u) => t.ns_href(t.nodes[a].ns) == Some(u),
        };
        if ok {
            return Some(t.list_get_string(t.nodes[a].children, true));
        }
    }
    None
}

/// xmlIsBlankNode
fn is_blank_node(t: &Tree, n: NodeId) -> bool {
    let nd = &t.nodes[n];
    if nd.t != NType::Text && nd.t != NType::CData {
        return false;
    }
    nd.content.bytes().all(tree::is_blank_char)
}

/// xmlNodeGetSpacePreserve: 1 preserve, 0 default, -1 unspecified
fn space_preserve(t: &Tree, n: NodeId) -> i32 {
    // only defined from an element (the reader passes the text node itself)
    if t.nodes[n].t != NType::Element {
        return -1;
    }
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            for a in t.attrs_of(c) {
                if t.nodes[a].name == "space" && t.ns_href(t.nodes[a].ns) == Some(XML_XML_NAMESPACE)
                {
                    let v = t.node_get_content(a);
                    if v == "preserve" {
                        return 1;
                    }
                    if v == "default" {
                        return 0;
                    }
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    -1
}

/// php_stream_read through fread(); Err when the stream is gone.
fn stream_read(vm: &mut Vm, h: &Value, len: usize) -> Result<Vec<u8>, ()> {
    if let Some(f) = vm.functions.get(b"get_resource_type".as_slice()).copied() {
        match f(vm, std::slice::from_ref(h)) {
            Ok(Value::String(s)) if s.as_bytes() == b"stream" => {}
            _ => {
                vm.current_exception = None;
                return Err(());
            }
        }
    }
    let f = match vm.functions.get(b"fread".as_slice()).copied() {
        Some(f) => f,
        None => return Err(()),
    };
    match f(vm, &[h.clone(), Value::Long(len as i64)]) {
        Ok(Value::String(s)) => Ok(s.as_bytes().to_vec()),
        _ => {
            vm.current_exception = None;
            Err(())
        }
    }
}

// ---------------------------------------------------------------------------
// PHP object

struct XmlReaderData {
    core: RefCell<Option<Box<Core>>>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property),
    write_property: Some(h_write_property),
    has_property: Some(h_has_property),
    unset_property: Some(h_unset_property),
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: Some(h_get_properties),
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(h_clone),
};

fn data_of(o: &Obj) -> Option<Rc<XmlReaderData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<XmlReaderData>().ok()
}

fn attach(o: &Obj) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(XmlReaderData {
            core: RefCell::new(None),
        }) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

fn create_object(_vm: &mut Vm, o: &Obj) {
    if data_of(o).is_none() {
        attach(o);
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

fn class_display(o: &Obj) -> String {
    goro_core::value::display_class_name(&o.borrow().class_name).to_string()
}

const PROPS: [&str; 14] = [
    "attributeCount",
    "baseURI",
    "depth",
    "hasAttributes",
    "hasValue",
    "isDefault",
    "isEmptyElement",
    "localName",
    "name",
    "namespaceURI",
    "nodeType",
    "prefix",
    "value",
    "xmlLang",
];

fn is_prop(name: &[u8]) -> bool {
    PROPS.iter().any(|p| p.as_bytes() == name)
}

fn s(v: Option<String>) -> Value {
    Value::String(PhpString::from_string(v.unwrap_or_default()))
}

/// xmlreader_property_reader: Err when the read function failed (exception
/// thrown).
fn prop_value(vm: &mut Vm, o: &Obj, name: &[u8]) -> Result<Value, ()> {
    let d = match data_of(o) {
        Some(d) => d,
        None => return Ok(Value::Null),
    };
    let core = d.core.borrow();
    let c = match core.as_ref() {
        Some(c) => c,
        None => {
            return Ok(match name {
                b"attributeCount" | b"depth" | b"nodeType" => Value::Long(0),
                b"hasAttributes" | b"hasValue" | b"isDefault" | b"isEmptyElement" => Value::False,
                _ => Value::String(PhpString::empty()),
            });
        }
    };
    let int = |v: i64| -> Result<i64, ()> { if v == -1 { Err(()) } else { Ok(v) } };
    let r = match name {
        b"attributeCount" => int(c.attribute_count()).map(Value::Long),
        b"baseURI" => Ok(s(c.base_uri())),
        b"depth" => int(c.depth()).map(Value::Long),
        b"hasAttributes" => Ok(vb(c.has_attributes())),
        b"hasValue" => Ok(vb(c.has_value())),
        b"isDefault" => Ok(Value::False),
        b"isEmptyElement" => int(c.is_empty_element()).map(|v| vb(v != 0)),
        b"localName" => Ok(s(c.local_name())),
        b"name" => Ok(s(c.name())),
        b"namespaceURI" => Ok(s(c.namespace_uri())),
        b"nodeType" => int(c.node_type()).map(Value::Long),
        b"prefix" => Ok(s(c.prefix())),
        b"value" => Ok(s(c.value())),
        b"xmlLang" => Ok(s(c.xml_lang())),
        _ => Ok(Value::Null),
    };
    drop(core);
    if r.is_err() {
        throw(
            vm,
            b"Error",
            "Failed to read property because no XML data has been read yet",
        );
    }
    r
}

fn h_read_property(vm: &mut Vm, o: &Obj, name: &[u8], _mode: u8) -> Option<Value> {
    if !is_prop(name) {
        return None;
    }
    Some(prop_value(vm, o, name).unwrap_or(Value::Null))
}

fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], _v: &Value) -> bool {
    if !is_prop(name) {
        return false;
    }
    let msg = format!(
        "Cannot modify readonly property {}::${}",
        class_display(o),
        String::from_utf8_lossy(name)
    );
    throw(vm, b"Error", &msg);
    true
}

fn h_has_property(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    if !is_prop(name) {
        return None;
    }
    match prop_value(vm, o, name) {
        Ok(v) => Some(if check_empty == 1 {
            v.is_truthy()
        } else {
            !matches!(v, Value::Null)
        }),
        Err(()) => Some(false),
    }
}

fn h_unset_property(vm: &mut Vm, o: &Obj, name: &[u8]) -> bool {
    if !is_prop(name) {
        return false;
    }
    let msg = format!(
        "Cannot unset {}::${}",
        class_display(o),
        String::from_utf8_lossy(name)
    );
    throw(vm, b"Error", &msg);
    true
}

/// xmlreader_get_debug_info
fn h_get_properties(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut arr = PhpArray::new();
    let std: Vec<(Vec<u8>, Value)> = o
        .borrow()
        .properties
        .iter()
        .filter(|(k, _)| !k.starts_with(b"__"))
        .cloned()
        .collect();
    for (k, v) in std {
        arr.set(ArrayKey::String(PhpString::from_vec(k)), v);
    }
    for p in PROPS {
        if let Ok(v) = prop_value(vm, o, p.as_bytes()) {
            arr.set(ArrayKey::String(PhpString::from_bytes(p.as_bytes())), v);
        }
    }
    Some(arr)
}

fn h_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let msg = format!(
        "Trying to clone an uncloneable object of class {}",
        class_display(src)
    );
    throw(vm, b"Error", &msg);
}

fn prop_read_hook(_vm: &mut Vm, _o: &Obj, name: &[u8]) -> Option<Option<Value>> {
    if is_prop(name) {
        Some(Some(Value::Null))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Methods

const METHODS: &[&str] = &[
    "close",
    "getAttribute",
    "getAttributeNo",
    "getAttributeNs",
    "getParserProperty",
    "isValid",
    "lookupNamespace",
    "moveToAttribute",
    "moveToAttributeNo",
    "moveToAttributeNs",
    "moveToElement",
    "moveToFirstAttribute",
    "moveToNextAttribute",
    "read",
    "next",
    "open",
    "fromUri",
    "fromStream",
    "readInnerXml",
    "readOuterXml",
    "readString",
    "setSchema",
    "setParserProperty",
    "setRelaxNGSchema",
    "setRelaxNGSchemaSource",
    "XML",
    "fromString",
    "expand",
];

const STATIC_METHODS: &[&str] = &["open", "fromUri", "fromStream", "XML", "fromString"];

fn canon(method: &[u8]) -> Option<&'static str> {
    METHODS
        .iter()
        .copied()
        .find(|m| m.as_bytes().eq_ignore_ascii_case(method))
}

fn has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    // non-static methods called statically error out in call_method
    let _ = is_static;
    canon(method).is_some()
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let m = canon(method)?;
    Some(match m {
        "getAttribute" | "moveToAttribute" => &["name"],
        "getAttributeNo" | "moveToAttributeNo" => &["index"],
        "getAttributeNs" | "moveToAttributeNs" => &["name", "namespace"],
        "getParserProperty" => &["property"],
        "setParserProperty" => &["property", "value"],
        "lookupNamespace" => &["prefix"],
        "next" => &["name"],
        "open" | "fromUri" => &["uri", "encoding", "flags"],
        "fromStream" => &["stream", "encoding", "flags", "documentUri"],
        "XML" | "fromString" => &["source", "encoding", "flags"],
        "setSchema" | "setRelaxNGSchema" => &["filename"],
        "setRelaxNGSchemaSource" => &["source"],
        "expand" => &["baseNode"],
        _ => &[],
    })
}

fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let m = match canon(method) {
        Some(m) => m,
        None => return Value::Null,
    };
    let fname = format!("XMLReader::{}", m);
    let this = match this {
        Some(Value::Object(o)) => Some(o.clone()),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Object(o) => Some(o.clone()),
            _ => None,
        },
        _ => None,
    };
    let is_static = STATIC_METHODS.contains(&m);
    let o = match (&this, is_static) {
        (Some(o), _) => Some(o.clone()),
        (None, true) => None,
        (None, false) => {
            throw(
                vm,
                b"Error",
                &format!("Non-static method {}() cannot be called statically", fname),
            );
            return Value::Null;
        }
    };
    let a = Args::new(fname.clone(), args);
    let r = match m {
        "open" | "fromUri" => m_open(vm, &a, o.as_ref(), called, m == "fromUri"),
        "XML" | "fromString" => m_xml(vm, &a, o.as_ref(), called, m == "fromString"),
        "fromStream" => m_from_stream(vm, &a, called),
        _ => {
            let o = o.unwrap();
            let d = match data_of(&o) {
                Some(d) => d,
                None => {
                    attach(&o);
                    data_of(&o).unwrap()
                }
            };
            instance_method(vm, &a, m, &o, &d)
        }
    };
    r.unwrap_or(Value::Null)
}

type R<T> = Result<T, ()>;

/// Report the parser errors collected by the reader.
fn flush_errors(vm: &mut Vm, d: &XmlReaderData, fname: &str) {
    let errs = match d.core.borrow_mut().as_mut() {
        Some(c) => std::mem::take(&mut c.errors),
        None => return,
    };
    if !errs.is_empty() {
        libxml::report(vm, fname, &errs, Style::Generic);
    }
}

/// Run `f` on the reader core (None when no data is loaded), then report
/// the errors it collected.
fn with_core<T>(
    vm: &mut Vm,
    d: &XmlReaderData,
    fname: &str,
    f: impl FnOnce(&mut Vm, Option<&mut Core>) -> T,
) -> T {
    let taken = d.core.borrow_mut().take();
    let mut core = taken;
    let r = f(vm, core.as_deref_mut());
    {
        let mut slot = d.core.borrow_mut();
        if slot.is_none() {
            *slot = core;
        }
    }
    flush_errors(vm, d, fname);
    r
}

fn arg_string(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<String> {
    let b = a.str_bytes(vm, i, name)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// 'p' / 'p!' zpp
fn arg_path(vm: &mut Vm, a: &Args, i: usize, name: &str, nullable: bool) -> R<Option<Vec<u8>>> {
    let v = a.raw(i);
    if nullable && matches!(v, None | Some(Value::Null)) {
        return Ok(None);
    }
    let b = a.str_bytes(vm, i, name)?;
    if b.contains(&0) {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #{} (${}) must not contain any null bytes",
                a.f,
                i + 1,
                name
            ),
        );
        return Err(());
    }
    Ok(Some(b))
}

fn empty_error(vm: &mut Vm, a: &Args, i: usize, name: &str) {
    throw(
        vm,
        b"ValueError",
        &format!(
            "{}(): Argument #{} (${}) must not be empty",
            a.f,
            i + 1,
            name
        ),
    );
}

fn valid_encoding(enc: &Option<Vec<u8>>) -> bool {
    match enc {
        None => true,
        Some(e) => parser::encoding_supported(&String::from_utf8_lossy(e)),
    }
}

/// object_init_with_constructor for the from*() factories.
fn new_instance(vm: &mut Vm, class: &[u8], core: Box<Core>) -> Value {
    let lower = class.to_ascii_lowercase();
    let disp = vm
        .classes
        .get(&lower)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| b"XMLReader".to_vec());
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(disp, id);
    vm.apply_class_property_defaults(&mut o, &lower);
    let o = Rc::new(RefCell::new(o));
    attach(&o);
    let has_ctor = lower != b"xmlreader"
        && vm
            .classes
            .get(&lower)
            .and_then(|c| c.get_method(b"__construct"))
            .is_some();
    let v = Value::Object(o.clone());
    if has_ctor {
        let _ = vm.call_object_method(&v, b"__construct", &[]);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    if let Some(d) = data_of(&o) {
        *d.core.borrow_mut() = Some(core);
    }
    v
}

fn resolve_file(source: &str) -> (Option<String>, bool) {
    // _xmlreader_get_valid_file_path
    let lower = source.to_ascii_lowercase();
    let mut path = source.to_string();
    let mut is_file_uri = false;
    let has_scheme = {
        let b = source.as_bytes();
        let mut i = 0;
        while i < b.len()
            && (b[i].is_ascii_alphanumeric() || b[i] == b'+' || b[i] == b'-' || b[i] == b'.')
        {
            i += 1;
        }
        i > 0 && b[0].is_ascii_alphabetic() && i < b.len() && b[i] == b':'
    };
    if has_scheme {
        if lower.starts_with("file:///") {
            is_file_uri = true;
            path = source[7..].to_string();
        } else if lower.starts_with("file://localhost/") {
            is_file_uri = true;
            path = source[16..].to_string();
        }
    }
    if !has_scheme || is_file_uri {
        let p = std::path::Path::new(&path);
        let abs = match std::fs::canonicalize(p) {
            Ok(c) => c,
            Err(_) => {
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    match std::env::current_dir() {
                        Ok(d) => d.join(p),
                        Err(_) => return (None, false),
                    }
                }
            }
        };
        return (Some(abs.to_string_lossy().into_owned()), true);
    }
    (Some(path), false)
}

fn m_open(
    vm: &mut Vm,
    a: &Args,
    this: Option<&Obj>,
    called: &[u8],
    use_exceptions: bool,
) -> R<Value> {
    a.count(vm, 1, 3)?;
    let source = arg_path(vm, a, 0, "uri", false)?.unwrap_or_default();
    let encoding = arg_path(vm, a, 1, "encoding", true)?;
    let options = a.long(vm, 2, "flags", 0)?;
    if let Some(o) = this {
        if let Some(d) = data_of(o) {
            *d.core.borrow_mut() = None;
        }
    }
    if source.is_empty() {
        empty_error(vm, a, 0, "uri");
        return Err(());
    }
    if !valid_encoding(&encoding) {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($encoding) must be a valid character encoding",
                a.f
            ),
        );
        return Err(());
    }
    let source = String::from_utf8_lossy(&source).into_owned();
    let (valid, is_local) = resolve_file(&source);
    let data = match &valid {
        Some(p) if is_local => {
            if std::path::Path::new(p).is_dir() {
                None
            } else {
                std::fs::read(p).ok()
            }
        }
        Some(p) => crate::io::read_resource(vm, p),
        None => None,
    };
    let data = match data {
        Some(d) => d,
        None => {
            if use_exceptions {
                throw(vm, b"Error", "Unable to open source data");
                return Err(());
            }
            vm.emit_warning(&format!("{}(): Unable to open source data", a.f));
            return Ok(Value::False);
        }
    };
    let enc = encoding.map(|e| String::from_utf8_lossy(&e).into_owned());
    let core = Box::new(Core::new(
        Source::Data(data, 0),
        Vec::new(),
        valid,
        enc.as_deref(),
        options,
        vm,
    ));
    match this {
        None => {
            let class: Vec<u8> = if use_exceptions {
                called.to_vec()
            } else {
                b"XMLReader".to_vec()
            };
            Ok(new_instance(vm, &class, core))
        }
        Some(o) => {
            if let Some(d) = data_of(o) {
                *d.core.borrow_mut() = Some(core);
            }
            Ok(Value::True)
        }
    }
}

fn m_xml(
    vm: &mut Vm,
    a: &Args,
    this: Option<&Obj>,
    called: &[u8],
    use_exceptions: bool,
) -> R<Value> {
    a.count(vm, 1, 3)?;
    let source = a.str_bytes(vm, 0, "source")?;
    let encoding = arg_path(vm, a, 1, "encoding", true)?;
    let options = a.long(vm, 2, "flags", 0)?;
    if let Some(o) = this {
        if let Some(d) = data_of(o) {
            *d.core.borrow_mut() = None;
        }
    }
    if source.is_empty() {
        empty_error(vm, a, 0, "source");
        return Err(());
    }
    if !valid_encoding(&encoding) {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($encoding) must be a valid character encoding",
                a.f
            ),
        );
        return Err(());
    }
    let _ = use_exceptions;
    // the base directory: the current working directory
    let uri = std::env::current_dir().ok().map(|d| {
        let mut s = d.to_string_lossy().into_owned();
        if !s.ends_with('/') {
            s.push('/');
        }
        s
    });
    let enc = encoding.map(|e| String::from_utf8_lossy(&e).into_owned());
    let core = Box::new(Core::new(
        Source::Memory,
        source,
        uri,
        enc.as_deref(),
        options,
        vm,
    ));
    match this {
        None => {
            let class: Vec<u8> = if use_exceptions {
                called.to_vec()
            } else {
                b"XMLReader".to_vec()
            };
            Ok(new_instance(vm, &class, core))
        }
        Some(o) => {
            if let Some(d) = data_of(o) {
                *d.core.borrow_mut() = Some(core);
            }
            Ok(Value::True)
        }
    }
}

fn m_from_stream(vm: &mut Vm, a: &Args, called: &[u8]) -> R<Value> {
    a.count(vm, 1, 4)?;
    let h = match a.raw(0) {
        Some(v @ Value::Resource(_)) => v,
        Some(v) => {
            a.type_err(vm, 0, "stream", "resource", &v);
            return Err(());
        }
        None => {
            a.type_err(vm, 0, "stream", "resource", &Value::Null);
            return Err(());
        }
    };
    let encoding = arg_path(vm, a, 1, "encoding", true)?;
    let options = a.long(vm, 2, "flags", 0)?;
    let doc_uri = arg_path(vm, a, 3, "documentUri", true)?;
    if let Some(f) = vm.functions.get(b"get_resource_type".as_slice()).copied() {
        let ok = matches!(f(vm, std::slice::from_ref(&h)), Ok(Value::String(ref s)) if s.as_bytes() == b"stream");
        if !ok {
            vm.current_exception = None;
            throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): supplied resource is not a valid stream resource",
                    a.f
                ),
            );
            return Err(());
        }
    }
    if !valid_encoding(&encoding) {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($encoding) must be a valid character encoding",
                a.f
            ),
        );
        return Err(());
    }
    let enc = encoding.map(|e| String::from_utf8_lossy(&e).into_owned());
    let url = doc_uri.map(|u| String::from_utf8_lossy(&u).into_owned());
    let core = Box::new(Core::new(
        Source::Stream(h),
        Vec::new(),
        url,
        enc.as_deref(),
        options,
        vm,
    ));
    Ok(new_instance(vm, called, core))
}

fn need_loaded(vm: &mut Vm, d: &XmlReaderData, msg: &str) -> R<()> {
    if d.core.borrow().is_none() {
        throw(vm, b"Error", msg);
        return Err(());
    }
    Ok(())
}

fn instance_method(vm: &mut Vm, a: &Args, m: &str, o: &Obj, d: &XmlReaderData) -> R<Value> {
    let f = a.f.clone();
    match m {
        "close" => {
            a.count(vm, 0, 0)?;
            *d.core.borrow_mut() = None;
            Ok(Value::True)
        }
        "getAttribute" | "lookupNamespace" => {
            a.count(vm, 1, 1)?;
            let pname = if m == "getAttribute" {
                "name"
            } else {
                "prefix"
            };
            let name = arg_string(vm, a, 0, pname)?;
            if name.is_empty() {
                empty_error(vm, a, 0, pname);
                return Err(());
            }
            let r = with_core(vm, d, &f, |_, c| {
                c.and_then(|c| {
                    if m == "getAttribute" {
                        c.get_attribute(&name)
                    } else {
                        c.lookup_namespace(&name)
                    }
                })
            });
            Ok(r.map(|s| Value::String(PhpString::from_string(s)))
                .unwrap_or(Value::Null))
        }
        "getAttributeNo" => {
            a.count(vm, 1, 1)?;
            let no = a.long(vm, 0, "index", 0)?;
            let r = with_core(vm, d, &f, |_, c| c.and_then(|c| c.get_attribute_no(no)));
            Ok(r.map(|s| Value::String(PhpString::from_string(s)))
                .unwrap_or(Value::Null))
        }
        "getAttributeNs" | "moveToAttributeNs" => {
            a.count(vm, 2, 2)?;
            let name = arg_string(vm, a, 0, "name")?;
            let ns = arg_string(vm, a, 1, "namespace")?;
            if name.is_empty() {
                empty_error(vm, a, 0, "name");
                return Err(());
            }
            if ns.is_empty() {
                empty_error(vm, a, 1, "namespace");
                return Err(());
            }
            if m == "getAttributeNs" {
                let r = with_core(vm, d, &f, |_, c| {
                    c.and_then(|c| c.get_attribute_ns(&name, &ns))
                });
                Ok(r.map(|s| Value::String(PhpString::from_string(s)))
                    .unwrap_or(Value::Null))
            } else {
                let r = with_core(vm, d, &f, |_, c| {
                    c.map(|c| c.move_to_attribute_ns(&name, &ns)).unwrap_or(0)
                });
                Ok(vb(r == 1))
            }
        }
        "getParserProperty" => {
            a.count(vm, 1, 1)?;
            let p = a.long(vm, 0, "property", 0)?;
            need_loaded(vm, d, "Cannot access parser properties before loading data")?;
            let r = with_core(vm, d, &f, |_, c| {
                c.map(|c| c.get_parser_prop(p)).unwrap_or(-1)
            });
            if r == -1 {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #1 ($property) must be a valid parser property",
                        f
                    ),
                );
                return Err(());
            }
            Ok(vb(r != 0))
        }
        "setParserProperty" => {
            a.count(vm, 2, 2)?;
            let p = a.long(vm, 0, "property", 0)?;
            let v = a.bool(vm, 1, "value", false)?;
            need_loaded(vm, d, "Cannot access parser properties before loading data")?;
            let r = with_core(vm, d, &f, |_, c| {
                c.map(|c| c.set_parser_prop(p, v)).unwrap_or(-1)
            });
            if r == -1 {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #1 ($property) must be a valid parser property",
                        f
                    ),
                );
                return Err(());
            }
            Ok(Value::True)
        }
        "isValid" => {
            a.count(vm, 0, 0)?;
            let r = with_core(vm, d, &f, |_, c| c.map(|c| c.is_valid()).unwrap_or(false));
            Ok(vb(r))
        }
        "moveToAttribute" => {
            a.count(vm, 1, 1)?;
            let name = arg_string(vm, a, 0, "name")?;
            if name.is_empty() {
                empty_error(vm, a, 0, "name");
                return Err(());
            }
            let r = with_core(vm, d, &f, |_, c| {
                c.map(|c| c.move_to_attribute(&name)).unwrap_or(0)
            });
            Ok(vb(r == 1))
        }
        "moveToAttributeNo" => {
            a.count(vm, 1, 1)?;
            let no = a.long(vm, 0, "index", 0)?;
            let r = with_core(vm, d, &f, |_, c| {
                c.map(|c| c.move_to_attribute_no(no)).unwrap_or(0)
            });
            Ok(vb(r == 1))
        }
        "moveToElement" | "moveToFirstAttribute" | "moveToNextAttribute" => {
            a.count(vm, 0, 0)?;
            let r = with_core(vm, d, &f, |_, c| {
                c.map(|c| match m {
                    "moveToElement" => c.move_to_element(),
                    "moveToFirstAttribute" => c.move_to_first_attribute(),
                    _ => c.move_to_next_attribute(),
                })
                .unwrap_or(0)
            });
            Ok(vb(r == 1))
        }
        "read" => {
            a.count(vm, 0, 0)?;
            need_loaded(vm, d, "Data must be loaded before reading")?;
            let r = with_core(vm, d, &f, |vm, c| c.map(|c| c.read(vm)).unwrap_or(-1));
            Ok(vb(r == 1))
        }
        "next" => {
            a.count(vm, 0, 1)?;
            let name = match a.raw(0) {
                None | Some(Value::Null) => None,
                Some(_) => Some(arg_string(vm, a, 0, "name")?),
            };
            need_loaded(vm, d, "Data must be loaded before reading")?;
            let r = with_core(vm, d, &f, |vm, c| {
                let c = match c {
                    Some(c) => c,
                    None => return -1,
                };
                let mut ret = c.next(vm);
                if let Some(n) = &name {
                    while ret == 1 {
                        if c.local_name().as_deref() == Some(n.as_str()) {
                            return 1;
                        }
                        ret = c.next(vm);
                    }
                }
                ret
            });
            Ok(vb(r == 1))
        }
        "readInnerXml" | "readOuterXml" => {
            a.count(vm, 0, 0)?;
            let r = with_core(vm, d, &f, |vm, c| {
                c.and_then(|c| {
                    if m == "readInnerXml" {
                        c.read_inner_xml(vm)
                    } else {
                        c.read_outer_xml(vm)
                    }
                })
            });
            Ok(Value::String(PhpString::from_vec(r.unwrap_or_default())))
        }
        "readString" => {
            a.count(vm, 0, 0)?;
            let r = with_core(vm, d, &f, |vm, c| c.and_then(|c| c.read_string(vm)));
            Ok(Value::String(PhpString::from_string(r.unwrap_or_default())))
        }
        "setSchema" | "setRelaxNGSchema" | "setRelaxNGSchemaSource" => {
            a.count(vm, 1, 1)?;
            let pname = if m == "setRelaxNGSchemaSource" {
                "source"
            } else {
                "filename"
            };
            let source = arg_path(vm, a, 0, pname, true)?;
            if let Some(s) = &source {
                if s.is_empty() {
                    empty_error(vm, a, 0, pname);
                    return Err(());
                }
            }
            need_loaded(vm, d, "Schema must be set prior to reading")?;
            let initial = d
                .core
                .borrow()
                .as_ref()
                .map(|c| c.mode == Mode::Initial)
                .unwrap_or(false);
            if source.is_none() && initial {
                // unset the associated schema
                return Ok(Value::True);
            }
            // Schema validation (XSD / RelaxNG) is not available: the schema
            // cannot be compiled.
            vm.emit_warning(&format!("{}(): Schema contains errors", f));
            Ok(Value::False)
        }
        "expand" => {
            a.count(vm, 0, 1)?;
            let base = a.obj_of(vm, 0, "baseNode", "DOMNode", true)?;
            let mut docp: Option<NodeId> = None;
            if let Some(b) = &base {
                match dom::node_of(b) {
                    Some(n) => docp = dom::doc_of_node(n),
                    None => {
                        vm.emit_warning(&format!("{}(): Couldn't fetch {}", f, class_display(b)));
                        return Ok(Value::False);
                    }
                }
            }
            need_loaded(vm, d, "Data must be loaded before expanding")?;
            let node = with_core(vm, d, &f, |vm, c| c.and_then(|c| c.expand(vm)));
            let node = match node {
                Some(n) => n,
                None => {
                    vm.emit_warning(&format!("{}(): An Error Occurred while expanding", f));
                    return Ok(Value::False);
                }
            };
            let copy = tree::with(|t| match t.nodes[node].t {
                NType::Dtd
                | NType::DocumentType
                | NType::Notation
                | NType::ElementDecl
                | NType::AttributeDecl
                | NType::EntityDecl => None,
                _ => Some(t.copy_node(node, docp, 1, None)),
            });
            match copy {
                Some(c) => Ok(dom::node_obj(vm, c)),
                None => {
                    let line = vm.current_line;
                    vm.emit_notice_at(&format!("{}(): Cannot expand this node type", f), line);
                    Ok(Value::False)
                }
            }
        }
        _ => {
            let _ = o;
            Ok(Value::Null)
        }
    }
}

// ---------------------------------------------------------------------------
// Registration

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"xmlreader");
    let mut ce = ClassEntry::new(b"XMLReader".to_vec());
    for (n, v) in [
        ("NONE", T_NONE),
        ("ELEMENT", T_ELEMENT),
        ("ATTRIBUTE", T_ATTRIBUTE),
        ("TEXT", T_TEXT),
        ("CDATA", T_CDATA),
        ("ENTITY_REF", T_ENTITY_REF),
        ("ENTITY", T_ENTITY),
        ("PI", T_PI),
        ("COMMENT", T_COMMENT),
        ("DOC", T_DOC),
        ("DOC_TYPE", T_DOC_TYPE),
        ("DOC_FRAGMENT", T_DOC_FRAGMENT),
        ("NOTATION", T_NOTATION),
        ("WHITESPACE", T_WHITESPACE),
        ("SIGNIFICANT_WHITESPACE", T_SIGNIFICANT_WHITESPACE),
        ("END_ELEMENT", T_END_ELEMENT),
        ("END_ENTITY", 16),
        ("XML_DECLARATION", 17),
        ("LOADDTD", P_LOADDTD),
        ("DEFAULTATTRS", P_DEFAULTATTRS),
        ("VALIDATE", P_VALIDATE),
        ("SUBST_ENTITIES", P_SUBST_ENTITIES),
    ] {
        ce.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
    }
    vm.classes.insert(b"xmlreader".to_vec(), ce);
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: Some(prop_read_hook),
        param_names: Some(param_names),
    };
    vm.native_class_handlers.insert(b"xmlreader".to_vec(), h);
    vm.object_create_handlers
        .insert(b"xmlreader".to_vec(), create_object);
}

fn vb(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}
