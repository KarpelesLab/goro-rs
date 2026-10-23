//! php_mailparse_mime.c: the MIME part tree and the incremental parser.
//!
//! Every part is a "mailparse_mail_structure" resource; a part holds its
//! children's resources, so a child lives as long as its parent or a PHP
//! reference to it.
//!
//! Unlike the C code, the parser does not drop the last line of a message
//! that has no line break (nor the last header line of a message without
//! body): before the tree is inspected, the pending data is processed as
//! the end of the message ("finalize"). That is undone if more data is
//! parsed later, so incremental parsing gives the same result as parsing
//! the whole message at once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::rfc822::{self, Tokenized};

pub const LE_NAME: &[u8] = b"mailparse_mail_structure";

const MAXLEVELS: usize = 20;
const MAXPARTS: usize = 300;

/// A header with attributes (Content-Type, Content-Disposition).
#[derive(Clone, Default, Debug)]
pub struct HeaderAttrs {
    pub value: Vec<u8>,
    /// In insertion order; a later value for the same name replaces the
    /// earlier one in place.
    pub attributes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl HeaderAttrs {
    fn new(value: &[u8]) -> HeaderAttrs {
        HeaderAttrs {
            value: value.to_vec(),
            attributes: Vec::new(),
        }
    }

    fn set(&mut self, name: &[u8], value: &[u8]) {
        match self.attributes.iter_mut().find(|(k, _)| k == name) {
            Some(e) => e.1 = value.to_vec(),
            None => self.attributes.push((name.to_vec(), value.to_vec())),
        }
    }

    /// php_mimepart_attribute_get()
    pub fn get(&self, name: &[u8]) -> Option<&[u8]> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_slice())
    }
}

/// Where the message text of a MimeMessage comes from.
#[derive(Clone, Default)]
pub enum Source {
    #[default]
    None,
    Str(PhpString),
    Stream(Value),
}

#[derive(Clone, Default)]
pub struct Part {
    pub parent: Option<i64>,
    /// The children's resources.
    pub children: Vec<Value>,
    pub source: Source,

    pub startpos: i64,
    pub endpos: i64,
    pub bodystart: i64,
    pub bodyend: i64,
    pub nlines: i64,
    pub nbodylines: i64,

    pub mime_version: Option<Vec<u8>>,
    pub content_transfer_encoding: Option<Vec<u8>>,
    pub content_location: Option<Vec<u8>>,
    pub content_base: Option<Vec<u8>>,
    pub boundary: Option<Vec<u8>>,
    pub charset: Option<Vec<u8>>,
    pub content_type: Option<HeaderAttrs>,
    pub content_disposition: Option<HeaderAttrs>,

    pub headers: PhpArray,

    // parse state
    pub in_header: bool,
    pub is_dummy: bool,
    pub completed: bool,
    pub workbuf: Vec<u8>,
    pub headerbuf: Vec<u8>,
    pub lastpart: Option<i64>,
    /// php_mimepart_parse() was called on this part.
    pub parse_root: bool,
    /// The last php_mimepart_parse() call failed.
    pub parse_failed: bool,
}

pub type PartRef = Rc<RefCell<Part>>;

thread_local! {
    /// The live parts by resource id (the resource list entries).
    static PARTS: RefCell<HashMap<i64, PartRef>> = RefCell::new(HashMap::new());
    /// Trees whose pending data was processed as the end of the message,
    /// by top part: the state of their parts before that.
    static FINALIZED: RefCell<HashMap<i64, Vec<(i64, Part)>>> = RefCell::new(HashMap::new());
}

pub fn get(id: i64) -> Option<PartRef> {
    PARTS.with(|p| p.borrow().get(&id).cloned())
}

fn with<T>(id: i64, f: impl FnOnce(&mut Part) -> T) -> Option<T> {
    let p = get(id)?;
    let mut b = p.borrow_mut();
    Some(f(&mut b))
}

fn read<T>(id: i64, f: impl FnOnce(&Part) -> T) -> Option<T> {
    let p = get(id)?;
    let b = p.borrow();
    Some(f(&b))
}

/// The id of a part resource value (open and of our type).
pub fn part_id(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Resource(r)
            if goro_core::value::is_open_resource(r.id())
                && goro_core::value::resource_type_of(r.id()).as_deref() == Some(LE_NAME)
                && get(r.id()).is_some() =>
        {
            Some(r.id())
        }
        _ => None,
    }
}

/// php_mimepart_alloc()
pub fn alloc() -> i64 {
    let id = goro_core::value::alloc_resource_id();
    goro_core::value::register_resource(id, LE_NAME);
    goro_core::value::set_resource_dtor(id, dtor);
    let part = Part {
        in_header: true,
        ..Part::default()
    };
    PARTS.with(|p| p.borrow_mut().insert(id, Rc::new(RefCell::new(part))));
    id
}

/// The resource destructor (mimepart_dtor -> php_mimepart_free()).
fn dtor(_vm: &mut Vm, id: i64) {
    free(id);
    goro_core::value::remove_resource(id);
}

fn free(id: i64) {
    let snap = FINALIZED.with(|f| f.borrow_mut().remove(&id));
    let part = PARTS.with(|p| p.borrow_mut().remove(&id));
    // the children's references go with them
    drop(snap);
    drop(part);
}

/// zend_list_close(): free the part now; the resource becomes unusable.
pub fn close(id: i64) {
    goro_core::value::clear_resource_dtor(id);
    goro_core::value::close_resource(id);
    free(id);
}

fn child_ids(p: &Part) -> Vec<i64> {
    p.children
        .iter()
        .filter_map(|v| match v {
            Value::Resource(r) => Some(r.id()),
            _ => None,
        })
        .collect()
}

/// The live children (a child freed by mailparse_msg_free() is left out).
pub fn children(id: i64) -> Vec<i64> {
    let mut c = read(id, child_ids).unwrap_or_default();
    c.retain(|&cid| get(cid).is_some());
    c
}

pub fn parent(id: i64) -> Option<i64> {
    read(id, |p| p.parent)
        .flatten()
        .filter(|&p| get(p).is_some())
}

fn top(mut id: i64) -> i64 {
    let mut n = 0;
    while let Some(p) = parent(id) {
        id = p;
        n += 1;
        if n > 64 {
            break;
        }
    }
    id
}

fn tree(top: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut stack = vec![top];
    while let Some(id) = stack.pop() {
        if out.contains(&id) {
            continue;
        }
        out.push(id);
        let mut c = children(id);
        c.reverse();
        stack.extend(c);
    }
    out
}

fn content_type_is(p: &Part, v: &[u8]) -> bool {
    p.content_type
        .as_ref()
        .is_some_and(|c| c.value.eq_ignore_ascii_case(v))
}

pub fn content_type_isl(p: &Part, v: &[u8]) -> bool {
    p.content_type
        .as_ref()
        .is_some_and(|c| c.value.len() >= v.len() && c.value[..v.len()].eq_ignore_ascii_case(v))
}

fn is_mime_1(p: &Part) -> bool {
    p.mime_version.as_deref() == Some(b"1.0") || p.parent.is_some()
}

/// The default charset (mailparse.def_charset).
pub fn def_charset(vm: &Vm) -> Vec<u8> {
    match vm.constants.get(b"mailparse.def_charset".as_slice()) {
        Some(Value::Null) | None => Vec::new(),
        Some(v) => v.to_php_string().as_bytes().to_vec(),
    }
}

/// php_mimepart_update_positions()
fn update_positions(mut id: i64, newendpos: i64, newbodyend: i64, deltanlines: i64) {
    let mut n = 0;
    loop {
        let Some(next) = with(id, |p| {
            p.endpos = newendpos;
            p.bodyend = newbodyend;
            p.nlines += deltanlines;
            if !p.in_header {
                p.nbodylines += deltanlines;
            }
            p.parent
        }) else {
            return;
        };
        match next {
            Some(p) if n < 64 => {
                id = p;
                n += 1;
            }
            _ => return,
        }
    }
}

/// alloc_new_child_part(): None when the message is nested too deeply.
fn alloc_child(parent: i64, startpos: i64, inherit: bool) -> Option<i64> {
    let mut depth = 0;
    let mut a = Some(parent);
    while let Some(id) = a {
        depth += 1;
        if depth >= MAXLEVELS {
            return None;
        }
        a = read(id, |p| p.parent).flatten();
    }
    let child = alloc();
    let (source, cte, charset) = read(parent, |p| {
        (
            p.source.clone(),
            p.content_transfer_encoding.clone(),
            p.charset.clone(),
        )
    })?;
    with(parent, |p| {
        p.lastpart = Some(child);
        p.children.push(Value::resource(child));
    });
    with(child, |c| {
        c.parent = Some(parent);
        c.source = source;
        c.startpos = startpos;
        c.endpos = startpos;
        c.bodystart = startpos;
        c.bodyend = startpos;
        if inherit {
            c.content_transfer_encoding = cte;
            c.charset = charset;
        }
    });
    Some(child)
}

/// Without leading and trailing blanks.
fn trim(mut v: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', rest @ ..] = v {
        v = rest;
    }
    while let [rest @ .., b' ' | b'\t'] = v {
        v = rest;
    }
    v
}

const QUOTE: u8 = 0x27;

/// rfc2231_to_mime()
fn rfc2231_to_mime(buf: &mut Vec<u8>, value: Option<&mut Vec<u8>>, charset_p: bool, prev_p: bool) {
    fn cstr(v: &[u8]) -> &[u8] {
        match v.iter().position(|&b| b == 0) {
            Some(n) => &v[..n],
            None => v,
        }
    }
    let mut startofvalue = None;
    let mut value = value;
    if let (true, Some(v)) = (charset_p, value.as_deref_mut()) {
        // with a previous charset only %nn -> =nn
        let mut quotes = if prev_p { 2 } else { 0 };
        let len = cstr(v).len();
        for (i, c) in v[..len].iter_mut().enumerate() {
            if *c == QUOTE {
                if quotes <= 1 {
                    if quotes == 0 {
                        // end of the charset
                        *c = 0;
                    } else {
                        startofvalue = Some(i + 1);
                    }
                    quotes += 1;
                }
            } else if *c == b'%' && quotes == 2 {
                *c = b'=';
            }
        }
    }
    if let (true, false, Some(sv), Some(v)) = (charset_p, prev_p, startofvalue, value.as_deref()) {
        // first encoded token
        buf.extend_from_slice(b"=?");
        buf.extend_from_slice(cstr(v));
        buf.extend_from_slice(b"?Q?");
        buf.extend_from_slice(cstr(&v[sv..]));
    }
    if prev_p && !charset_p {
        // last encoded token
        buf.extend_from_slice(b"?=");
    }
    if !charset_p || prev_p {
        if let Some(v) = value {
            buf.extend_from_slice(cstr(v));
        }
    }
}

/// php_mimeheader_alloc_from_tok(): the value and attributes of a header
/// ("Name: value; attr=value; ...").
fn header_from_tokens(toks: &Tokenized) -> HeaderAttrs {
    use rfc822::{RECOMBINE_IGNORE_COMMENTS as IGN, RECOMBINE_STRTOLOWER as LOWER, recombine};
    let n = toks.len();
    let tok = |i: usize| toks.tok(i);
    let mut first_semi = 2;
    while first_semi < n && tok(first_semi) != b';' {
        first_semi += 1;
    }
    let mut attr = HeaderAttrs::new(&recombine(
        toks,
        2,
        first_semi.saturating_sub(2),
        LOWER | IGN,
    ));
    if first_semi < n {
        first_semi += 1;
    }

    let mut name_buf: Option<Vec<u8>> = None;
    let mut value_buf: Vec<u8> = Vec::new();
    let mut is_rfc2231_name = false;
    let mut prevcharset_p = false;
    let mut namechanged = false;
    let mut charset_p = false;
    let mut netscape_bug = false;

    while first_semi < n {
        // find the next ;
        let mut comments_before_semi = 0;
        let mut next_semi = first_semi;
        while next_semi < n {
            if tok(next_semi) == b';' {
                break;
            }
            if tok(next_semi) == b'(' {
                comments_before_semi += 1;
            }
            next_semi += 1;
        }

        let mut i = first_semi;
        if i < next_semi {
            i += 1;
            // ignore comments
            while i < next_semi && tok(i) == b'(' {
                i += 1;
            }
            if i < next_semi && tok(i) == b'=' {
                // skip the "="
                i += 1;
                // Netscape Messenger sometimes omits the ; when wrapping
                if next_semi < n
                    && tok(next_semi) != b';'
                    && next_semi - first_semi - comments_before_semi > 3
                {
                    next_semi = i + 1;
                    netscape_bug = true;
                }

                let mut name = Some(recombine(toks, first_semi, 1, LOWER | IGN));
                let mut value = recombine(toks, i, next_semi - i, IGN);

                // RFC 2231 parameter value continuations and charsets
                let mut currentencoded = false;
                let star = name
                    .as_ref()
                    .and_then(|nm| nm.iter().position(|&b| b == b'*'));
                if let Some(star) = star {
                    currentencoded = true;
                    let nm = name.as_mut().unwrap();
                    // a final * means a charset encoding
                    charset_p = nm.last() == Some(&b'*');
                    nm.truncate(star);
                    match &name_buf {
                        None => {
                            namechanged = false;
                            name_buf = name.clone();
                        }
                        Some(nb) => {
                            namechanged = nb != nm;
                            if !namechanged {
                                name = None;
                            }
                        }
                    }
                    if !namechanged {
                        rfc2231_to_mime(&mut value_buf, Some(&mut value), charset_p, prevcharset_p);
                        prevcharset_p = charset_p;
                    }
                    is_rfc2231_name = true;
                }

                if is_rfc2231_name {
                    if let (Some(nm), Some(nb)) = (&name, &name_buf) {
                        if nb != nm {
                            // finalize the packet
                            rfc2231_to_mime(&mut value_buf, None, false, prevcharset_p);
                            attr.set(nb, &value_buf);
                            name_buf = None;
                            value_buf.clear();
                            prevcharset_p = false;
                            is_rfc2231_name = false;
                            if !currentencoded {
                                attr.set(nm, cstr(&value));
                            } else if namechanged {
                                // a new encoded name
                                rfc2231_to_mime(
                                    &mut value_buf,
                                    Some(&mut value),
                                    charset_p,
                                    prevcharset_p,
                                );
                                is_rfc2231_name = true;
                                name_buf = name.clone();
                                prevcharset_p = charset_p;
                            }
                            namechanged = false;
                        }
                    }
                } else if let Some(nm) = &name {
                    attr.set(nm, cstr(&value));
                }
            }
        }

        if next_semi < n && !netscape_bug {
            next_semi += 1;
        }
        first_semi = next_semi;
        netscape_bug = false;
    }

    if is_rfc2231_name {
        rfc2231_to_mime(&mut value_buf, None, false, prevcharset_p);
        if let Some(nb) = &name_buf {
            attr.set(nb, &value_buf);
        }
    }
    attr
}

fn cstr(v: &[u8]) -> &[u8] {
    match v.iter().position(|&b| b == 0) {
        Some(n) => &v[..n],
        None => v,
    }
}

fn str_val(s: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(s))
}

/// add_assoc_*(): a symtable key (numeric strings are integer keys).
pub fn sym_key(k: &[u8]) -> ArrayKey {
    Vm::value_to_array_key(str_val(k))
}

/// php_mimepart_process_header(): the header collected in headerbuf.
fn process_header(id: i64) {
    let Some(part) = get(id) else {
        return;
    };
    let mut p = part.borrow_mut();
    if p.headerbuf.is_empty() {
        return;
    }
    let hb = std::mem::take(&mut p.headerbuf);
    let hb = cstr(&hb);
    let (toks, _) = rfc822::tokenize(hb, false);
    // a header is a word, ':' and the value
    if toks.len() < 2 || toks.tok(0) != 0 || toks.tok(1) != b':' {
        return;
    }
    use rfc822::{RECOMBINE_IGNORE_COMMENTS as IGN, RECOMBINE_STRTOLOWER as LOWER, recombine};
    let key = recombine(&toks, 0, 1, IGN | LOWER);
    let stripped = recombine(&toks, 2, toks.len() - 2, IGN | LOWER);
    let Some(colon) = hb.iter().position(|&b| b == b':') else {
        return;
    };
    let mut v = colon + 1;
    while v < hb.len() && matches!(hb[v], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        v += 1;
    }
    let val = &hb[v..];

    // the header hash; multiple To: or Cc: lines are joined
    let lookup = ArrayKey::String(PhpString::from_bytes(&key));
    let existing = p.headers.get(&lookup).cloned();
    match existing {
        Some(Value::String(prev)) if key == b"to" || key == b"cc" => {
            let mut joined = prev.as_bytes().to_vec();
            joined.extend_from_slice(b", ");
            joined.extend_from_slice(val);
            p.headers.set(sym_key(&key), str_val(&joined));
        }
        Some(Value::Array(a)) => {
            let mut arr = a.borrow().clone();
            arr.push(str_val(val));
            p.headers
                .set(sym_key(&key), Value::Array(Rc::new(RefCell::new(arr))));
        }
        Some(prev) => {
            // more than one header of the name: an array of them
            let mut arr = PhpArray::new();
            arr.push(prev);
            arr.push(str_val(val));
            p.headers
                .set(sym_key(&key), Value::Array(Rc::new(RefCell::new(arr))));
        }
        None => p.headers.set(sym_key(&key), str_val(val)),
    }

    match key.as_slice() {
        b"mime-version" => p.mime_version = Some(stripped),
        b"content-location" => {
            p.content_location = Some(recombine(&toks, 2, toks.len() - 2, IGN));
        }
        b"content-base" => p.content_base = Some(recombine(&toks, 2, toks.len() - 2, IGN)),
        b"content-transfer-encoding" => p.content_transfer_encoding = Some(stripped),
        b"content-type" => {
            let ct = header_from_tokens(&toks);
            if let Some(b) = ct.get(b"boundary") {
                p.boundary = Some(b.to_vec());
            }
            if let Some(c) = ct.get(b"charset") {
                p.charset = Some(c.to_vec());
            }
            p.content_type = Some(ct);
        }
        b"content-disposition" => p.content_disposition = Some(header_from_tokens(&toks)),
        _ => {}
    }
}

pub enum ParseError {
    TooComplex,
    TooDeep,
}

impl ParseError {
    pub fn message(&self) -> &'static str {
        match self {
            ParseError::TooComplex => "MIME message too complex",
            ParseError::TooDeep => "MIME message too deeply nested",
        }
    }
}

/// php_mimepart_process_line(): `line` ends with "\n", unless `last` (the
/// final line of the message, without a line break).
fn process_line(root: i64, def_charset: &[u8], line: &[u8], last: bool) -> Result<(), ParseError> {
    // sanity check
    if read(root, |p| p.children.len()).unwrap_or(0) > MAXPARTS {
        return Err(ParseError::TooComplex);
    }
    let c = line;
    let origcount = c.len() as i64;
    let mut linelen = if last { c.len() } else { c.len() - 1 };
    if linelen > 0 && c[linelen - 1] == b'\r' {
        linelen -= 1;
    }
    let at = |i: usize| c.get(i).copied().unwrap_or(0);

    // discover which part we were last working on
    let mut wp = root;
    while let Some(lastpart) = read(wp, |p| p.lastpart).flatten() {
        if get(lastpart).is_none() {
            break;
        }
        let endpos = read(wp, |p| p.endpos).unwrap_or(0);
        if read(lastpart, |p| p.completed).unwrap_or(false) {
            update_positions(wp, endpos + origcount, endpos + origcount, 1);
            return Ok(());
        }
        let (boundary, in_header, mime_version) = read(wp, |p| {
            (p.boundary.clone(), p.in_header, p.mime_version.clone())
        })
        .unwrap();
        let Some(boundary) = boundary.filter(|_| !in_header) else {
            wp = lastpart;
            continue;
        };
        let blen = boundary.len();
        // a boundary line?
        if at(0) == b'-'
            && at(1) == b'-'
            && linelen >= 2 + blen
            && c[2..2 + blen].eq_ignore_ascii_case(&boundary)
        {
            // the final boundary?
            if linelen >= 4 + blen && &c[2 + blen..4 + blen] == b"--" {
                with(lastpart, |p| p.completed = true);
                update_positions(wp, endpos + origcount, endpos + origcount, 1);
                return Ok(());
            }
            let newpart = alloc_child(wp, endpos + origcount, true).ok_or(ParseError::TooDeep)?;
            update_positions(wp, endpos + origcount, endpos + linelen as i64, 1);
            with(newpart, |p| {
                if mime_version.is_some() {
                    p.mime_version = mime_version;
                }
                p.in_header = true;
            });
            return Ok(());
        }
        wp = lastpart;
    }

    let (in_header, completed, has_last, endpos, parent) = read(wp, |p| {
        (
            p.in_header,
            p.completed,
            p.lastpart.is_some(),
            p.endpos,
            p.parent,
        )
    })
    .unwrap();
    if !in_header {
        if !completed && !has_last {
            // the final newline of a part of a multipart belongs to the
            // boundary, otherwise to the body
            let in_multipart = parent
                .and_then(|pp| read(pp, |p| content_type_isl(p, b"multipart/")))
                .unwrap_or(false);
            if in_multipart {
                update_positions(wp, endpos + origcount, endpos + linelen as i64, 1);
            } else {
                update_positions(wp, endpos + origcount, endpos + origcount, 1);
            }
        }
        return Ok(());
    }

    if linelen > 0 {
        update_positions(wp, endpos + origcount, endpos + linelen as i64, 1);
        let mut start = 0;
        if c[0] == b' ' || c[0] == b'\t' {
            // a continuation (a folding tab becomes a space)
            with(wp, |p| p.headerbuf.push(b' '));
            start = 1;
            linelen -= 1;
        } else {
            process_header(wp);
        }
        // kept for a possible continuation
        with(wp, |p| {
            p.headerbuf.extend_from_slice(&c[start..start + linelen])
        });
        return Ok(());
    }

    // end of the headers
    process_header(wp);
    let bodystart = endpos + origcount;
    with(wp, |p| {
        p.in_header = false;
        p.bodystart = bodystart;
    });
    update_positions(wp, bodystart, bodystart, 1);
    with(wp, |p| p.nbodylines -= 1);
    body_defaults(wp, def_charset);

    let (is_rfc822, has_boundary) = read(wp, |p| {
        (content_type_is(p, b"message/rfc822"), p.boundary.is_some())
    })
    .unwrap();
    if is_rfc822 {
        let child = alloc_child(wp, bodystart, false).ok_or(ParseError::TooDeep)?;
        with(child, |p| p.in_header = true);
        return Ok(());
    }
    // a section for the preamble before the first boundary
    if has_boundary {
        let child = alloc_child(wp, bodystart, true).ok_or(ParseError::TooDeep)?;
        with(child, |p| {
            p.in_header = false;
            p.is_dummy = true;
        });
    }
    Ok(())
}

/// The content type and charset defaults applied at the end of the
/// headers.
fn body_defaults(id: i64, def_charset: &[u8]) {
    let parent_is_digest = parent(id)
        .and_then(|pp| read(pp, |p| content_type_is(p, b"multipart/digest")))
        .unwrap_or(false);
    with(id, |p| {
        // some mailers send a content-type without a mime-version, or a
        // version other than 1.0: pretend they said 1.0
        if !is_mime_1(p) && p.content_type.is_some() {
            p.mime_version = Some(b"1.0".to_vec());
        }
        if !is_mime_1(p) {
            // an unknown MIME version: no content-type or boundary
            p.content_disposition = None;
            p.boundary = None;
            p.content_type = Some(HeaderAttrs::new(b"text/plain"));
        }
        // text/plain by default, message/rfc822 in a multipart/digest
        if is_mime_1(p) && p.content_type.is_none() {
            let def: &[u8] = if parent_is_digest {
                b"message/rfc822"
            } else {
                b"text/plain"
            };
            p.content_type = Some(HeaderAttrs::new(def));
        }
        // us-ascii (mailparse.def_charset) without an inherited or
        // explicit charset
        if p.charset.is_none() {
            p.charset = Some(def_charset.to_vec());
        }
    });
}

/// php_mimepart_parse()
pub fn parse(id: i64, def_charset: &[u8], mut buf: &[u8]) -> Result<(), ParseError> {
    rollback(id);
    with(id, |p| p.parse_root = true);
    while !buf.is_empty() {
        match buf.iter().position(|&b| b == b'\n') {
            Some(eol) => {
                let len = eol + 1;
                let line = with(id, |p| {
                    p.workbuf.extend_from_slice(&buf[..len]);
                    p.workbuf.clone()
                })
                .unwrap_or_default();
                if let Err(e) = process_line(id, def_charset, &line, false) {
                    with(id, |p| p.parse_failed = true);
                    return Err(e);
                }
                with(id, |p| p.workbuf.clear());
                buf = &buf[len..];
            }
            None => {
                // no EOL: kept for the next call
                with(id, |p| p.workbuf.extend_from_slice(buf));
                break;
            }
        }
    }
    with(id, |p| p.parse_failed = false);
    Ok(())
}

/// Process the data still pending in the tree of `id` as the end of the
/// message: the last line without a line break, and the header line still
/// pending in a part whose body never started. Warnings of the processing
/// are returned.
pub fn finalize(id: i64, def_charset: &[u8]) -> Vec<&'static str> {
    let top = top(id);
    if FINALIZED.with(|f| f.borrow().contains_key(&top)) {
        return Vec::new();
    }
    let parts = tree(top);
    let snapshot: Vec<(i64, Part)> = parts
        .iter()
        .filter_map(|&pid| read(pid, |p| (pid, p.clone())))
        .collect();
    let mut warnings = Vec::new();
    for &pid in &parts {
        let Some((is_root, failed, line)) =
            read(pid, |p| (p.parse_root, p.parse_failed, p.workbuf.clone()))
        else {
            continue;
        };
        if !is_root || failed {
            continue;
        }
        if !line.is_empty() {
            if let Err(e) = process_line(pid, def_charset, &line, true) {
                warnings.push(e.message());
            }
            with(pid, |p| p.workbuf.clear());
        }
        flush_pending_header(pid);
    }
    FINALIZED.with(|f| f.borrow_mut().insert(top, snapshot));
    warnings
}

/// The header still collected by the part the next line would go to
/// (a header is processed when the line after it starts) is processed:
/// the message ends there. The part stays in its headers (as when the
/// message ends with a complete header line), its body is not started.
fn flush_pending_header(root: i64) {
    let mut wp = root;
    while let Some(lastpart) = read(wp, |p| p.lastpart).flatten() {
        if get(lastpart).is_none() || read(lastpart, |p| p.completed).unwrap_or(true) {
            return;
        }
        wp = lastpart;
    }
    if read(wp, |p| p.in_header).unwrap_or(false) {
        process_header(wp);
    }
}

/// Undo finalize() before more data is parsed into the tree of `id`.
pub fn rollback(id: i64) {
    let top = top(id);
    let Some(snapshot) = FINALIZED.with(|f| f.borrow_mut().remove(&top)) else {
        return;
    };
    for (pid, state) in snapshot {
        if let Some(p) = get(pid) {
            *p.borrow_mut() = state;
        }
    }
}

/// php_mimepart_get_offsets(): (start, end, start of body, lines, body
/// lines)
pub fn offsets(p: &Part) -> (i64, i64, i64, i64, i64) {
    let mut end = p.endpos;
    let mut nlines = p.nlines;
    let mut nbodylines = p.nbodylines;
    // the newline before a boundary belongs to the boundary
    if p.parent.is_some() {
        end = p.bodyend;
        if nlines > 0 {
            nlines -= 1;
        }
        if nbodylines > 0 {
            nbodylines -= 1;
        }
    }
    (p.startpos, end, p.bodystart, nlines, nbodylines)
}

/// php_mimepart_enum_parts(): every part with its section name ("1.2.1"),
/// depth first; the preamble of a multipart is left out.
pub fn enum_parts(id: i64) -> Vec<(i64, Vec<u32>)> {
    fn rec(id: i64, path: &mut Vec<u32>, out: &mut Vec<(i64, Vec<u32>)>, depth: usize) {
        out.push((id, path.clone()));
        if depth > 64 {
            return;
        }
        let multipart = read(id, |p| content_type_isl(p, b"multipart/")).unwrap_or(false);
        let mut next: u32 = if multipart { 0 } else { 1 };
        for c in children(id) {
            if next != 0 {
                path.push(next);
                rec(c, path, out, depth + 1);
                path.pop();
            }
            next = next.wrapping_add(1);
        }
    }
    let mut out = Vec::new();
    rec(id, &mut vec![1], &mut out, 0);
    out
}

/// php_mimepart_find_by_name()
pub fn find_by_name(id: i64, name: &[u8]) -> Option<i64> {
    let name = cstr(name);
    let mut found = None;
    'parts: for (pid, path) in enum_parts(id) {
        let mut i = 0;
        for &want in &path {
            if !name.get(i).is_some_and(|b| b.is_ascii_digit()) {
                continue 'parts;
            }
            let mut n: u32 = 0;
            while let Some(d) = name.get(i).filter(|b| b.is_ascii_digit()) {
                n = n.wrapping_mul(10).wrapping_add((d - b'0') as u32);
                i += 1;
            }
            if i < name.len() {
                if name[i] != b'.' {
                    continue 'parts;
                }
                i += 1;
            }
            if n != want {
                continue 'parts;
            }
        }
        if i >= name.len() {
            found = Some(pid);
        }
    }
    found
}

/// php_mimepart_find_child_by_position()
pub fn find_child_by_position(id: i64, position: i32) -> Option<i64> {
    let c = children(id);
    let idx = if position < 0 { 0 } else { position as usize };
    c.get(idx).copied().filter(|&cid| get(cid).is_some())
}

/// php_mimepart_remove_from_parent()
pub fn remove_from_parent(id: i64) {
    let Some(parent) = parent(id) else {
        return;
    };
    // the tree changes: its pending data stays pending
    rollback(parent);
    with(id, |p| p.parent = None);
    let removed = with(parent, |p| {
        let pos = p
            .children
            .iter()
            .position(|v| matches!(v, Value::Resource(r) if r.id() == id));
        pos.map(|i| p.children.remove(i))
    });
    drop(removed);
}

/// The part data array (mailparse_get_part_data()).
pub fn part_data(vm: &Vm, id: i64) -> PhpArray {
    let def = def_charset(vm);
    let Some(part) = get(id) else {
        return PhpArray::new();
    };
    let p = part.borrow();
    let mut a = PhpArray::new();
    let set = |a: &mut PhpArray, k: &[u8], v: Value| a.set(sym_key(k), v);
    let headers = p.headers.clone();
    set(
        &mut a,
        b"headers",
        Value::Array(Rc::new(RefCell::new(headers))),
    );
    let (start, end, bodystart, nlines, nbodylines) = offsets(&p);
    set(&mut a, b"starting-pos", Value::Long(start));
    set(&mut a, b"starting-pos-body", Value::Long(bodystart));
    set(&mut a, b"ending-pos", Value::Long(end));
    set(&mut a, b"ending-pos-body", Value::Long(p.bodyend));
    set(&mut a, b"line-count", Value::Long(nlines));
    set(&mut a, b"body-line-count", Value::Long(nbodylines));
    set(
        &mut a,
        b"charset",
        str_val(cstr(p.charset.as_deref().unwrap_or(&def))),
    );
    set(
        &mut a,
        b"transfer-encoding",
        str_val(p.content_transfer_encoding.as_deref().unwrap_or(b"8bit")),
    );
    let attr_header = |a: &mut PhpArray, label: &[u8], prefix: &[u8], h: &HeaderAttrs| {
        // An attribute with an integer name ("1") is an integer key of the
        // C array, for which zend_hash_get_current_key_ex() leaves the key
        // string of the previous attribute in place: it is stored under
        // that name.
        let mut last_str: Option<&[u8]> = None;
        for (k, v) in &h.attributes {
            let name: &[u8] = match sym_key(k) {
                ArrayKey::Int(_) => last_str.unwrap_or(k),
                ArrayKey::String(_) => {
                    last_str = Some(k);
                    k
                }
            };
            let mut key = prefix.to_vec();
            key.extend_from_slice(name);
            a.set(sym_key(&key), str_val(cstr(v)));
        }
        // last, so that a bogus type= attribute does not override it
        a.set(sym_key(label), str_val(&h.value));
    };
    match &p.content_type {
        Some(ct) => attr_header(&mut a, b"content-type", b"content-", ct),
        None => set(&mut a, b"content-type", str_val(b"text/plain; (error)")),
    }
    if let Some(cd) = &p.content_disposition {
        attr_header(&mut a, b"content-disposition", b"disposition-", cd);
    }
    if let Some(l) = &p.content_location {
        set(&mut a, b"content-location", str_val(l));
    }
    set(
        &mut a,
        b"content-base",
        str_val(p.content_base.as_deref().unwrap_or(b"/")),
    );
    if let Some(b) = &p.boundary {
        set(&mut a, b"content-boundary", str_val(cstr(b)));
    }
    // the content-id value, without its <>
    let hk = |k: &[u8]| ArrayKey::String(PhpString::from_bytes(k));
    if let Some(cid) = p.headers.get(&hk(b"content-id")) {
        let cid = match cid {
            Value::Array(arr) => arr.borrow().get(&ArrayKey::Int(0)).cloned(),
            v => Some(v.clone()),
        };
        if let Some(Value::String(s)) = cid {
            let mut id: &[u8] = s.as_bytes();
            id = trim(id);
            let value: &[u8] = if id.len() >= 2 && id[0] == b'<' {
                match id[1..].iter().position(|&b| b == b'>') {
                    Some(close) => &id[1..1 + close],
                    // '<' without '>': skip the '<'
                    None => trim(&id[1..]),
                }
            } else {
                id
            };
            set(&mut a, b"content-id", str_val(value));
        }
    }
    for k in [
        &b"content-description"[..],
        b"content-language",
        b"content-md5",
    ] {
        if let Some(v) = p.headers.get(&hk(k)) {
            set(&mut a, k, v.clone());
        }
    }
    a
}
