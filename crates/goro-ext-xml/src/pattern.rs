//! Port of libxml2's pattern.c streaming patterns (xmlPatterncompile,
//! xmlPatternGetStreamCtxt, xmlStreamPush / xmlStreamPushAttr /
//! xmlStreamPop), as used by XML Schema identity constraints for the
//! XPath subsets of `xs:selector` and `xs:field`.
//!
//! Only the streaming evaluation is ported: the reversed step programs
//! used by xmlPatternMatch() are not needed by any caller.

use crate::parser::{is_name_char, is_name_start_char};

/// xmlPatternFlags
pub const PATTERN_XPATH: i32 = 1 << 0;
pub const PATTERN_XSSEL: i32 = 1 << 1;
pub const PATTERN_XSFIELD: i32 = 1 << 2;

const PATTERN_NOTPATTERN: i32 = PATTERN_XPATH | PATTERN_XSSEL | PATTERN_XSFIELD;

const PAT_FROM_ROOT: i32 = 1 << 8;
const PAT_FROM_CUR: i32 = 1 << 9;

// step flags
const STEP_DESC: i32 = 1;
const STEP_FINAL: i32 = 2;
const STEP_ROOT: i32 = 4;
const STEP_ATTR: i32 = 8;
const STEP_NODE: i32 = 16;
const STEP_IN_SET: i32 = 32;

// stream comp flags
const STREAM_FINAL_IS_ANY_NODE: i32 = 1 << 14;
const STREAM_FROM_ROOT: i32 = 1 << 15;
const STREAM_DESC: i32 = 1 << 16;

/// xmlElementType values pushed to a stream.
pub const ELEMENT_NODE: i32 = 1;
pub const ATTRIBUTE_NODE: i32 = 2;
const STREAM_ANY_NODE: i32 = 100;

#[derive(Clone, Copy, PartialEq, Debug)]
enum PatOp {
    Root,
    Elem,
    Attr,
    Parent,
    Ancestor,
    Ns,
    All,
}

#[derive(Clone, Debug)]
struct StepOp {
    op: PatOp,
    value: Option<String>,
    value2: Option<String>,
}

#[derive(Clone, Debug)]
struct StreamStep {
    flags: i32,
    name: Option<String>,
    ns: Option<String>,
    node_type: i32,
}

#[derive(Clone, Debug, Default)]
struct StreamComp {
    steps: Vec<StreamStep>,
    flags: i32,
}

/// One alternative of a compiled pattern (xmlPattern; `|` chains them).
#[derive(Clone, Debug)]
struct Alt {
    flags: i32,
    stream: Option<StreamComp>,
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    alts: Vec<Alt>,
}

struct PatParser<'a> {
    b: &'a [u8],
    cur: usize,
    error: i32,
    flags: i32,
    steps: Vec<StepOp>,
    namespaces: &'a [(String, Option<String>)],
}

const XML_XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

fn is_blank_ch(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

fn utf8_char(b: &[u8], i: usize) -> Option<(u32, usize)> {
    let s = std::str::from_utf8(&b[i..(i + 4).min(b.len())])
        .or_else(|e| std::str::from_utf8(&b[i..i + e.valid_up_to()]))
        .ok()?;
    let c = s.chars().next()?;
    Some((c as u32, c.len_utf8()))
}

/// xmlScanName: the end of the name starting at `i` (== i when none).
fn scan_name(b: &[u8], i: usize, nc: bool) -> usize {
    let mut p = i;
    let mut first = true;
    while p < b.len() {
        if nc && b[p] == b':' {
            break;
        }
        let (c, l) = match utf8_char(b, p) {
            Some(x) => x,
            None => break,
        };
        let ok = if first {
            is_name_start_char(c)
        } else {
            is_name_char(c)
        };
        if !ok {
            break;
        }
        first = false;
        p += l;
    }
    p
}

impl PatParser<'_> {
    fn cur(&self) -> u8 {
        self.b.get(self.cur).copied().unwrap_or(0)
    }
    fn nxt(&self, n: usize) -> u8 {
        self.b.get(self.cur + n).copied().unwrap_or(0)
    }
    fn next(&mut self) {
        if self.cur < self.b.len() {
            self.cur += 1;
        }
    }
    fn skip_blanks(&mut self) {
        while is_blank_ch(self.cur()) {
            self.next();
        }
    }
    fn push(&mut self, op: PatOp, value: Option<String>, value2: Option<String>) {
        self.steps.push(StepOp { op, value, value2 });
    }
    fn is_xs_sel(&self) -> bool {
        self.flags & PATTERN_XSSEL != 0
    }

    /// xmlPatScanName / xmlPatScanNCName
    fn scan(&mut self, nc: bool) -> Option<String> {
        self.skip_blanks();
        let q = self.cur;
        let e = scan_name(self.b, q, nc);
        if e == q {
            return None;
        }
        self.cur = e;
        Some(String::from_utf8_lossy(&self.b[q..e]).into_owned())
    }

    /// The namespace name bound to `prefix` in the pattern's context.
    fn lookup_prefix(&self, prefix: &str) -> Option<String> {
        if prefix == "xml" {
            return Some(XML_XML_NAMESPACE.to_string());
        }
        self.namespaces
            .iter()
            .find(|(_, p)| p.as_deref() == Some(prefix))
            .map(|(h, _)| h.clone())
    }

    /// xmlCompileAttributeTest
    fn compile_attribute_test(&mut self) {
        self.skip_blanks();
        let name = match self.scan(true) {
            None => {
                if self.cur() == b'*' {
                    self.push(PatOp::Attr, None, None);
                    self.next();
                } else {
                    self.error = 1;
                }
                return;
            }
            Some(n) => n,
        };
        if self.cur() == b':' {
            let prefix = name;
            self.next();
            if is_blank_ch(self.cur()) {
                self.error = 1;
                return;
            }
            let token = self.scan(false);
            let url = match self.lookup_prefix(&prefix) {
                Some(u) => u,
                None => {
                    self.error = 1;
                    return;
                }
            };
            match token {
                None => {
                    if self.cur() == b'*' {
                        self.next();
                        self.push(PatOp::Attr, None, Some(url));
                    } else {
                        self.error = 1;
                    }
                }
                Some(t) => self.push(PatOp::Attr, Some(t), Some(url)),
            }
        } else {
            self.push(PatOp::Attr, Some(name), None);
        }
    }

    /// The `prefix:` part of a step has been read: "prefix:name" or
    /// "prefix:*".
    fn compile_prefixed_step(&mut self, prefix: String) {
        let token = self.scan(false);
        let url = match self.lookup_prefix(&prefix) {
            Some(u) => u,
            None => {
                self.error = 1;
                return;
            }
        };
        match token {
            None => {
                if self.cur() == b'*' {
                    self.next();
                    self.push(PatOp::Ns, Some(url), None);
                } else {
                    self.error = 1;
                }
            }
            Some(t) => self.push(PatOp::Elem, Some(t), Some(url)),
        }
    }

    /// xmlCompileStepPattern
    fn compile_step_pattern(&mut self) {
        self.skip_blanks();
        if self.cur() == b'.' {
            self.next();
            self.push(PatOp::Elem, None, None);
            return;
        }
        if self.cur() == b'@' {
            if self.is_xs_sel() {
                // "Unexpected attribute axis"
                self.error = 1;
                return;
            }
            self.next();
            self.compile_attribute_test();
            return;
        }
        let name = match self.scan(true) {
            None => {
                if self.cur() == b'*' {
                    self.next();
                    self.push(PatOp::All, None, None);
                } else {
                    self.error = 1;
                }
                return;
            }
            Some(n) => n,
        };
        let mut has_blanks = false;
        if is_blank_ch(self.cur()) {
            has_blanks = true;
            self.skip_blanks();
        }
        if self.cur() == b':' {
            self.next();
            if self.cur() != b':' {
                if has_blanks || is_blank_ch(self.cur()) {
                    // "Invalid QName."
                    self.error = 1;
                    return;
                }
                self.compile_prefixed_step(name);
            } else {
                self.next();
                if name == "child" {
                    let name = match self.scan(false) {
                        None => {
                            if self.cur() == b'*' {
                                self.next();
                                self.push(PatOp::All, None, None);
                            } else {
                                self.error = 1;
                            }
                            return;
                        }
                        Some(n) => n,
                    };
                    if self.cur() == b':' {
                        self.next();
                        if is_blank_ch(self.cur()) {
                            self.error = 1;
                            return;
                        }
                        self.compile_prefixed_step(name);
                    } else {
                        self.push(PatOp::Elem, Some(name), None);
                    }
                } else if name == "attribute" {
                    if self.is_xs_sel() {
                        self.error = 1;
                        return;
                    }
                    self.compile_attribute_test();
                } else {
                    // "The 'element' or 'attribute' axis is expected."
                    self.error = 1;
                }
            }
        } else if self.cur() == b'*' {
            // a name followed by '*'
            self.error = 1;
        } else {
            self.push(PatOp::Elem, Some(name), None);
        }
    }

    /// xmlCompileIDCXPathPath
    fn compile_idc_xpath_path(&mut self) {
        self.skip_blanks();
        if self.cur() == b'/' {
            // "Unexpected selection of the document root"
            self.error = 1;
            return;
        }
        self.flags |= PAT_FROM_CUR;
        if self.cur() == b'.' {
            self.next();
            self.skip_blanks();
            if self.cur() == 0 {
                self.push(PatOp::Elem, None, None);
                return;
            }
            if self.cur() != b'/' {
                self.error = 1;
                return;
            }
            self.next();
            self.skip_blanks();
            if self.cur() == b'/' {
                if self.cur > 0 && is_blank_ch(self.b[self.cur - 1]) {
                    // "./ /"
                    self.error = 1;
                    return;
                }
                self.push(PatOp::Ancestor, None, None);
                self.next();
                self.skip_blanks();
            }
            if self.cur() == 0 {
                // "Unfinished expression"
                self.error = 1;
                return;
            }
        }
        loop {
            self.compile_step_pattern();
            if self.error != 0 {
                self.error = 1;
                return;
            }
            self.skip_blanks();
            if self.cur() != b'/' {
                break;
            }
            self.push(PatOp::Parent, None, None);
            self.next();
            self.skip_blanks();
            if self.cur() == b'/' {
                // "Unexpected subsequent '//'"
                self.error = 1;
                return;
            }
            if self.cur() == 0 {
                // "Unfinished expression"
                self.error = 1;
                return;
            }
        }
        if self.cur() != 0 {
            self.error = 1;
        }
    }

    /// xmlCompilePathPattern
    fn compile_path_pattern(&mut self) {
        self.skip_blanks();
        if self.cur() == b'/' {
            self.flags |= PAT_FROM_ROOT;
        } else if self.cur() == b'.' || self.flags & PATTERN_NOTPATTERN != 0 {
            self.flags |= PAT_FROM_CUR;
        }
        if self.cur() == b'/' && self.nxt(1) == b'/' {
            self.push(PatOp::Ancestor, None, None);
            self.next();
            self.next();
        } else if self.cur() == b'.' && self.nxt(1) == b'/' && self.nxt(2) == b'/' {
            self.push(PatOp::Ancestor, None, None);
            self.next();
            self.next();
            self.next();
            self.skip_blanks();
            if self.cur() == 0 {
                self.error = 1;
                return;
            }
        }
        if self.cur() == b'@' {
            self.next();
            self.compile_attribute_test();
            if self.error != 0 {
                return;
            }
            self.skip_blanks();
            if self.cur() != 0 {
                self.compile_step_pattern();
                if self.error != 0 {
                    return;
                }
            }
        } else {
            if self.cur() == b'/' {
                self.push(PatOp::Root, None, None);
                self.next();
                self.skip_blanks();
                if self.cur() == 0 {
                    self.error = 1;
                    return;
                }
            }
            self.compile_step_pattern();
            if self.error != 0 {
                return;
            }
            self.skip_blanks();
            while self.cur() == b'/' {
                if self.nxt(1) == b'/' {
                    self.push(PatOp::Ancestor, None, None);
                    self.next();
                    self.next();
                    self.skip_blanks();
                    self.compile_step_pattern();
                    if self.error != 0 {
                        return;
                    }
                } else {
                    self.push(PatOp::Parent, None, None);
                    self.next();
                    self.skip_blanks();
                    if self.cur() == 0 {
                        self.error = 1;
                        return;
                    }
                    self.compile_step_pattern();
                    if self.error != 0 {
                        return;
                    }
                }
            }
        }
        if self.cur() != 0 {
            self.error = 1;
        }
    }
}

/// xmlStreamCompile: None when the steps cannot be streamed.
fn stream_compile(steps: &[StepOp], pat_flags: i32) -> Option<StreamComp> {
    if steps.len() == 1
        && steps[0].op == PatOp::Elem
        && steps[0].value.is_none()
        && steps[0].value2.is_none()
    {
        return Some(StreamComp {
            steps: Vec::new(),
            flags: STREAM_FINAL_IS_ANY_NODE,
        });
    }
    let mut stream = StreamComp::default();
    let mut s: usize = 0;
    let mut root = false;
    let mut flags = 0;
    let mut prevs: Option<usize> = None;
    if pat_flags & PAT_FROM_ROOT != 0 {
        stream.flags |= STREAM_FROM_ROOT;
    }
    let add =
        |stream: &mut StreamComp, name: Option<String>, ns: Option<String>, nt: i32, f: i32| {
            stream.steps.push(StreamStep {
                flags: f,
                name,
                ns,
                node_type: nt,
            });
            stream.steps.len() - 1
        };
    for (i, step) in steps.iter().enumerate() {
        match step.op {
            PatOp::Root => {
                if i != 0 {
                    return None;
                }
                root = true;
            }
            PatOp::Ns => {
                s = add(&mut stream, None, step.value.clone(), ELEMENT_NODE, flags);
                prevs = Some(s);
                flags = 0;
            }
            PatOp::Attr => {
                flags |= STEP_ATTR;
                prevs = None;
                s = add(
                    &mut stream,
                    step.value.clone(),
                    step.value2.clone(),
                    ATTRIBUTE_NODE,
                    flags,
                );
                flags = 0;
            }
            PatOp::Elem => {
                if step.value.is_none() && step.value2.is_none() {
                    if steps.len() == i + 1 && flags & STEP_DESC != 0 {
                        stream.flags |= STREAM_FINAL_IS_ANY_NODE;
                        flags |= STEP_NODE;
                        s = add(&mut stream, None, None, STREAM_ANY_NODE, flags);
                        flags = 0;
                        if let Some(p) = prevs.take() {
                            stream.steps[p].flags |= STEP_IN_SET;
                        }
                    }
                    continue;
                }
                s = add(
                    &mut stream,
                    step.value.clone(),
                    step.value2.clone(),
                    ELEMENT_NODE,
                    flags,
                );
                prevs = Some(s);
                flags = 0;
            }
            PatOp::All => {
                s = add(&mut stream, None, None, ELEMENT_NODE, flags);
                prevs = Some(s);
                flags = 0;
            }
            PatOp::Parent => {}
            PatOp::Ancestor => {
                if flags & STEP_DESC != 0 {
                    continue;
                }
                flags |= STEP_DESC;
                stream.flags |= STREAM_DESC;
            }
        }
    }
    if !root && pat_flags & PATTERN_NOTPATTERN == 0 {
        stream.flags |= STREAM_DESC;
        if let Some(st) = stream.steps.first_mut() {
            st.flags |= STEP_DESC;
        }
    }
    if stream.steps.len() <= s {
        return None;
    }
    stream.steps[s].flags |= STEP_FINAL;
    if root {
        stream.steps[0].flags |= STEP_ROOT;
    }
    Some(stream)
}

impl Pattern {
    /// xmlPatterncompile: `namespaces` are the in-scope (href, prefix)
    /// bindings. None when the pattern does not compile.
    pub fn compile(
        pattern: &str,
        flags: i32,
        namespaces: &[(String, Option<String>)],
    ) -> Option<Pattern> {
        let mut alts: Vec<Alt> = Vec::new();
        let mut streamable = true;
        let mut typ = 0;
        let b = pattern.as_bytes();
        let mut start = 0;
        while start < b.len() {
            let end = b[start..]
                .iter()
                .position(|&c| c == b'|')
                .map(|p| start + p)
                .unwrap_or(b.len());
            let part = &b[start..end];
            let mut p = PatParser {
                b: part,
                cur: 0,
                error: 0,
                flags,
                steps: Vec::new(),
                namespaces,
            };
            if flags & (PATTERN_XSSEL | PATTERN_XSFIELD) != 0 {
                p.compile_idc_xpath_path();
            } else {
                p.compile_path_pattern();
            }
            if p.error != 0 {
                return None;
            }
            if streamable {
                if typ == 0 {
                    typ = p.flags & (PAT_FROM_ROOT | PAT_FROM_CUR);
                } else if typ == PAT_FROM_ROOT {
                    if p.flags & PAT_FROM_CUR != 0 {
                        streamable = false;
                    }
                } else if typ == PAT_FROM_CUR && p.flags & PAT_FROM_ROOT != 0 {
                    streamable = false;
                }
            }
            let stream = if streamable {
                stream_compile(&p.steps, p.flags)
            } else {
                None
            };
            let alt = Alt {
                flags: p.flags,
                stream,
            };
            // the first pattern heads the list, later ones are inserted
            // right after it
            if alts.is_empty() {
                alts.push(alt);
            } else {
                alts.insert(1, alt);
            }
            start = if end < b.len() { end + 1 } else { end };
        }
        if alts.is_empty() {
            // an empty pattern: xmlPatternCompileSafe returns NULL
            return None;
        }
        if !streamable {
            for a in alts.iter_mut() {
                a.stream = None;
            }
        }
        Some(Pattern { alts })
    }

    /// xmlPatternGetStreamCtxt
    pub fn stream_ctxt(&self) -> Option<StreamCtxt> {
        let mut subs = Vec::new();
        for a in &self.alts {
            let comp = a.stream.clone()?;
            subs.push(SubStream {
                comp,
                states: Vec::new(),
                level: 0,
                flags: a.flags,
                block_level: -1,
            });
        }
        // the first context heads the chain, later ones follow it in
        // reverse order (like the patterns)
        Some(StreamCtxt { subs })
    }
}

#[derive(Clone, Debug)]
struct SubStream {
    comp: StreamComp,
    /// (step index, level)
    states: Vec<(i32, i32)>,
    level: i32,
    flags: i32,
    block_level: i32,
}

/// xmlStreamCtxt (with its `next` chain for unions).
#[derive(Clone, Debug)]
pub struct StreamCtxt {
    subs: Vec<SubStream>,
}

fn step_matches(step: &StreamStep, name: Option<&str>, ns: Option<&str>) -> bool {
    if step.node_type == STREAM_ANY_NODE {
        return true;
    }
    match &step.name {
        None => match &step.ns {
            None => true,
            Some(sns) => ns.is_some_and(|n| n == sns),
        },
        Some(sname) => {
            step.ns.is_some() == ns.is_some()
                && name.is_some_and(|n| n == sname)
                && step.ns.as_deref() == ns
        }
    }
}

impl SubStream {
    fn add_state(&mut self, idx: i32, level: i32) {
        for st in self.states.iter_mut() {
            if st.0 < 0 {
                *st = (idx, level);
                return;
            }
        }
        self.states.push((idx, level));
    }

    /// One stream of xmlStreamPushInternal; Err on internal error.
    fn push(&mut self, name: Option<&str>, ns: Option<&str>, node_type: i32) -> Result<bool, ()> {
        let mut ret = false;
        if node_type == ELEMENT_NODE && name.is_none() && ns.is_none() {
            self.states.clear();
            self.level = 0;
            self.block_level = -1;
            let comp = &self.comp;
            if comp.flags & STREAM_FROM_ROOT != 0 {
                // "/." and "//." match the document node
                if comp.steps.is_empty()
                    || (comp.steps.len() == 1
                        && comp.steps[0].node_type == STREAM_ANY_NODE
                        && comp.steps[0].flags & STEP_DESC != 0)
                {
                    ret = true;
                } else if comp.steps[0].flags & STEP_ROOT != 0 {
                    self.add_state(0, 0);
                }
            }
            return Ok(ret);
        }
        if self.comp.steps.is_empty() {
            if self.flags & PATTERN_XPATH != 0 {
                return Ok(false);
            }
            if node_type != ATTRIBUTE_NODE
                && (self.flags & PATTERN_NOTPATTERN == 0 || self.level == 0)
            {
                ret = true;
            }
            self.level += 1;
            return Ok(ret);
        }
        if self.block_level != -1 {
            self.level += 1;
            return Ok(false);
        }
        if node_type != ELEMENT_NODE
            && node_type != ATTRIBUTE_NODE
            && self.comp.flags & STREAM_FINAL_IS_ANY_NODE == 0
        {
            self.level += 1;
            return Ok(false);
        }
        let desc_stream = self.comp.flags & STREAM_DESC != 0;
        let mut i = 0;
        let m = self.states.len();
        while i < m {
            let step_nr;
            if !desc_stream {
                let last = self.states[self.states.len() - 1];
                step_nr = last.0;
                if last.1 < self.level {
                    return Err(());
                }
                i = m;
            } else {
                step_nr = self.states[i].0;
                if step_nr < 0 {
                    i += 1;
                    continue;
                }
                let tmp = self.states[i].1;
                if tmp > self.level {
                    i += 1;
                    continue;
                }
                let desc = self.comp.steps[step_nr as usize].flags & STEP_DESC != 0;
                if tmp < self.level && !desc {
                    i += 1;
                    continue;
                }
            }
            let step = self.comp.steps[step_nr as usize].clone();
            if step.node_type != node_type {
                if step.node_type == ATTRIBUTE_NODE {
                    if !desc_stream {
                        self.block_level = self.level + 1;
                    }
                    i += 1;
                    continue;
                } else if step.node_type != STREAM_ANY_NODE {
                    i += 1;
                    continue;
                }
            }
            let matched = step_matches(&step, name, ns);
            let mut fin = false;
            if matched {
                fin = step.flags & STEP_FINAL != 0;
                if fin {
                    ret = true;
                } else {
                    let lv = self.level + 1;
                    self.add_state(step_nr + 1, lv);
                }
                if !ret && step.flags & STEP_IN_SET != 0 {
                    ret = true;
                }
            }
            if !desc_stream && (!matched || fin) {
                self.block_level = self.level + 1;
            }
            i += 1;
        }
        self.level += 1;
        let step = self.comp.steps[0].clone();
        if step.flags & STEP_ROOT != 0 {
            return Ok(ret);
        }
        let desc = step.flags & STEP_DESC != 0;
        let xs_idc = self.flags & (PATTERN_XSSEL | PATTERN_XSFIELD) != 0;
        if self.flags & PATTERN_NOTPATTERN != 0 {
            let compare = if self.level == 1 {
                !xs_idc
            } else {
                desc || (self.level == 2 && xs_idc)
            };
            if !compare {
                return Ok(ret);
            }
        }
        // compare:
        if step.node_type != node_type {
            if node_type == ATTRIBUTE_NODE || step.node_type != STREAM_ANY_NODE {
                return Ok(ret);
            }
        }
        let matched = step_matches(&step, name, ns);
        let fin = step.flags & STEP_FINAL != 0;
        if matched {
            if fin {
                ret = true;
            } else {
                let lv = self.level;
                self.add_state(1, lv);
            }
            if !ret && step.flags & STEP_IN_SET != 0 {
                ret = true;
            }
        }
        if !desc_stream && (!matched || fin) {
            self.block_level = self.level;
        }
        Ok(ret)
    }

    fn pop(&mut self) {
        if self.block_level == self.level {
            self.block_level = -1;
        }
        if self.level != 0 {
            self.level -= 1;
        }
        while let Some(&(_, lev)) = self.states.last() {
            if lev > self.level {
                self.states.pop();
            } else {
                break;
            }
        }
    }
}

impl StreamCtxt {
    fn push_internal(&mut self, name: Option<&str>, ns: Option<&str>, node_type: i32) -> i32 {
        let mut ret = 0;
        for s in self.subs.iter_mut() {
            match s.push(name, ns, node_type) {
                Err(()) => return -1,
                Ok(true) => ret = 1,
                Ok(false) => {}
            }
        }
        ret
    }

    /// xmlStreamPush: 1 on a match, 0 otherwise, -1 on error.
    pub fn push(&mut self, name: Option<&str>, ns: Option<&str>) -> i32 {
        self.push_internal(name, ns, ELEMENT_NODE)
    }

    /// xmlStreamPushAttr
    pub fn push_attr(&mut self, name: Option<&str>, ns: Option<&str>) -> i32 {
        self.push_internal(name, ns, ATTRIBUTE_NODE)
    }

    /// xmlStreamPop
    pub fn pop(&mut self) {
        for s in self.subs.iter_mut() {
            s.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns() -> Vec<(String, Option<String>)> {
        vec![("urn:x".to_string(), Some("x".to_string()))]
    }

    #[test]
    fn compile_errors() {
        let n = ns();
        assert!(Pattern::compile("/a", PATTERN_XSSEL, &n).is_none());
        assert!(Pattern::compile("@a", PATTERN_XSSEL, &n).is_none());
        assert!(Pattern::compile("@a", PATTERN_XSFIELD, &n).is_some());
        assert!(Pattern::compile("y:a", PATTERN_XSSEL, &n).is_none());
        assert!(Pattern::compile("x:a/x:b", PATTERN_XSSEL, &n).is_some());
        assert!(Pattern::compile("a//b", PATTERN_XSSEL, &n).is_none());
        assert!(Pattern::compile(".//a|b", PATTERN_XSSEL, &n).is_some());
    }

    #[test]
    fn selector_child() {
        // the scope element is pushed first, then its descendants
        let p = Pattern::compile("x:b", PATTERN_XSSEL, &ns()).unwrap();
        let mut c = p.stream_ctxt().unwrap();
        assert_eq!(c.push(Some("scope"), Some("urn:x")), 0);
        assert_eq!(c.push(Some("b"), Some("urn:x")), 1);
        assert_eq!(c.push(Some("b"), Some("urn:x")), 0);
        c.pop();
        c.pop();
        assert_eq!(c.push(Some("b"), None), 0);
        c.pop();
        assert_eq!(c.push(Some("b"), Some("urn:x")), 1);
    }

    #[test]
    fn selector_desc() {
        let p = Pattern::compile(".//b", PATTERN_XSSEL, &[]).unwrap();
        let mut c = p.stream_ctxt().unwrap();
        assert_eq!(c.push(Some("scope"), None), 0);
        assert_eq!(c.push(Some("a"), None), 0);
        assert_eq!(c.push(Some("b"), None), 1);
        assert_eq!(c.push(Some("b"), None), 1);
    }

    #[test]
    fn field_attr() {
        let p = Pattern::compile("@id", PATTERN_XSFIELD, &[]).unwrap();
        let mut c = p.stream_ctxt().unwrap();
        assert_eq!(c.push(Some("target"), None), 0);
        assert_eq!(c.push_attr(Some("id"), None), 1);
        c.pop();
        assert_eq!(c.push_attr(Some("other"), None), 0);
    }

    #[test]
    fn field_self() {
        let p = Pattern::compile(".", PATTERN_XSFIELD, &[]).unwrap();
        let mut c = p.stream_ctxt().unwrap();
        assert_eq!(c.push(Some("target"), None), 1);
        assert_eq!(c.push(Some("child"), None), 0);
    }
}
