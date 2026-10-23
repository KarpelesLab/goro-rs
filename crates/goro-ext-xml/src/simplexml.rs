//! ext/simplexml, ported from php-src/ext/simplexml/simplexml.c on top of the
//! tree in `tree.rs`.

use crate::libxml;
use crate::parser::{self, XML_PARSE_RECOVER};
use crate::sax2;
use crate::serialize;
use crate::tree::{self, NType, NodeId, Tree};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum It {
    None = 0,
    Element = 1,
    Child = 2,
    AttrList = 3,
}

pub struct Sxe {
    pub node: Cell<Option<NodeId>>,
    pub doc: Cell<Option<NodeId>>,
    pub itype: Cell<It>,
    pub name: RefCell<Option<String>>,
    pub nsprefix: RefCell<Option<String>>,
    pub isprefix: Cell<bool>,
    /// iter.data: the current element of the iteration
    pub data: RefCell<Option<Value>>,
    pub xpath_ns: RefCell<Vec<(String, String)>>,
    /// user subclass overrides count()
    pub fptr_count: Cell<bool>,
}

impl Sxe {
    fn new() -> Sxe {
        Sxe {
            node: Cell::new(None),
            doc: Cell::new(None),
            itype: Cell::new(It::None),
            name: RefCell::new(None),
            nsprefix: RefCell::new(None),
            isprefix: Cell::new(false),
            data: RefCell::new(None),
            xpath_ns: RefCell::new(Vec::new()),
            fptr_count: Cell::new(false),
        }
    }
}

type Obj = Rc<RefCell<PhpObject>>;

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property),
    write_property: Some(h_write_property),
    has_property: Some(h_has_property),
    unset_property: Some(h_unset_property),
    read_dimension: Some(h_read_dimension),
    write_dimension: Some(h_write_dimension),
    has_dimension: Some(h_has_dimension),
    unset_dimension: Some(h_unset_dimension),
    get_properties: Some(h_get_properties),
    count_elements: None,
    cast: Some(h_cast),
    compare: Some(h_compare),
    clone_obj: Some(h_clone),
};

pub fn sxe_of(o: &Obj) -> Option<Rc<Sxe>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<Sxe>().ok()
}

fn is_sxe_class(vm: &Vm, class_lower: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if cur == b"simplexmlelement" {
            return true;
        }
        match vm.classes.get(&cur).and_then(|c| c.parent.clone()) {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// Does the (user) class override count()?
fn find_fptr_count(vm: &Vm, class_lower: &[u8]) -> bool {
    if class_lower == b"simplexmlelement" || class_lower == b"simplexmliterator" {
        return false;
    }
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if cur == b"simplexmlelement" || cur == b"simplexmliterator" {
            return false;
        }
        match vm.classes.get(&cur) {
            Some(c) => {
                if c.methods.contains_key(&b"count".to_vec()) {
                    return true;
                }
                match &c.parent {
                    Some(p) => cur = p.to_ascii_lowercase(),
                    None => return false,
                }
            }
            None => return false,
        }
    }
    false
}

fn class_display(vm: &Vm, class_name: &[u8]) -> Vec<u8> {
    let lower = class_name.to_ascii_lowercase();
    match lower.as_slice() {
        b"simplexmlelement" => b"SimpleXMLElement".to_vec(),
        b"simplexmliterator" => b"SimpleXMLIterator".to_vec(),
        _ => vm
            .classes
            .get(&lower)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| class_name.to_vec()),
    }
}

/// php_sxe_object_new + attach state
fn new_sxe_object(vm: &mut Vm, class_name: &[u8], sxe: Sxe) -> Obj {
    let lower = class_name.to_ascii_lowercase();
    let disp = class_display(vm, class_name);
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(disp, id);
    if vm.classes.contains_key(&lower)
        && lower != b"simplexmlelement"
        && lower != b"simplexmliterator"
    {
        vm.apply_class_property_defaults(&mut o, &lower);
    }
    sxe.fptr_count.set(find_fptr_count(vm, &lower));
    o.native = Some(NativeData {
        data: Rc::new(sxe) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    Rc::new(RefCell::new(o))
}

/// An object of `class` (SimpleXMLElement or a subclass) for `node` of
/// `doc` (object_init_ex + php_libxml_increment_node_ptr, as ext/xsl's
/// transformToDoc() does with a result document).
pub fn wrap_node(vm: &mut Vm, class_name: &[u8], doc: NodeId, node: NodeId) -> Value {
    let s = Sxe::new();
    s.doc.set(Some(doc));
    s.node.set(Some(node));
    Value::Object(new_sxe_object(vm, class_name, s))
}

/// node_as_zval
fn node_as_zval(
    vm: &mut Vm,
    parent: &Obj,
    psxe: &Sxe,
    node: Option<NodeId>,
    itype: It,
    name: Option<&str>,
    nsprefix: Option<&str>,
    isprefix: bool,
) -> Value {
    let s = Sxe::new();
    s.doc.set(psxe.doc.get());
    s.itype.set(itype);
    *s.name.borrow_mut() = name.map(|x| x.to_string());
    if let Some(p) = nsprefix {
        if !p.is_empty() {
            *s.nsprefix.borrow_mut() = Some(p.to_string());
            s.isprefix.set(isprefix);
        }
    }
    s.node.set(node);
    let cn = parent.borrow().class_name.clone();
    Value::Object(new_sxe_object(vm, &cn, s))
}

/// php_error_docref(NULL, E_WARNING, ...) from an object handler: the
/// message is prefixed with the active function ("main" at top level).
pub fn docref_warning(vm: &mut Vm, msg: &str) {
    let f = vm
        .call_stack
        .last()
        .map(|f| f.0.clone())
        .unwrap_or_else(|| "main".to_string());
    vm.emit_warning(&format!("{}(): {}", f, msg));
}

fn throw_error(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    // (zend_throw_exception(): one already thrown becomes its previous)
    if let Some(prev) = vm.current_exception.take() {
        Vm::exception_set_previous(&e, prev);
    }
    vm.current_exception = Some(e);
}

fn not_initialized(vm: &mut Vm) {
    throw_error(vm, b"Error", "SimpleXMLElement is not properly initialized");
}

/// GET_NODE
fn get_node(vm: &mut Vm, s: &Sxe) -> Option<NodeId> {
    match s.node.get() {
        Some(n) => Some(n),
        None => {
            not_initialized(vm);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tree helpers mirroring the C code

fn match_ns(t: &Tree, node: NodeId, name: Option<&str>, prefix: bool) -> bool {
    let ns = t.nodes[node].ns;
    if name.is_none() && (ns.is_none() || t.nss[ns.unwrap()].prefix.is_none()) {
        return true;
    }
    if let Some(ns) = ns {
        let v = if prefix {
            t.nss[ns].prefix.as_deref()
        } else {
            Some(t.nss[ns].href.as_str())
        };
        if v.is_some() && v == name {
            return true;
        }
    }
    false
}

fn is_blank_node(t: &Tree, n: NodeId) -> bool {
    let nd = &t.nodes[n];
    if nd.t != NType::Text && nd.t != NType::CData {
        return false;
    }
    tree::is_blank_str(&nd.content)
}

impl Sxe {
    fn nsp(&self) -> Option<String> {
        self.nsprefix.borrow().clone()
    }
    fn nm(&self) -> Option<String> {
        self.name.borrow().clone()
    }
}

/// php_sxe_iterator_fetch (without storing data)
fn iterator_fetch_node(t: &Tree, s: &Sxe, mut node: Option<NodeId>) -> Option<NodeId> {
    let prefix = s.nsp();
    let isprefix = s.isprefix.get();
    let name = s.nm();
    match s.itype.get() {
        It::AttrList => {
            while let Some(n) = node {
                if t.nodes[n].t == NType::Attribute {
                    let ok = match &name {
                        Some(nm) => {
                            t.nodes[n].name == *nm && match_ns(t, n, prefix.as_deref(), isprefix)
                        }
                        None => match_ns(t, n, prefix.as_deref(), isprefix),
                    };
                    if ok {
                        break;
                    }
                }
                node = t.nodes[n].next;
            }
        }
        It::Element if name.is_some() => {
            let nm = name.unwrap();
            while let Some(n) = node {
                if t.nodes[n].t == NType::Element
                    && t.nodes[n].name == nm
                    && match_ns(t, n, prefix.as_deref(), isprefix)
                {
                    break;
                }
                node = t.nodes[n].next;
            }
        }
        _ => {
            while let Some(n) = node {
                if t.nodes[n].t == NType::Element && match_ns(t, n, prefix.as_deref(), isprefix) {
                    break;
                }
                node = t.nodes[n].next;
            }
        }
    }
    node
}

/// php_sxe_reset_iterator_no_clear_iter_data(sxe, 0)
fn reset_iter_node(t: &Tree, s: &Sxe) -> Option<NodeId> {
    let node = s.node.get()?;
    let start = match s.itype.get() {
        It::Element | It::Child | It::None => t.nodes[node].children,
        It::AttrList => t.nodes[node].properties,
    };
    iterator_fetch_node(t, s, start)
}

/// php_sxe_get_first_node_non_destructive
pub fn first_node(t: &Tree, s: &Sxe, node: Option<NodeId>) -> Option<NodeId> {
    if s.itype.get() != It::None {
        reset_iter_node(t, s)
    } else {
        node
    }
}

fn get_element_by_offset(
    t: &Tree,
    s: &Sxe,
    offset: i64,
    mut node: Option<NodeId>,
) -> (Option<NodeId>, i64) {
    let mut nodendx = 0i64;
    if s.itype.get() == It::None {
        if offset == 0 {
            return (node, 0);
        } else {
            return (None, 0);
        }
    }
    let prefix = s.nsp();
    let name = s.nm();
    while let Some(n) = node {
        if nodendx > offset {
            break;
        }
        if t.nodes[n].t == NType::Element && match_ns(t, n, prefix.as_deref(), s.isprefix.get()) {
            if s.itype.get() == It::Child
                || (s.itype.get() == It::Element && Some(&t.nodes[n].name) == name.as_ref())
            {
                if nodendx == offset {
                    break;
                }
                nodendx += 1;
            }
        }
        node = t.nodes[n].next;
    }
    (node, nodendx)
}

fn find_element_by_name(t: &Tree, s: &Sxe, mut node: Option<NodeId>, name: &str) -> Option<NodeId> {
    let prefix = s.nsp();
    while let Some(n) = node {
        if t.nodes[n].t == NType::Element
            && match_ns(t, n, prefix.as_deref(), s.isprefix.get())
            && t.nodes[n].name == name
        {
            return Some(n);
        }
        node = t.nodes[n].next;
    }
    None
}

/// sxe_get_element_by_name: returns the node whose child is named `name`.
fn get_element_by_name(t: &Tree, s: &Sxe, node: Option<NodeId>, name: &str) -> Option<NodeId> {
    let mut node = node;
    let mut orgnode = node;
    if s.itype.get() != It::AttrList {
        let orgtype = s.itype.get();
        if orgtype == It::None {
            s.itype.set(It::Child);
        }
        node = first_node(t, s, node);
        s.itype.set(orgtype);
    }
    if s.itype.get() == It::Element {
        let nm = s.nm().unwrap_or_default();
        orgnode = find_element_by_name(t, s, node, &nm);
        orgnode?;
        node = t.nodes[orgnode.unwrap()].children;
    }
    let prefix = s.nsp();
    while let Some(n) = node {
        if t.nodes[n].t == NType::Element
            && match_ns(t, n, prefix.as_deref(), s.isprefix.get())
            && t.nodes[n].name == name
        {
            return orgnode;
        }
        node = t.nodes[n].next;
    }
    None
}

/// xmlNewDocRawNode + xmlAddChild under `parent` (xmlNewTextChild)
fn new_text_child(
    t: &mut Tree,
    parent: NodeId,
    ns: Option<tree::NsId>,
    name: &str,
    content: Option<&str>,
) -> NodeId {
    let doc = t.nodes[parent].doc;
    let el = t.new_element(doc, name);
    if t.nodes[parent].t == NType::Element {
        t.nodes[el].ns = ns.or(t.nodes[parent].ns);
    } else {
        t.nodes[el].ns = ns;
    }
    if let Some(c) = content {
        if !c.is_empty() {
            let tx = t.new_text(doc, c);
            t.link_child_raw(el, tx);
        }
    }
    t.add_child(parent, el);
    el
}

/// xmlNewChild: content parsed for entity references
fn new_child(
    t: &mut Tree,
    parent: NodeId,
    ns: Option<tree::NsId>,
    name: &str,
    content: Option<&str>,
) -> NodeId {
    let doc = t.nodes[parent].doc;
    let el = t.new_element(doc, name);
    if t.nodes[parent].t == NType::Element {
        t.nodes[el].ns = ns.or(t.nodes[parent].ns);
    } else {
        t.nodes[el].ns = ns;
    }
    if let Some(c) = content {
        string_get_node_list(t, doc, el, c);
    }
    t.add_child(parent, el);
    el
}

/// xmlNodeParseAttValue into `parent`'s children (xmlStringGetNodeList).
pub fn string_get_node_list(t: &mut Tree, doc: Option<NodeId>, parent: NodeId, value: &str) {
    let b = value.as_bytes();
    if b.is_empty() {
        return;
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut cur = 0usize;
    let mut q = 0usize;
    let len = b.len();
    let flush = |t: &mut Tree, buf: &mut Vec<u8>, nodes: &mut Vec<NodeId>| {
        if !buf.is_empty() {
            let s = String::from_utf8_lossy(buf).into_owned();
            buf.clear();
            let n = t.new_text(doc, &s);
            nodes.push(n);
        }
    };
    while cur < len {
        if b[cur] == b'&' {
            let mut charval: u32 = 0;
            if cur != q {
                buf.extend_from_slice(&b[q..cur]);
            }
            if cur + 2 < len && b.get(cur + 1) == Some(&b'#') && b.get(cur + 2) == Some(&b'x') {
                cur += 3;
                let mut tmp = 0u8;
                while cur < len {
                    tmp = b[cur];
                    if tmp == b';' {
                        break;
                    }
                    let d = match tmp {
                        b'0'..=b'9' => (tmp - b'0') as u32,
                        b'a'..=b'f' => (tmp - b'a') as u32 + 10,
                        b'A'..=b'F' => (tmp - b'A') as u32 + 10,
                        _ => {
                            charval = 0;
                            break;
                        }
                    };
                    charval = (charval * 16 + d).min(0x110000);
                    cur += 1;
                }
                if tmp == b';' && cur < len {
                    cur += 1;
                }
                q = cur;
            } else if b.get(cur + 1) == Some(&b'#') {
                cur += 2;
                let mut tmp = 0u8;
                while cur < len {
                    tmp = b[cur];
                    if tmp == b';' {
                        break;
                    }
                    if tmp.is_ascii_digit() {
                        charval = (charval * 10 + (tmp - b'0') as u32).min(0x110000);
                    } else {
                        charval = 0;
                        break;
                    }
                    cur += 1;
                }
                if tmp == b';' && cur < len {
                    cur += 1;
                }
                q = cur;
            } else {
                cur += 1;
                q = cur;
                while cur < len && b[cur] != b';' {
                    cur += 1;
                }
                if cur >= len {
                    break;
                }
                if cur != q {
                    let name = String::from_utf8_lossy(&b[q..cur]).into_owned();
                    let predef = match name.as_str() {
                        "lt" => Some("<"),
                        "gt" => Some(">"),
                        "amp" => Some("&"),
                        "apos" => Some("'"),
                        "quot" => Some("\""),
                        _ => None,
                    };
                    if let Some(p) = predef {
                        buf.extend_from_slice(p.as_bytes());
                    } else {
                        flush(t, &mut buf, &mut nodes);
                        let r = t.new_entity_ref(doc, &name);
                        nodes.push(r);
                    }
                }
                cur += 1;
                q = cur;
            }
            if charval != 0 {
                let cv = if charval >= 0x110000 { 0xFFFD } else { charval };
                if let Some(c) = char::from_u32(cv) {
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                }
            }
        } else {
            cur += 1;
        }
    }
    if cur != q && q < len {
        buf.extend_from_slice(&b[q..cur.min(len)]);
    }
    flush(t, &mut buf, &mut nodes);
    if nodes.is_empty() {
        let n = t.new_text(doc, "");
        nodes.push(n);
    }
    for n in nodes {
        t.link_child_raw(parent, n);
    }
}

// ---------------------------------------------------------------------------
// Property / dimension access

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Elements,
    Attribs,
}

/// BP_VAR_* subset
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bp {
    R,
    W,
    Is,
}

enum Member {
    Long(i64),
    Str(String),
}

fn member_of(vm: &mut Vm, v: &Value) -> Option<Member> {
    match v {
        Value::Long(n) => Some(Member::Long(*n)),
        Value::String(s) => Some(Member::Str(s.to_string_lossy())),
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            member_of(vm, &inner)
        }
        other => {
            let s = vm.value_to_string(other);
            if vm.current_exception.is_some() {
                return None;
            }
            Some(Member::Str(s.to_string_lossy()))
        }
    }
}

/// sxe_prop_dim_read
fn prop_dim_read(
    vm: &mut Vm,
    obj: &Obj,
    member: Option<&Value>,
    mut access: Access,
    bp: Bp,
) -> Value {
    let s = match sxe_of(obj) {
        Some(s) => s,
        None => return Value::Null,
    };
    let mem: Option<Member> = match member {
        None => {
            if s.itype.get() == It::AttrList {
                throw_error(vm, b"Error", "Cannot create unnamed attribute");
                return Value::Null;
            }
            access = Access::Elements;
            None
        }
        Some(m) => {
            let mm = match member_of(vm, m) {
                Some(x) => x,
                None => return Value::Null,
            };
            if let Member::Long(_) = mm {
                if s.itype.get() != It::AttrList {
                    access = Access::Elements;
                }
            }
            Some(mm)
        }
    };
    let node = match get_node(vm, &s) {
        Some(n) => n,
        None => return Value::Null,
    };
    let mut attr: Option<NodeId> = None;
    let mut test = false;
    let mut node = Some(node);
    let it = s.itype.get();
    if it == It::AttrList {
        access = Access::Attribs;
        node = tree::with(|t| first_node(t, &s, node));
        attr = node;
        test = s.nm().is_some();
    } else if it != It::Child {
        node = tree::with(|t| first_node(t, &s, node));
        attr = node.and_then(|n| tree::with(|t| t.nodes[n].properties));
        test = false;
        if mem.is_none() {
            if let Some(n) = node {
                let pdoc = tree::with(|t| {
                    t.nodes[n]
                        .parent
                        .map(|p| t.nodes[p].t == NType::Document)
                        .unwrap_or(false)
                });
                if pdoc {
                    throw_error(vm, b"Error", "Cannot create unnamed attribute");
                    return Value::Null;
                }
            }
        }
    }
    let node = match node {
        Some(n) => n,
        None => return Value::Null,
    };
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    if access == Access::Attribs {
        let mm = mem.unwrap();
        if !matches!(mm, Member::Long(_)) || it == It::AttrList {
            let sname = s.nm();
            let found = tree::with(|t| {
                let mut a = attr;
                match &mm {
                    Member::Long(idx) => {
                        let mut nodendx = 0;
                        while let Some(x) = a {
                            if nodendx > *idx {
                                break;
                            }
                            if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                                && match_ns(t, x, nsp.as_deref(), isp)
                            {
                                if nodendx == *idx {
                                    return Some(x);
                                }
                                nodendx += 1;
                            }
                            a = t.nodes[x].next;
                        }
                        None
                    }
                    Member::Str(name) => {
                        while let Some(x) = a {
                            if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                                && t.nodes[x].name == *name
                                && match_ns(t, x, nsp.as_deref(), isp)
                            {
                                return Some(x);
                            }
                            a = t.nodes[x].next;
                        }
                        None
                    }
                }
            });
            if let Some(a) = found {
                return node_as_zval(vm, obj, &s, Some(a), It::None, None, nsp.as_deref(), isp);
            }
        }
        return Value::Null;
    }
    // elements
    match &mem {
        None | Some(Member::Long(_)) => {
            let mynode = node;
            let mut cur = Some(node);
            let mut cnt = 0i64;
            if it == It::Child {
                cur = tree::with(|t| first_node(t, &s, cur));
            }
            if it == It::None {
                if let Some(Member::Long(n)) = &mem {
                    if *n > 0 {
                        let nm = tree::with(|t| t.nodes[mynode].name.clone());
                        docref_warning(
                            vm,
                            &format!(
                                "Cannot add element {} number {} when only 0 such elements exist",
                                nm, n
                            ),
                        );
                    }
                }
            } else if let Some(Member::Long(n)) = &mem {
                let (r, c) = tree::with(|t| get_element_by_offset(t, &s, *n, cur));
                cur = r;
                cnt = c;
            } else {
                cur = None;
            }
            if let Some(c) = cur {
                return node_as_zval(vm, obj, &s, Some(c), It::None, None, nsp.as_deref(), isp);
            } else if bp == Bp::W {
                if let Some(Member::Long(n)) = &mem {
                    if cnt < *n {
                        let nm = tree::with(|t| t.nodes[mynode].name.clone());
                        docref_warning(
                            vm,
                            &format!(
                                "Cannot add element {} number {} when only {} such elements exist",
                                nm, n, cnt
                            ),
                        );
                    }
                }
                let newn = tree::with(|t| {
                    let parent = t.nodes[mynode].parent?;
                    let (ns, name) = (t.nodes[mynode].ns, t.nodes[mynode].name.clone());
                    Some(new_text_child(t, parent, ns, &name, None))
                });
                return node_as_zval(vm, obj, &s, newn, It::None, None, nsp.as_deref(), isp);
            }
            Value::Null
        }
        Some(Member::Str(name)) => {
            let exists = tree::with(|t| {
                let ch = t.nodes[node].children;
                find_element_by_name(t, &s, ch, name).is_some()
            });
            if bp != Bp::Is || exists {
                return node_as_zval(
                    vm,
                    obj,
                    &s,
                    Some(node),
                    It::Element,
                    Some(name),
                    nsp.as_deref(),
                    isp,
                );
            }
            Value::Null
        }
    }
}

/// Convert a value assigned to an element/attribute to its string content.
fn value_str(vm: &mut Vm, v: &Value, access: Access) -> Result<Option<String>, ()> {
    match v {
        Value::Long(_)
        | Value::False
        | Value::True
        | Value::Double(_)
        | Value::Null
        | Value::String(_) => Ok(Some(vm.value_to_string(v).to_string_lossy())),
        Value::Undef => Ok(Some(String::new())),
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            value_str(vm, &inner, access)
        }
        Value::Object(o)
            if o.borrow()
                .class_name
                .eq_ignore_ascii_case(b"SimpleXMLElement")
                && sxe_of(o).is_some() =>
        {
            Ok(Some(cast_string(o)))
        }
        other => {
            let tn = zend_value_name(vm, other);
            let msg = format!(
                "It's not possible to assign a complex type to {}, {} given",
                if access == Access::Attribs {
                    "attributes"
                } else {
                    "properties"
                },
                tn
            );
            throw_error(vm, b"TypeError", &msg);
            Err(())
        }
    }
}

fn zend_value_name(_vm: &Vm, v: &Value) -> String {
    match v {
        Value::Object(o) => String::from_utf8_lossy(&o.borrow().class_name).into_owned(),
        Value::Array(_) => "array".into(),
        Value::Null | Value::Undef => "null".into(),
        Value::True => "true".into(),
        Value::False => "false".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Resource(_) => "resource".into(),
        Value::Generator(_) => "Generator".into(),
        Value::Reference(r) => zend_value_name(_vm, &r.borrow()),
    }
}

/// sxe_prop_dim_write. Returns the new node when created.
fn prop_dim_write(
    vm: &mut Vm,
    obj: &Obj,
    member: Option<&Value>,
    value: Option<&Value>,
    mut access: Access,
) -> Option<NodeId> {
    let s = sxe_of(obj)?;
    let mem: Option<Member> = match member {
        None => {
            if s.itype.get() == It::AttrList {
                throw_error(vm, b"Error", "Cannot append to an attribute list");
                return None;
            }
            access = Access::Elements;
            None
        }
        Some(m) => {
            let mm = match m {
                Value::Long(n) => Member::Long(*n),
                Value::String(st) => Member::Str(st.to_string_lossy()),
                other => {
                    let st = vm.value_to_string(other);
                    if vm.current_exception.is_some() {
                        return None;
                    }
                    Member::Str(
                        st.to_string_lossy()
                            .trim_matches(|c: char| {
                                c == ' '
                                    || c == '\t'
                                    || c == '\n'
                                    || c == '\r'
                                    || c == '\0'
                                    || c == '\x0B'
                            })
                            .to_string(),
                    )
                }
            };
            match &mm {
                Member::Long(_) => {
                    if s.itype.get() != It::AttrList {
                        access = Access::Elements;
                    }
                }
                Member::Str(st) => {
                    if st.is_empty() {
                        let msg = format!(
                            "Cannot create {} with an empty name",
                            if access == Access::Attribs {
                                "attribute"
                            } else {
                                "element"
                            }
                        );
                        throw_error(vm, b"ValueError", &msg);
                        return None;
                    }
                }
            }
            Some(mm)
        }
    };
    let Some(node0) = get_node(vm, &s) else {
        // GET_NODE() only throws: the value is still checked.
        if let Some(v) = value {
            let _ = value_str(vm, v, access);
        }
        return None;
    };
    let it = s.itype.get();
    let mut attr: Option<NodeId> = None;
    let mut test = false;
    let mut node = Some(node0);
    let mut mynode = node0;
    if it == It::AttrList {
        access = Access::Attribs;
        node = tree::with(|t| first_node(t, &s, node));
        attr = node;
        test = s.nm().is_some();
    } else if it != It::Child {
        mynode = node0;
        node = tree::with(|t| first_node(t, &s, node));
        attr = node.and_then(|n| tree::with(|t| t.nodes[n].properties));
        test = false;
        if mem.is_none() {
            if let Some(n) = node {
                let pdoc = tree::with(|t| {
                    t.nodes[n]
                        .parent
                        .map(|p| t.nodes[p].t == NType::Document)
                        .unwrap_or(false)
                });
                if pdoc {
                    throw_error(vm, b"ValueError", "Cannot append to an attribute list");
                    return None;
                }
            }
        }
        if access == Access::Attribs && node.is_none() && it == It::Element {
            let nm = s.nm().unwrap_or_default();
            let created = tree::with(|t| {
                let ns = t.nodes[mynode].ns;
                new_child(t, mynode, ns, &nm, None)
            });
            node = Some(created);
            attr = None;
        }
    }
    if let Some(n) = node {
        mynode = n;
    }
    let vstr: Option<String> = match value {
        Some(v) => match value_str(vm, v, access) {
            Ok(x) => x,
            Err(()) => return None,
        },
        None => None,
    };
    let node = node?;
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    let sname = s.nm();
    let mut counter = 0;
    let mut is_attr = false;
    let mut nodendx = 0i64;
    let mut newnode: Option<NodeId> = None;
    let mut cnt = 0i64;
    let mut cursor: Option<NodeId> = Some(node);
    if access == Access::Attribs {
        let mm = mem.as_ref().unwrap();
        tree::with(|t| {
            let mut a = attr;
            match mm {
                Member::Long(idx) => {
                    while let Some(x) = a {
                        if nodendx > *idx {
                            break;
                        }
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            if nodendx == *idx {
                                is_attr = true;
                                counter += 1;
                                attr = Some(x);
                                return;
                            }
                            nodendx += 1;
                        }
                        a = t.nodes[x].next;
                    }
                    attr = None;
                }
                Member::Str(name) => {
                    while let Some(x) = a {
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && t.nodes[x].name == *name
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            is_attr = true;
                            counter += 1;
                            attr = Some(x);
                            return;
                        }
                        a = t.nodes[x].next;
                    }
                    attr = None;
                }
            }
        });
    } else {
        match &mem {
            None | Some(Member::Long(_)) => {
                if tree::with(|t| t.nodes[node].t == NType::Attribute) {
                    throw_error(vm, b"Error", "Cannot create duplicate attribute");
                    return None;
                }
                if it == It::None {
                    newnode = Some(node);
                    counter += 1;
                    if let Some(Member::Long(n)) = &mem {
                        if *n > 0 {
                            let nm = tree::with(|t| t.nodes[mynode].name.clone());
                            docref_warning(
                                vm,
                                &format!(
                                    "Cannot add element {} number {} when only 0 such elements exist",
                                    nm, n
                                ),
                            );
                            return None;
                        }
                    }
                } else if let Some(Member::Long(n)) = &mem {
                    let (r, c) = tree::with(|t| get_element_by_offset(t, &s, *n, Some(node)));
                    newnode = r;
                    cnt = c;
                    if newnode.is_some() {
                        counter += 1;
                    }
                }
            }
            Some(Member::Str(name)) => {
                tree::with(|t| {
                    let mut c = t.nodes[node].children;
                    while let Some(x) = c {
                        if t.nodes[x].t != NType::Text
                            && t.nodes[x].name == *name
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            newnode = Some(x);
                            counter += 1;
                        }
                        c = t.nodes[x].next;
                    }
                });
                cursor = None;
            }
        }
    }
    if counter == 1 {
        if is_attr {
            newnode = attr;
        }
        if let (Some(v), Some(nn)) = (&vstr, newnode) {
            tree::with(|t| {
                for c in t.children_of(nn) {
                    t.unlink(c);
                }
                t.nodes[nn].children = None;
                t.nodes[nn].last = None;
                t.node_set_content(nn, v);
            });
        }
    } else if counter > 1 {
        docref_warning(
            vm,
            "Cannot assign to an array of nodes (duplicate subnodes or attr detected)",
        );
        return None;
    } else if access == Access::Elements {
        if cursor.is_none() {
            match &mem {
                None | Some(Member::Long(_)) => {
                    newnode = tree::with(|t| {
                        let parent = t.nodes[mynode].parent?;
                        let (ns, name) = (t.nodes[mynode].ns, t.nodes[mynode].name.clone());
                        Some(new_text_child(t, parent, ns, &name, vstr.as_deref()))
                    });
                }
                Some(Member::Str(name)) => {
                    newnode = Some(tree::with(|t| {
                        new_text_child(t, mynode, None, name, vstr.as_deref())
                    }));
                }
            }
        } else if let Some(Member::Long(n)) = &mem {
            if cnt < *n {
                let nm = tree::with(|t| t.nodes[mynode].name.clone());
                docref_warning(
                    vm,
                    &format!(
                        "Cannot add element {} number {} when only {} such elements exist",
                        nm, n, cnt
                    ),
                );
            }
            newnode = tree::with(|t| {
                let parent = t.nodes[mynode].parent?;
                let (ns, name) = (t.nodes[mynode].ns, t.nodes[mynode].name.clone());
                Some(new_text_child(t, parent, ns, &name, vstr.as_deref()))
            });
        } else if mem.is_none() {
            newnode = tree::with(|t| {
                let parent = t.nodes[mynode].parent?;
                let (ns, name) = (t.nodes[mynode].ns, t.nodes[mynode].name.clone());
                Some(new_text_child(t, parent, ns, &name, vstr.as_deref()))
            });
        }
    } else {
        match mem.as_ref().unwrap() {
            Member::Long(n) => {
                docref_warning(
                    vm,
                    &format!(
                        "Cannot change attribute number {} when only {} attributes exist",
                        n, nodendx
                    ),
                );
            }
            Member::Str(name) => {
                newnode = Some(tree::with(|t| {
                    t.new_prop(node, None, name, vstr.as_deref())
                }));
            }
        }
    }
    newnode
}

/// sxe_property_get_adr: fetch for write (creates the element if missing).
fn property_get_adr(vm: &mut Vm, obj: &Obj, name: &str) -> Option<Value> {
    let s = sxe_of(obj)?;
    let node = get_node(vm, &s)?;
    let found = tree::with(|t| get_element_by_name(t, &s, Some(node), name));
    if found.is_some() {
        return None; // use read_property(W)
    }
    let mv = Value::String(PhpString::from_string(name.to_string()));
    let newn = prop_dim_write(vm, obj, Some(&mv), None, Access::Elements);
    if vm.current_exception.is_some() {
        return Some(Value::Null);
    }
    let nsp = s.nsp();
    Some(node_as_zval(
        vm,
        obj,
        &s,
        newn,
        It::None,
        None,
        nsp.as_deref(),
        s.isprefix.get(),
    ))
}

fn prop_dim_exists(
    vm: &mut Vm,
    obj: &Obj,
    member: &Value,
    check_empty: bool,
    mut access: Access,
) -> bool {
    let s = match sxe_of(obj) {
        Some(s) => s,
        None => return false,
    };
    let mm = match member {
        Value::Long(n) => Member::Long(*n),
        Value::String(st) => Member::Str(st.to_string_lossy()),
        other => match member_of(vm, other) {
            Some(m) => m,
            None => return false,
        },
    };
    let node0 = match get_node(vm, &s) {
        Some(n) => n,
        None => return false,
    };
    let it = s.itype.get();
    let mut node = Some(node0);
    if let Member::Long(_) = mm {
        if it != It::AttrList {
            access = Access::Elements;
            if it == It::Child {
                node = tree::with(|t| first_node(t, &s, node));
            }
        }
    }
    let mut attr = None;
    let mut test = false;
    if it == It::AttrList {
        access = Access::Attribs;
        node = tree::with(|t| first_node(t, &s, node));
        attr = node;
        test = s.nm().is_some();
    } else if it != It::Child {
        node = tree::with(|t| first_node(t, &s, node));
        attr = node.and_then(|n| tree::with(|t| t.nodes[n].properties));
    }
    let node = match node {
        Some(n) => n,
        None => return false,
    };
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    let sname = s.nm();
    tree::with(|t| {
        if access == Access::Attribs {
            let mut a = attr;
            let mut found = None;
            match &mm {
                Member::Long(idx) => {
                    let mut nodendx = 0;
                    while let Some(x) = a {
                        if nodendx > *idx {
                            break;
                        }
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            if nodendx == *idx {
                                found = Some(x);
                                break;
                            }
                            nodendx += 1;
                        }
                        a = t.nodes[x].next;
                    }
                }
                Member::Str(name) => {
                    while let Some(x) = a {
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && t.nodes[x].name == *name
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            found = Some(x);
                            break;
                        }
                        a = t.nodes[x].next;
                    }
                }
            }
            match found {
                None => false,
                Some(a) => {
                    if check_empty {
                        let c = t.nodes[a].children;
                        match c {
                            None => false,
                            Some(c) => {
                                let ct = &t.nodes[c].content;
                                !(ct.is_empty() || ct == "0")
                            }
                        }
                    } else {
                        true
                    }
                }
            }
        } else {
            let n = match &mm {
                Member::Long(idx) => {
                    let mut start = Some(node);
                    if it == It::Child {
                        start = first_node(t, &s, start);
                    }
                    get_element_by_offset(t, &s, *idx, start).0
                }
                Member::Str(name) => {
                    let ch = t.nodes[node].children;
                    find_element_by_name(t, &s, ch, name)
                }
            };
            match n {
                None => false,
                Some(n) => {
                    if check_empty {
                        let c = t.nodes[n].children;
                        match c {
                            None => false,
                            Some(c) => {
                                if t.nodes[c].t == NType::Text && t.nodes[c].next.is_none() {
                                    let ct = &t.nodes[c].content;
                                    !(ct.is_empty() || ct == "0")
                                } else {
                                    true
                                }
                            }
                        }
                    } else {
                        true
                    }
                }
            }
        }
    })
}

fn prop_dim_delete(vm: &mut Vm, obj: &Obj, member: &Value, mut access: Access) {
    let s = match sxe_of(obj) {
        Some(s) => s,
        None => return,
    };
    let mm = match member {
        Value::Long(n) => Member::Long(*n),
        Value::String(st) => Member::Str(st.to_string_lossy()),
        other => match member_of(vm, other) {
            Some(m) => m,
            None => return,
        },
    };
    let node0 = match get_node(vm, &s) {
        Some(n) => n,
        None => return,
    };
    let it = s.itype.get();
    let mut node = Some(node0);
    if let Member::Long(_) = mm {
        if it != It::AttrList {
            access = Access::Elements;
            if it == It::Child {
                node = tree::with(|t| first_node(t, &s, node));
            }
        }
    }
    let mut attr = None;
    let mut test = false;
    if it == It::AttrList {
        access = Access::Attribs;
        node = tree::with(|t| first_node(t, &s, node));
        attr = node;
        test = s.nm().is_some();
    } else if it != It::Child {
        node = tree::with(|t| first_node(t, &s, node));
        attr = node.and_then(|n| tree::with(|t| t.nodes[n].properties));
    }
    let node = match node {
        Some(n) => n,
        None => return,
    };
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    let sname = s.nm();
    tree::with(|t| {
        if access == Access::Attribs {
            let mut a = attr;
            match &mm {
                Member::Long(idx) => {
                    let mut nodendx = 0;
                    while let Some(x) = a {
                        if nodendx > *idx {
                            break;
                        }
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            if nodendx == *idx {
                                t.remove_id_for_attr(x);
                                t.unlink(x);
                                break;
                            }
                            nodendx += 1;
                        }
                        a = t.nodes[x].next;
                    }
                }
                Member::Str(name) => {
                    while let Some(x) = a {
                        let next = t.nodes[x].next;
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && t.nodes[x].name == *name
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            t.remove_id_for_attr(x);
                            t.unlink(x);
                            break;
                        }
                        a = next;
                    }
                }
            }
        } else {
            match &mm {
                Member::Long(idx) => {
                    let mut start = Some(node);
                    if it == It::Child {
                        start = first_node(t, &s, start);
                    }
                    if let Some(n) = get_element_by_offset(t, &s, *idx, start).0 {
                        t.unlink(n);
                    }
                }
                Member::Str(name) => {
                    let mut c = t.nodes[node].children;
                    while let Some(x) = c {
                        let next = t.nodes[x].next;
                        if t.nodes[x].t != NType::Text
                            && t.nodes[x].name == *name
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            t.unlink(x);
                        }
                        c = next;
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Properties table

fn get_base_node_value(
    vm: &mut Vm,
    obj: &Obj,
    s: &Sxe,
    node: NodeId,
    nsprefix: Option<&str>,
    isprefix: bool,
) -> Value {
    let txt = tree::with(|t| {
        let c = t.nodes[node].children;
        match c {
            Some(c) if t.nodes[c].t == NType::Text && !is_blank_node(t, c) => {
                Some(t.list_get_string(Some(c), true))
            }
            _ => None,
        }
    });
    match txt {
        Some(st) => Value::String(PhpString::from_string(st)),
        None => {
            let sub = Sxe::new();
            sub.doc.set(s.doc.get());
            if let Some(p) = nsprefix {
                if !p.is_empty() {
                    *sub.nsprefix.borrow_mut() = Some(p.to_string());
                    sub.isprefix.set(isprefix);
                }
            }
            sub.node.set(Some(node));
            let cn = obj.borrow().class_name.clone();
            Value::Object(new_sxe_object(vm, &cn, sub))
        }
    }
}

fn properties_add(rv: &mut PhpArray, name: &str, value: Value) {
    let key = ArrayKey::String(PhpString::from_string(name.to_string()));
    match rv.get(&key).cloned() {
        Some(existing) => {
            if let Value::Array(a) = &existing {
                a.borrow_mut().push(value);
            } else {
                let mut pair = PhpArray::new();
                pair.push(existing);
                pair.push(value);
                rv.set(key, Value::Array(Rc::new(RefCell::new(pair))));
            }
        }
        None => {
            rv.set(key, value);
        }
    }
}

/// sxe_get_prop_hash
fn get_prop_hash(vm: &mut Vm, obj: &Obj, is_debug: bool) -> PhpArray {
    let mut rv = PhpArray::new();
    let s = match sxe_of(obj) {
        Some(s) => s,
        None => return rv,
    };
    let node0 = match s.node.get() {
        Some(n) => n,
        None => {
            not_initialized(vm);
            return rv;
        }
    };
    let it = s.itype.get();
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    if is_debug || it != It::Child {
        let mut node = Some(node0);
        if it == It::Element {
            node = tree::with(|t| first_node(t, &s, node));
        }
        if let Some(n) = node {
            let attrs: Vec<(String, String)> = tree::with(|t| {
                let mut v = Vec::new();
                if t.nodes[n].t != NType::EntityDecl && t.nodes[n].t == NType::Element {
                    let test = s.nm().is_some() && it == It::AttrList;
                    let sname = s.nm();
                    let mut a = t.nodes[n].properties;
                    while let Some(x) = a {
                        if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                            && match_ns(t, x, nsp.as_deref(), isp)
                        {
                            let val = t.list_get_string(t.nodes[x].children, true);
                            v.push((t.nodes[x].name.clone(), val));
                        }
                        a = t.nodes[x].next;
                    }
                }
                v
            });
            if !attrs.is_empty() {
                let mut za = PhpArray::new();
                for (k, v) in attrs {
                    za.set(
                        ArrayKey::String(PhpString::from_string(k)),
                        Value::String(PhpString::from_string(v)),
                    );
                }
                properties_add(
                    &mut rv,
                    "@attributes",
                    Value::Array(Rc::new(RefCell::new(za))),
                );
            }
        }
    }
    let node = tree::with(|t| first_node(t, &s, Some(node0)));
    let node = match node {
        Some(n) => n,
        None => return rv,
    };
    if it == It::AttrList {
        return rv;
    }
    let is_attr = tree::with(|t| t.nodes[node].t == NType::Attribute);
    if is_attr {
        let v = tree::with(|t| t.list_get_string(t.nodes[node].children, true));
        rv.push(Value::String(PhpString::from_string(v)));
        return rv;
    }
    let mut use_iter = false;
    let mut cur: Option<NodeId> = Some(node);
    if it != It::Child {
        let simple = tree::with(|t| {
            let n = &t.nodes[node];
            it == It::None
                || n.children.is_none()
                || n.parent.is_none()
                || n.next.is_none()
                || t.nodes[n.children.unwrap()].next.is_some()
                || t.nodes[n.children.unwrap()].children.is_some()
                || {
                    let p = n.parent.unwrap();
                    t.nodes[p].children == t.nodes[p].last
                }
        });
        if simple {
            cur = tree::with(|t| t.nodes[node].children);
        } else {
            cur = tree::with(|t| reset_iter_node(t, &s));
            use_iter = true;
        }
    }
    while let Some(n) = cur {
        enum Act {
            Skip,
            Text(String),
            Elem(String),
        }
        let act = tree::with(|t| {
            let nd = &t.nodes[n];
            if nd.children.is_some()
                || nd.prev.is_some()
                || nd.next.is_some()
                || is_blank_node(t, n)
            {
                if nd.t == NType::Text {
                    return Act::Skip;
                }
            } else if nd.t == NType::Text {
                if !nd.content.is_empty() {
                    return Act::Text(t.list_get_string(Some(n), true));
                }
                return Act::Skip;
            }
            if nd.t == NType::Element && !match_ns(t, n, nsp.as_deref(), isp) {
                return Act::Skip;
            }
            if nd.name.is_empty()
                || matches!(nd.t, NType::Comment | NType::Pi | NType::CData) && false
            {
                return Act::Skip;
            }
            Act::Elem(nd.name.clone())
        });
        match act {
            Act::Skip => {}
            Act::Text(st) => {
                rv.push(Value::String(PhpString::from_string(st)));
            }
            Act::Elem(name) => {
                let v = get_base_node_value(vm, obj, &s, n, nsp.as_deref(), isp);
                if use_iter {
                    rv.push(v);
                } else {
                    properties_add(&mut rv, &name, v);
                }
            }
        }
        let (is_ent, next) = tree::with(|t| (t.nodes[n].t == NType::EntityDecl, t.nodes[n].next));
        if is_ent {
            break;
        }
        cur = if use_iter {
            tree::with(|t| iterator_fetch_node(t, &s, next))
        } else {
            next
        };
    }
    rv
}

/// sxe_prop_is_empty
fn prop_is_empty(s: &Sxe) -> bool {
    let node0 = match s.node.get() {
        Some(n) => n,
        None => return true,
    };
    let it = s.itype.get();
    let nsp = s.nsp();
    let isp = s.isprefix.get();
    tree::with(|t| {
        let mut node = Some(node0);
        if it == It::Element {
            node = first_node(t, s, node);
        }
        if let Some(n) = node {
            if t.nodes[n].t != NType::EntityDecl && t.nodes[n].t == NType::Element {
                let test = s.nm().is_some() && it == It::AttrList;
                let sname = s.nm();
                let mut a = t.nodes[n].properties;
                while let Some(x) = a {
                    if (!test || Some(&t.nodes[x].name) == sname.as_ref())
                        && match_ns(t, x, nsp.as_deref(), isp)
                    {
                        return false;
                    }
                    a = t.nodes[x].next;
                }
            }
        }
        let node = first_node(t, s, Some(node0));
        let node = match node {
            Some(n) => n,
            None => return true,
        };
        if it == It::AttrList {
            return true;
        }
        if t.nodes[node].t == NType::Attribute {
            return false;
        }
        let mut use_iter = false;
        let mut cur = Some(node);
        if it != It::Child {
            let n = &t.nodes[node];
            let simple = it == It::None
                || n.children.is_none()
                || n.parent.is_none()
                || t.nodes[n.children.unwrap()].next.is_some()
                || t.nodes[n.children.unwrap()].children.is_some()
                || {
                    let p = n.parent.unwrap();
                    t.nodes[p].children == t.nodes[p].last
                };
            if simple {
                cur = n.children;
            } else {
                cur = reset_iter_node(t, s);
                use_iter = true;
            }
        }
        while let Some(n) = cur {
            let nd = &t.nodes[n];
            let mut skip = false;
            if nd.children.is_some() || nd.prev.is_some() || nd.next.is_some() {
                if nd.t == NType::Text {
                    skip = true;
                }
            } else if nd.t == NType::Text {
                if !nd.content.is_empty() {
                    return false;
                }
                skip = true;
            }
            if !skip
                && !((nd.t == NType::Element && !match_ns(t, n, nsp.as_deref(), isp))
                    || nd.name.is_empty())
            {
                return false;
            }
            let next = nd.next;
            cur = if use_iter {
                iterator_fetch_node(t, s, next)
            } else {
                next
            };
        }
        true
    })
}

// ---------------------------------------------------------------------------
// Casting

fn cast_contents(s: &Sxe) -> Option<String> {
    tree::with(|t| {
        if s.itype.get() != It::None {
            let node = first_node(t, s, None);
            node.map(|n| t.list_get_string(t.nodes[n].children, true))
        } else {
            if s.node.get().is_none() {
                if let Some(d) = s.doc.get() {
                    s.node.set(t.root_element(d));
                }
            }
            let n = s.node.get()?;
            if t.nodes[n].children.is_some() {
                Some(t.list_get_string(t.nodes[n].children, true))
            } else if matches!(t.nodes[n].t, NType::Comment | NType::Pi) {
                Some(t.nodes[n].content.clone())
            } else {
                None
            }
        }
    })
}

fn cast_string(o: &Obj) -> String {
    match sxe_of(o) {
        Some(s) => cast_contents(&s).unwrap_or_default(),
        None => String::new(),
    }
}

fn h_cast(o: &Obj, kind: u8) -> Option<Value> {
    let s = sxe_of(o)?;
    if kind == 0 {
        let node = tree::with(|t| first_node(t, &s, None));
        if node.is_some() {
            return Some(Value::True);
        }
        let empty = prop_is_empty(&s);
        return Some(if empty { Value::False } else { Value::True });
    }
    let c = cast_contents(&s);
    let sv = match c {
        Some(st) => Value::String(PhpString::from_string(st)),
        None => Value::Null,
    };
    Some(match kind {
        1 => Value::Long(sv.to_long()),
        2 => Value::Double(sv.to_double()),
        _ => Value::String(sv.to_php_string()),
    })
}

fn h_compare(a: &Value, b: &Value) -> Option<i64> {
    let (Value::Object(oa), Value::Object(ob)) = (a, b) else {
        return None;
    };
    let sa = sxe_of(oa)?;
    let sb = sxe_of(ob)?;
    match (sa.node.get(), sb.node.get()) {
        (Some(x), Some(y)) => Some(if x == y { 0 } else { 1 }),
        (None, None) => Some(if sa.doc.get() == sb.doc.get() { 0 } else { 1 }),
        _ => Some(1),
    }
}

fn h_clone(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let s = match sxe_of(src) {
        Some(s) => s,
        None => return,
    };
    let c = Sxe::new();
    let node = s.node.get();
    let is_root = node
        .map(|n| tree::with(|t| t.nodes[n].parent.map(|p| t.is_doc(p)).unwrap_or(false)))
        .unwrap_or(false);
    let mut docp = s.doc.get();
    if is_root {
        docp = s.doc.get().map(|d| tree::with(|t| t.copy_doc(d, true)));
    }
    c.doc.set(docp);
    c.isprefix.set(s.isprefix.get());
    *c.name.borrow_mut() = s.nm();
    *c.nsprefix.borrow_mut() = s.nsp();
    c.itype.set(s.itype.get());
    if let Some(n) = node {
        if is_root {
            c.node
                .set(docp.and_then(|d| tree::with(|t| t.root_element(d))));
        } else {
            c.node
                .set(Some(tree::with(|t| t.copy_node(n, docp, 1, None))));
        }
    }
    let cl = dst.borrow().class_name.to_ascii_lowercase();
    c.fptr_count.set(find_fptr_count(vm, &cl));
    dst.borrow_mut().native = Some(NativeData {
        data: Rc::new(c) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

// ---------------------------------------------------------------------------
// Object handler entry points

fn h_read_property(vm: &mut Vm, o: &Obj, name: &[u8], mode: u8) -> Option<Value> {
    sxe_of(o)?;
    let nm = String::from_utf8_lossy(name).into_owned();
    if mode == 1 {
        if let Some(v) = property_get_adr(vm, o, &nm) {
            return Some(v);
        }
        if vm.current_exception.is_some() {
            return Some(Value::Null);
        }
    }
    let mv = Value::String(PhpString::from_string(nm));
    let bp = match mode {
        1 => Bp::W,
        2 => Bp::Is,
        _ => Bp::R,
    };
    Some(prop_dim_read(vm, o, Some(&mv), Access::Elements, bp))
}

/// sxe_property_get_adr() for a by-reference fetch: a missing element is
/// created; an existing one is read for write (read_property BP_VAR_W).
pub fn prop_ptr(vm: &mut Vm, o: &Obj, name: &[u8]) -> Option<Value> {
    h_read_property(vm, o, name, 1)
}

fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], v: &Value) -> bool {
    if sxe_of(o).is_none() {
        return false;
    }
    let mv = Value::String(PhpString::from_bytes(name));
    prop_dim_write(vm, o, Some(&mv), Some(v), Access::Elements);
    true
}

fn h_has_property(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    sxe_of(o)?;
    let mv = Value::String(PhpString::from_bytes(name));
    Some(prop_dim_exists(
        vm,
        o,
        &mv,
        check_empty == 1,
        Access::Elements,
    ))
}

fn h_unset_property(vm: &mut Vm, o: &Obj, name: &[u8]) -> bool {
    if sxe_of(o).is_none() {
        return false;
    }
    let mv = Value::String(PhpString::from_bytes(name));
    prop_dim_delete(vm, o, &mv, Access::Elements);
    true
}

fn h_read_dimension(vm: &mut Vm, o: &Obj, key: Option<&Value>, mode: u8) -> Option<Value> {
    sxe_of(o)?;
    let bp = match mode {
        1 => Bp::W,
        2 => Bp::Is,
        _ => Bp::R,
    };
    Some(prop_dim_read(vm, o, key, Access::Attribs, bp))
}

fn h_write_dimension(vm: &mut Vm, o: &Obj, key: Option<&Value>, v: &Value) -> bool {
    if sxe_of(o).is_none() {
        return false;
    }
    prop_dim_write(vm, o, key, Some(v), Access::Attribs);
    true
}

fn h_has_dimension(vm: &mut Vm, o: &Obj, key: &Value, check_empty: u8) -> Option<bool> {
    sxe_of(o)?;
    Some(prop_dim_exists(
        vm,
        o,
        key,
        check_empty == 1,
        Access::Attribs,
    ))
}

fn h_unset_dimension(vm: &mut Vm, o: &Obj, key: &Value) -> bool {
    if sxe_of(o).is_none() {
        return false;
    }
    prop_dim_delete(vm, o, key, Access::Attribs);
    true
}

fn h_get_properties(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    sxe_of(o)?;
    Some(get_prop_hash(vm, o, purpose == 0))
}

// ---------------------------------------------------------------------------
// Iteration

fn iterator_fetch(vm: &mut Vm, obj: &Obj, s: &Sxe, node: Option<NodeId>) -> Option<NodeId> {
    let n = tree::with(|t| iterator_fetch_node(t, s, node));
    if let Some(x) = n {
        let nsp = s.nsp();
        let v = node_as_zval(
            vm,
            obj,
            s,
            Some(x),
            It::None,
            None,
            nsp.as_deref(),
            s.isprefix.get(),
        );
        *s.data.borrow_mut() = Some(v);
    }
    n
}

fn rewind(vm: &mut Vm, obj: &Obj, s: &Sxe) {
    *s.data.borrow_mut() = None;
    let node = match get_node(vm, s) {
        Some(n) => n,
        None => return,
    };
    let start = tree::with(|t| match s.itype.get() {
        It::Element | It::Child | It::None => t.nodes[node].children,
        It::AttrList => t.nodes[node].properties,
    });
    iterator_fetch(vm, obj, s, start);
}

fn move_forward(vm: &mut Vm, obj: &Obj, s: &Sxe) {
    let cur = s.data.borrow_mut().take();
    let mut node = None;
    if let Some(Value::Object(c)) = &cur {
        if let Some(cs) = sxe_of(c) {
            node = get_node(vm, &cs);
        }
    }
    if let Some(n) = node {
        let next = tree::with(|t| t.nodes[n].next);
        iterator_fetch(vm, obj, s, next);
    }
}

fn count_elements(s: &Sxe) -> i64 {
    tree::with(|t| {
        let mut count = 0;
        let mut node = reset_iter_node(t, s);
        while let Some(n) = node {
            count += 1;
            node = iterator_fetch_node(t, s, t.nodes[n].next);
        }
        count
    })
}

// ---------------------------------------------------------------------------
// Loading

fn read_memory(
    vm: &mut Vm,
    fname: &str,
    data: &[u8],
    options: i64,
    url: Option<&str>,
) -> Option<NodeId> {
    read_memory_enc(vm, fname, data, options, url, None)
}

fn read_memory_enc(
    vm: &mut Vm,
    fname: &str,
    data: &[u8],
    options: i64,
    url: Option<&str>,
    encoding: Option<&str>,
) -> Option<NodeId> {
    crate::parser::sync_open_basedir(vm);
    let out = sax2::parse_doc_enc(data, url, options, None, encoding);
    libxml::report(vm, fname, &out.errors, libxml::Style::Generic);
    if out.well_formed || (options & XML_PARSE_RECOVER != 0 && out.doc.is_some()) {
        out.doc
    } else {
        None
    }
}

fn read_file(vm: &mut Vm, fname: &str, filename: &str, options: i64) -> Option<NodeId> {
    let data =
        crate::dom::with_active_fn(fname.to_string(), || crate::io::read_resource(vm, filename));
    match data {
        Some(data) => {
            let url = crate::io::resource_url(filename);
            let enc = crate::io::take_transport_charset();
            read_memory_enc(vm, fname, &data, options, Some(&url), enc.as_deref())
        }
        None if crate::io::last_failure_reported() => None,
        None => {
            let msg = format!(
                "I/O warning : failed to load \"{}\": No such file or directory",
                filename
            );
            let e = parser::XmlError::simple(
                parser::FROM_IO,
                1549,
                parser::ERR_WARNING,
                format!(
                    "failed to load \"{}\": No such file or directory\n",
                    filename
                ),
            );
            let mut e = e;
            e.formatted = vec![msg];
            libxml::report(vm, fname, &[e], libxml::Style::Generic);
            None
        }
    }
}

fn arg_str(vm: &mut Vm, v: Option<&Value>) -> Vec<u8> {
    match v {
        None => Vec::new(),
        Some(v) => vm.value_to_string(v).as_bytes().to_vec(),
    }
}

/// Resolve the class name argument (`$class_name`); returns None after
/// throwing for invalid classes.
fn resolve_class_arg(vm: &mut Vm, fname: &str, argn: usize, v: Option<&Value>) -> Option<Vec<u8>> {
    match v {
        None | Some(Value::Null) | Some(Value::Undef) => Some(b"SimpleXMLElement".to_vec()),
        Some(v) => {
            let name = vm.value_to_string(v).as_bytes().to_vec();
            let name = if name.starts_with(b"\\") {
                name[1..].to_vec()
            } else {
                name
            };
            let lower = name.to_ascii_lowercase();
            if !is_sxe_class(vm, &lower) && lower != b"simplexmlelement" {
                let msg = format!(
                    "{}(): Argument #{} ($class_name) must be a class name derived from SimpleXMLElement, {} given",
                    fname,
                    argn,
                    String::from_utf8_lossy(&name)
                );
                if lower != b"simplexmliterator" {
                    throw_error(vm, b"TypeError", &msg);
                    return None;
                }
            }
            Some(name)
        }
    }
}

fn simplexml_load_string(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = arg_str(vm, args.first());
    let class = match resolve_class_arg(vm, "simplexml_load_string", 2, args.get(1)) {
        Some(c) => c,
        None => return Ok(Value::Null),
    };
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let ns = args
        .get(3)
        .map(|v| vm.value_to_string(v).to_string_lossy())
        .unwrap_or_default();
    let isprefix = args.get(4).map(|v| v.is_truthy()).unwrap_or(false);
    if options > i32::MAX as i64 || options < i32::MIN as i64 {
        throw_error(
            vm,
            b"ValueError",
            "simplexml_load_string(): Argument #3 ($options) is too large",
        );
        return Ok(Value::Null);
    }
    let doc = match read_memory(vm, "simplexml_load_string", &data, options, None) {
        Some(d) => d,
        None => return Ok(Value::False),
    };
    Ok(make_root(vm, &class, doc, &ns, isprefix))
}

fn make_root(vm: &mut Vm, class: &[u8], doc: NodeId, ns: &str, isprefix: bool) -> Value {
    let s = Sxe::new();
    s.doc.set(Some(doc));
    if !ns.is_empty() {
        *s.nsprefix.borrow_mut() = Some(ns.to_string());
    }
    s.isprefix.set(isprefix);
    s.node.set(tree::with(|t| t.root_element(doc)));
    Value::Object(new_sxe_object(vm, class, s))
}

fn simplexml_load_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = arg_str(vm, args.first());
    if filename.contains(&0) {
        throw_error(
            vm,
            b"ValueError",
            "simplexml_load_file(): Argument #1 ($filename) must not contain any null bytes",
        );
        return Ok(Value::Null);
    }
    let class = match resolve_class_arg(vm, "simplexml_load_file", 2, args.get(1)) {
        Some(c) => c,
        None => return Ok(Value::Null),
    };
    let options = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let ns = args
        .get(3)
        .map(|v| vm.value_to_string(v).to_string_lossy())
        .unwrap_or_default();
    let isprefix = args.get(4).map(|v| v.is_truthy()).unwrap_or(false);
    let fname = String::from_utf8_lossy(&filename).into_owned();
    let doc = match read_file(vm, "simplexml_load_file", &fname, options) {
        Some(d) => d,
        None => return Ok(Value::False),
    };
    Ok(make_root(vm, &class, doc, &ns, isprefix))
}

fn simplexml_import_dom(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let node = match args.first() {
        Some(Value::Object(o)) => crate::dom_node_of(o),
        _ => None,
    };
    let class = match resolve_class_arg(vm, "simplexml_import_dom", 2, args.get(1)) {
        Some(c) => c,
        None => return Ok(Value::Null),
    };
    let nodep = match node {
        Some(n) => n,
        None => {
            throw_error(
                vm,
                b"TypeError",
                "simplexml_import_dom(): Argument #1 ($node) must be a valid XML node",
            );
            return Ok(Value::Null);
        }
    };
    let (doc, mut n) = tree::with(|t| (t.nodes[nodep].doc, nodep));
    let doc = match doc {
        Some(d) => d,
        None => {
            vm.emit_warning("simplexml_import_dom(): Imported Node must have associated Document");
            return Ok(Value::Null);
        }
    };
    if tree::with(|t| t.is_doc(n)) {
        match tree::with(|t| t.root_element(n)) {
            Some(r) => n = r,
            None => {
                vm.emit_warning("simplexml_import_dom(): Invalid Nodetype to import");
                return Ok(Value::Null);
            }
        }
    }
    if tree::with(|t| t.nodes[n].t != NType::Element) {
        vm.emit_warning("simplexml_import_dom(): Invalid Nodetype to import");
        return Ok(Value::Null);
    }
    let s = Sxe::new();
    s.doc.set(Some(doc));
    s.node.set(Some(n));
    Ok(Value::Object(new_sxe_object(vm, &class, s)))
}

/// create_object: every SimpleXMLElement starts uninitialized.
fn create_object(vm: &mut Vm, o: &Obj) {
    let s = Sxe::new();
    let cl = o.borrow().class_name.to_ascii_lowercase();
    s.fptr_count.set(find_fptr_count(vm, &cl));
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(s) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

/// Native constructor `new SimpleXMLElement(...)`: runs in its own frame
/// (SimpleXMLElement->__construct() in backtraces).
fn construct(vm: &mut Vm, args: &[Value], line: u32) -> Result<(), String> {
    if !matches!(args.first(), Some(Value::Object(_))) {
        return Ok(());
    }
    vm.call_stack.push((
        "SimpleXMLElement::__construct".to_string(),
        vm.current_file.clone(),
        vm.current_line,
        args[1..].to_vec(),
        true,
    ));
    let r = construct_inner(vm, args, line);
    vm.call_stack.pop();
    r
}

fn construct_inner(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let a = &args[1..];
    if a.is_empty() {
        let msg = "SimpleXMLElement::__construct() expects at least 1 argument, 0 given";
        throw_error(vm, b"ArgumentCountError", msg);
        return Err(msg.into());
    }
    let data = arg_str(vm, a.first());
    let options = a.get(1).map(|v| v.to_long()).unwrap_or(0);
    let is_url = a.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let ns = a
        .get(3)
        .map(|v| vm.value_to_string(v).to_string_lossy())
        .unwrap_or_default();
    let isprefix = a.get(4).map(|v| v.is_truthy()).unwrap_or(false);
    if options > i32::MAX as i64 || options < i32::MIN as i64 {
        let msg = "SimpleXMLElement::__construct(): Argument #2 ($options) is invalid";
        throw_error(vm, b"Exception", msg);
        return Err(msg.into());
    }
    let doc = if is_url {
        let f = String::from_utf8_lossy(&data).into_owned();
        read_file(vm, "SimpleXMLElement::__construct", &f, options)
    } else {
        read_memory(vm, "SimpleXMLElement::__construct", &data, options, None)
    };
    let doc = match doc {
        Some(d) => d,
        None => {
            let msg = "String could not be parsed as XML";
            throw_error(vm, b"Exception", msg);
            return Err(msg.into());
        }
    };
    let s = Sxe::new();
    s.doc.set(Some(doc));
    if !ns.is_empty() {
        *s.nsprefix.borrow_mut() = Some(ns);
    }
    s.isprefix.set(isprefix);
    s.node.set(tree::with(|t| t.root_element(doc)));
    let cl = this.borrow().class_name.to_ascii_lowercase();
    s.fptr_count.set(find_fptr_count(vm, &cl));
    this.borrow_mut().native = Some(NativeData {
        data: Rc::new(s) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Methods

const METHODS: &[&str] = &[
    "xpath",
    "registerxpathnamespace",
    "asxml",
    "savexml",
    "getnamespaces",
    "getdocnamespaces",
    "children",
    "attributes",
    "addchild",
    "addattribute",
    "getname",
    "__tostring",
    "count",
    "rewind",
    "valid",
    "current",
    "key",
    "next",
    "haschildren",
    "getchildren",
    "__debuginfo",
];

fn has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    if is_static {
        return false;
    }
    let m = String::from_utf8_lossy(method);
    METHODS.contains(&m.as_ref())
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match method {
        b"__construct" => &[
            "data",
            "options",
            "dataIsURL",
            "namespaceOrPrefix",
            "isPrefix",
        ],
        b"xpath" => &["expression"],
        b"registerxpathnamespace" => &["prefix", "namespace"],
        b"asxml" | b"savexml" => &["filename"],
        b"getnamespaces" => &["recursive"],
        b"getdocnamespaces" => &["recursive", "fromRoot"],
        b"children" | b"attributes" => &["namespaceOrPrefix", "isPrefix"],
        b"addchild" => &["qualifiedName", "value", "namespace"],
        b"addattribute" => &["qualifiedName", "value", "namespace"],
        _ => &[],
    })
}

fn opt_str(vm: &mut Vm, v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(v) => Some(vm.value_to_string(v).to_string_lossy()),
    }
}

fn split_qname(q: &str) -> (Option<String>, String) {
    // xmlSplitQName2: returns None (no prefix) if no colon, or colon first/last
    match q.find(':') {
        Some(i) if i > 0 && i + 1 < q.len() => (Some(q[..i].to_string()), q[i + 1..].to_string()),
        _ => (None, q.to_string()),
    }
}

fn add_ns(arr: &mut PhpArray, prefix: Option<&str>, href: &str) {
    let k = ArrayKey::String(PhpString::from_string(prefix.unwrap_or("").to_string()));
    if arr.get(&k).is_none() {
        arr.set(k, Value::String(PhpString::from_string(href.to_string())));
    }
}

fn add_namespaces(t: &Tree, node: NodeId, recursive: bool, arr: &mut PhpArray) {
    if let Some(ns) = t.nodes[node].ns {
        add_ns(arr, t.nss[ns].prefix.as_deref(), &t.nss[ns].href);
    }
    let mut a = t.nodes[node].properties;
    while let Some(x) = a {
        if let Some(ns) = t.nodes[x].ns {
            add_ns(arr, t.nss[ns].prefix.as_deref(), &t.nss[ns].href);
        }
        a = t.nodes[x].next;
    }
    if recursive {
        let mut c = t.nodes[node].children;
        while let Some(x) = c {
            if t.nodes[x].t == NType::Element {
                add_namespaces(t, x, recursive, arr);
            }
            c = t.nodes[x].next;
        }
    }
}

/// sxe_add_registered_namespaces(): with `xmlns_attrs` (modern documents)
/// attributes in the xmlns namespace count as declarations too.
fn add_registered_namespaces(
    t: &Tree,
    node: NodeId,
    recursive: bool,
    xmlns_attrs: bool,
    arr: &mut PhpArray,
) {
    if t.nodes[node].t == NType::Element {
        for &ns in &t.nodes[node].ns_def {
            add_ns(arr, t.nss[ns].prefix.as_deref(), &t.nss[ns].href);
        }
        if xmlns_attrs {
            for a in t.attrs_of(node) {
                if let Some(ns) = t.nodes[a].ns {
                    if t.nss[ns].href == "http://www.w3.org/2000/xmlns/" {
                        let prefix = if t.nss[ns].prefix.is_some() {
                            t.nodes[a].name.clone()
                        } else {
                            String::new()
                        };
                        add_ns(arr, Some(&prefix), &t.node_get_content(a));
                    }
                }
            }
        }
        if recursive {
            let mut c = t.nodes[node].children;
            while let Some(x) = c {
                add_registered_namespaces(t, x, recursive, xmlns_attrs, arr);
                c = t.nodes[x].next;
            }
        }
    }
}

fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let obj = match this {
        Some(Value::Object(o)) => o.clone(),
        _ => return Value::Null,
    };
    let mname = String::from_utf8_lossy(method).to_ascii_lowercase();
    let s = match sxe_of(&obj) {
        Some(s) => s,
        None => {
            // object constructed without data (e.g. newInstanceWithoutConstructor)
            match mname.as_str() {
                "__tostring" => return Value::String(PhpString::empty()),
                "count" => return Value::Long(0),
                "valid" | "haschildren" => return Value::False,
                "getnamespaces" | "getdocnamespaces" | "xpath" => {
                    not_initialized(vm);
                    return Value::Null;
                }
                _ => {
                    not_initialized(vm);
                    return Value::Null;
                }
            }
        }
    };
    match mname.as_str() {
        "__tostring" => Value::String(PhpString::from_string(
            cast_contents(&s).unwrap_or_default(),
        )),
        "count" => {
            if get_node(vm, &s).is_none() {
                return Value::Null;
            }
            Value::Long(count_elements(&s))
        }
        "getname" => {
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            let name = tree::with(|t| first_node(t, &s, Some(n)).map(|x| t.nodes[x].name.clone()));
            Value::String(PhpString::from_string(name.unwrap_or_default()))
        }
        "children" | "attributes" => {
            let nsprefix = opt_str(vm, args.first());
            let isprefix = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            if mname == "children" && s.itype.get() == It::AttrList {
                return Value::Null;
            }
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            let node = tree::with(|t| first_node(t, &s, Some(n)));
            let node = match node {
                Some(x) => x,
                None => return Value::Null,
            };
            if mname == "attributes" && s.itype.get() == It::AttrList {
                return Value::Null;
            }
            let it = if mname == "children" {
                It::Child
            } else {
                It::AttrList
            };
            node_as_zval(
                vm,
                &obj,
                &s,
                Some(node),
                it,
                None,
                nsprefix.as_deref(),
                isprefix,
            )
        }
        "addchild" => {
            let qname = opt_str(vm, args.first()).unwrap_or_default();
            let value = opt_str(vm, args.get(1));
            let nsuri = opt_str(vm, args.get(2));
            if qname.is_empty() {
                throw_error(
                    vm,
                    b"ValueError",
                    "SimpleXMLElement::addChild(): Argument #1 ($qualifiedName) must not be empty",
                );
                return Value::Null;
            }
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            if s.itype.get() == It::AttrList {
                vm.emit_warning("SimpleXMLElement::addChild(): Cannot add element to attributes");
                return Value::Null;
            }
            let node = tree::with(|t| first_node(t, &s, Some(n)));
            let node = match node {
                Some(x) => x,
                None => {
                    vm.emit_warning("SimpleXMLElement::addChild(): Cannot add child. Parent is not a permanent member of the XML tree");
                    return Value::Null;
                }
            };
            let (prefix, localname) = split_qname(&qname);
            let newnode = tree::with(|t| {
                let nn = new_child(t, node, None, &localname, value.as_deref());
                if let Some(u) = &nsuri {
                    if u.is_empty() {
                        t.nodes[nn].ns = None;
                        t.new_ns(Some(nn), u, prefix.as_deref());
                    } else {
                        let found = t.search_ns_by_href(node, u);
                        let ns = match found {
                            Some(x) => Some(x),
                            None => t.new_ns(Some(nn), u, prefix.as_deref()),
                        };
                        t.nodes[nn].ns = ns;
                    }
                }
                nn
            });
            node_as_zval(
                vm,
                &obj,
                &s,
                Some(newnode),
                It::None,
                Some(&localname),
                prefix.as_deref(),
                false,
            )
        }
        "addattribute" => {
            if args.len() < 2 {
                let msg = format!(
                    "SimpleXMLElement::addAttribute() expects at least 2 arguments, {} given",
                    args.len()
                );
                throw_error(vm, b"ArgumentCountError", &msg);
                return Value::Null;
            }
            let qname = opt_str(vm, args.first()).unwrap_or_default();
            let value = opt_str(vm, args.get(1)).unwrap_or_default();
            let nsuri = opt_str(vm, args.get(2));
            if qname.is_empty() {
                throw_error(
                    vm,
                    b"ValueError",
                    "SimpleXMLElement::addAttribute(): Argument #1 ($qualifiedName) must not be empty",
                );
                return Value::Null;
            }
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            let node = tree::with(|t| {
                let mut x = first_node(t, &s, Some(n));
                if let Some(y) = x {
                    if t.nodes[y].t != NType::Element {
                        x = t.nodes[y].parent;
                    }
                }
                x
            });
            let node = match node {
                Some(x) => x,
                None => {
                    vm.emit_warning(
                        "SimpleXMLElement::addAttribute(): Unable to locate parent Element",
                    );
                    return Value::Null;
                }
            };
            let (prefix, localname) = split_qname(&qname);
            if prefix.is_none() && nsuri.as_deref().map(|u| !u.is_empty()).unwrap_or(false) {
                vm.emit_warning(
                    "SimpleXMLElement::addAttribute(): Attribute requires prefix for namespace",
                );
                return Value::Null;
            }
            let exists =
                tree::with(|t| t.has_ns_prop(node, &localname, nsuri.as_deref()).is_some());
            if exists {
                vm.emit_warning("SimpleXMLElement::addAttribute(): Attribute already exists");
                return Value::Null;
            }
            tree::with(|t| {
                let ns = match &nsuri {
                    Some(u) => match t.search_ns_by_href(node, u) {
                        Some(x) => Some(x),
                        None => t.new_ns(Some(node), u, prefix.as_deref()),
                    },
                    None => None,
                };
                let a = t.new_prop(node, ns, &localname, Some(&value));
                // xmlNewNsProp creates a text child even for empty values
                if t.nodes[a].children.is_none() {
                    let doc = t.nodes[a].doc;
                    let tx = t.new_text(doc, "");
                    t.link_child_raw(a, tx);
                }
            });
            Value::Null
        }
        "getnamespaces" => {
            let recursive = args.first().map(|v| v.is_truthy()).unwrap_or(false);
            let mut arr = PhpArray::new();
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            tree::with(|t| {
                if let Some(node) = first_node(t, &s, Some(n)) {
                    if t.nodes[node].t == NType::Element {
                        add_namespaces(t, node, recursive, &mut arr);
                    } else if t.nodes[node].t == NType::Attribute {
                        if let Some(ns) = t.nodes[node].ns {
                            add_ns(&mut arr, t.nss[ns].prefix.as_deref(), &t.nss[ns].href);
                        }
                    }
                }
            });
            arr_val(arr)
        }
        "getdocnamespaces" => {
            let recursive = args.first().map(|v| v.is_truthy()).unwrap_or(false);
            let from_root = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
            let node = if from_root {
                match s.doc.get() {
                    Some(d) => tree::with(|t| t.root_element(d)),
                    None => {
                        not_initialized(vm);
                        return Value::Null;
                    }
                }
            } else {
                match get_node(vm, &s) {
                    Some(n) => Some(n),
                    None => return Value::Null,
                }
            };
            let node = match node {
                Some(n) => n,
                None => return Value::False,
            };
            // Only do this for modern documents to keep BC.
            let modern = crate::dom::modern::is_modern_doc(s.doc.get());
            let mut arr = PhpArray::new();
            tree::with(|t| add_registered_namespaces(t, node, recursive, modern, &mut arr));
            arr_val(arr)
        }
        "asxml" | "savexml" => {
            let filename = opt_str(vm, args.first());
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            let node = match tree::with(|t| first_node(t, &s, Some(n))) {
                Some(x) => x,
                None => return Value::False,
            };
            let modern = crate::dom::modern::is_modern_doc(s.doc.get());
            let bytes = tree::with(|t| {
                let doc = s.doc.get();
                let enc = doc.and_then(|d| t.doc_info(d).and_then(|i| i.encoding.clone()));
                let is_root = t.nodes[node]
                    .parent
                    .map(|p| t.nodes[p].t == NType::Document)
                    .unwrap_or(false);
                if modern {
                    // the document handlers of Dom\XMLDocument: the spec serializer
                    let target = match (is_root, doc) {
                        (true, Some(d)) => d,
                        _ => node,
                    };
                    crate::dom::modern::serializer::dump_for_handlers(t, target, enc.as_deref())
                } else if let (true, Some(d)) = (is_root, doc) {
                    serialize::doc_to_bytes(t, d, enc.as_deref(), false, false, false)
                } else {
                    serialize::node_to_bytes(
                        t,
                        node,
                        if filename.is_some() {
                            None
                        } else {
                            enc.as_deref()
                        },
                        false,
                        false,
                    )
                }
            });
            match filename {
                Some(f) => {
                    if f.contains('\0') {
                        throw_error(
                            vm,
                            b"ValueError",
                            &format!(
                                "SimpleXMLElement::{}(): Argument #1 ($filename) must not contain any null bytes",
                                if mname == "asxml" { "asXML" } else { "saveXML" }
                            ),
                        );
                        return Value::Null;
                    }
                    let fname = format!(
                        "SimpleXMLElement::{}",
                        if mname == "asxml" { "asXML" } else { "saveXML" }
                    );
                    if crate::dom::with_active_fn(fname, || {
                        crate::io::write_resource(vm, &f, &bytes)
                    }) {
                        Value::True
                    } else {
                        Value::False
                    }
                }
                None => Value::String(PhpString::from_vec(bytes)),
            }
        }
        "xpath" => {
            let expr = opt_str(vm, args.first()).unwrap_or_default();
            if s.itype.get() == It::AttrList {
                return Value::Null;
            }
            let n = match get_node(vm, &s) {
                Some(n) => n,
                None => return Value::Null,
            };
            let node = match tree::with(|t| first_node(t, &s, Some(n))) {
                Some(x) => x,
                None => return Value::Null,
            };
            let mut nsmap: Vec<(String, String)> = tree::with(|t| crate::xpath::ns_list(t, node));
            for (p, u) in s.xpath_ns.borrow().iter() {
                nsmap.push((p.clone(), u.clone()));
            }
            let res = crate::xpath::eval(&expr, node, &nsmap, &[]);
            match res {
                Err(e) => {
                    crate::xpath::report_error(vm, "SimpleXMLElement::xpath", &e, &expr);
                    Value::False
                }
                Ok(crate::xpath::XValue::NodeSet(nodes)) => {
                    let mut arr = PhpArray::new();
                    for x in nodes {
                        let (t_, parent, name, href) = tree::with(|t| {
                            let nd = &t.nodes[x];
                            (
                                nd.t,
                                nd.parent,
                                nd.name.clone(),
                                t.ns_href(nd.ns).map(|h| h.to_string()),
                            )
                        });
                        match t_ {
                            NType::Text => {
                                let v =
                                    node_as_zval(vm, &obj, &s, parent, It::None, None, None, false);
                                arr.push(v);
                            }
                            NType::Attribute => {
                                let v = node_as_zval(
                                    vm,
                                    &obj,
                                    &s,
                                    parent,
                                    It::AttrList,
                                    Some(&name),
                                    href.as_deref(),
                                    false,
                                );
                                arr.push(v);
                            }
                            NType::Element | NType::Pi | NType::Comment => {
                                let v = node_as_zval(
                                    vm,
                                    &obj,
                                    &s,
                                    Some(x),
                                    It::None,
                                    None,
                                    None,
                                    false,
                                );
                                arr.push(v);
                            }
                            _ => {}
                        }
                    }
                    arr_val(arr)
                }
                Ok(other) => {
                    vm.emit_warning(&format!("SimpleXMLElement::xpath(): XPath expression must return a node set, {} returned", other.type_name()));
                    Value::False
                }
            }
        }
        "registerxpathnamespace" => {
            let prefix = opt_str(vm, args.first()).unwrap_or_default();
            let uri = opt_str(vm, args.get(1)).unwrap_or_default();
            if s.doc.get().is_none() {
                not_initialized(vm);
                return Value::Null;
            }
            let mut v = s.xpath_ns.borrow_mut();
            v.retain(|(p, _)| *p != prefix);
            v.push((prefix, uri));
            Value::True
        }
        "rewind" => {
            rewind(vm, &obj, &s);
            Value::Null
        }
        "valid" => {
            if s.data.borrow().is_some() {
                Value::True
            } else {
                Value::False
            }
        }
        "current" => {
            let d = s.data.borrow().clone();
            match d {
                Some(v) => v,
                None => {
                    throw_error(vm, b"Error", "Iterator not initialized or already consumed");
                    Value::Null
                }
            }
        }
        "key" => {
            let d = s.data.borrow().clone();
            match d {
                Some(Value::Object(c)) => {
                    let n = sxe_of(&c).and_then(|cs| cs.node.get());
                    match n {
                        Some(n) => Value::String(PhpString::from_string(tree::with(|t| {
                            t.nodes[n].name.clone()
                        }))),
                        None => {
                            throw_error(
                                vm,
                                b"Error",
                                "Iterator not initialized or already consumed",
                            );
                            Value::Null
                        }
                    }
                }
                _ => {
                    throw_error(vm, b"Error", "Iterator not initialized or already consumed");
                    Value::Null
                }
            }
        }
        "next" => {
            move_forward(vm, &obj, &s);
            Value::Null
        }
        "haschildren" => {
            let d = s.data.borrow().clone();
            if d.is_none() || s.itype.get() == It::AttrList {
                return Value::False;
            }
            if let Some(Value::Object(c)) = d {
                if let Some(cs) = sxe_of(&c) {
                    let n = match get_node(vm, &cs) {
                        Some(n) => n,
                        None => return Value::Null,
                    };
                    let has = tree::with(|t| {
                        let mut x = t.nodes[n].children;
                        while let Some(y) = x {
                            if t.nodes[y].t == NType::Element {
                                return true;
                            }
                            x = t.nodes[y].next;
                        }
                        false
                    });
                    return if has { Value::True } else { Value::False };
                }
            }
            Value::False
        }
        "getchildren" => {
            let d = s.data.borrow().clone();
            if d.is_none() || s.itype.get() == It::AttrList {
                return Value::Null;
            }
            d.unwrap()
        }
        "__debuginfo" => arr_val(get_prop_hash(vm, &obj, true)),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Registration

pub fn register(vm: &mut Vm) {
    vm.register_function_with_params(
        b"simplexml_load_string",
        simplexml_load_string,
        &[
            b"data",
            b"class_name",
            b"options",
            b"namespace_or_prefix",
            b"is_prefix",
        ],
    );
    vm.register_function_with_params(
        b"simplexml_load_file",
        simplexml_load_file,
        &[
            b"filename",
            b"class_name",
            b"options",
            b"namespace_or_prefix",
            b"is_prefix",
        ],
    );
    vm.register_function_with_params(
        b"simplexml_import_dom",
        simplexml_import_dom,
        &[b"node", b"class_name"],
    );
    vm.register_extension(b"simplexml");
    vm.prop_ptr_handlers
        .insert(b"simplexmlelement".to_vec(), prop_ptr);

    let mut ce = ClassEntry::new(b"SimpleXMLElement".to_vec());
    ce.interfaces = vec![
        b"Stringable".to_vec(),
        b"Countable".to_vec(),
        b"RecursiveIterator".to_vec(),
        b"Traversable".to_vec(),
        b"Iterator".to_vec(),
    ];
    vm.classes.insert(b"simplexmlelement".to_vec(), ce);
    let mut ce = ClassEntry::new(b"SimpleXMLIterator".to_vec());
    ce.parent = Some(b"SimpleXMLElement".to_vec());
    vm.classes.insert(b"simplexmliterator".to_vec(), ce);

    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    vm.native_class_handlers
        .insert(b"simplexmlelement".to_vec(), h);
    vm.native_class_handlers
        .insert(b"simplexmliterator".to_vec(), h);
    vm.object_construct_handlers
        .insert(b"simplexmlelement".to_vec(), construct);
    vm.object_construct_handlers
        .insert(b"simplexmliterator".to_vec(), construct);
    vm.object_create_handlers
        .insert(b"simplexmlelement".to_vec(), create_object);
}
