//! Match patterns: compilation (pattern.c xsltCompilePattern & co.). The
//! matching itself (xsltTestCompMatch) lives in transform.rs as it needs
//! the transformation context.

use crate::style::Compiler;
use crate::util::*;
use crate::xpath::CompExpr;
use goro_ext_xml::tree::NodeId;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum POp {
    End,
    Root,
    Elem,
    Attr,
    Parent,
    Ancestor,
    Id,
    Key,
    Ns,
    All,
    Pi,
    Comment,
    Text,
    Node,
    Predicate,
}

#[derive(Clone, Debug)]
pub struct PStep {
    pub op: POp,
    pub value: Option<String>,
    pub value2: Option<String>,
    pub value3: Option<String>,
    pub comp: Option<Rc<CompExpr>>,
}

/// xsltCompMatch
#[derive(Clone, Debug)]
pub struct CompMatch {
    pub priority: f32,
    pub pattern: String,
    pub mode: Option<String>,
    pub mode_uri: Option<String>,
    pub template: Option<usize>,
    pub node: NodeId,
    pub direct: bool,
    pub ns_list: Rc<Vec<(String, String)>>,
    pub steps: Vec<PStep>,
}

struct PCtx<'b> {
    s: &'b [u8],
    cur: usize,
    error: bool,
    elem: NodeId,
    steps: Vec<PStep>,
    priority: f32,
}

impl<'b> PCtx<'b> {
    fn c(&self) -> u8 {
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
    fn skip_blanks(&mut self) {
        while matches!(self.c(), 0x20 | 0x09 | 0x0A | 0x0D) {
            self.next();
        }
    }
}

fn utf8_at(s: &[u8], i: usize) -> (u32, usize) {
    if i >= s.len() {
        return (0, 0);
    }
    match std::str::from_utf8(&s[i..])
        .ok()
        .and_then(|x| x.chars().next())
    {
        Some(c) => (c as u32, c.len_utf8()),
        None => {
            // decode the first char of a valid prefix
            let mut l = 1;
            while l <= 4 && i + l <= s.len() {
                if let Ok(x) = std::str::from_utf8(&s[i..i + l]) {
                    if let Some(c) = x.chars().next() {
                        return (c as u32, c.len_utf8());
                    }
                }
                l += 1;
            }
            (0xFFFD, 1)
        }
    }
}

fn is_char_q(c: u32) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn is_letter(c: u32) -> bool {
    goro_ext_xml::xmlunicode::is_letter(c)
}

/// xsltScanLiteral
fn scan_literal(ctxt: &mut PCtx) -> Option<String> {
    ctxt.skip_blanks();
    let q = ctxt.c();
    if q != b'"' && q != b'\'' {
        ctxt.error = true;
        return None;
    }
    ctxt.next();
    let start = ctxt.cur;
    let mut cur = ctxt.cur;
    loop {
        let (v, l) = utf8_at(ctxt.s, cur);
        if !is_char_q(v) || v == q as u32 {
            if !is_char_q(v) {
                ctxt.error = true;
                return None;
            }
            break;
        }
        cur += l;
    }
    let r = String::from_utf8_lossy(&ctxt.s[start..cur]).into_owned();
    ctxt.cur = cur + 1;
    Some(r)
}

/// xsltScanNCName
fn scan_ncname(ctxt: &mut PCtx) -> Option<String> {
    ctxt.skip_blanks();
    let start = ctxt.cur;
    let mut cur = ctxt.cur;
    let (v, _) = utf8_at(ctxt.s, cur);
    if !is_letter(v) && v != '_' as u32 {
        return None;
    }
    loop {
        let (v, l) = utf8_at(ctxt.s, cur);
        if l == 0 {
            break;
        }
        let ok = is_letter(v)
            || goro_ext_xml::xmlunicode::is_digit(v)
            || v == '.' as u32
            || v == '-' as u32
            || v == '_' as u32
            || goro_ext_xml::xmlunicode::is_combining(v)
            || goro_ext_xml::xmlunicode::is_extender(v);
        if !ok {
            break;
        }
        cur += l;
    }
    let r = String::from_utf8_lossy(&ctxt.s[start..cur]).into_owned();
    ctxt.cur = cur;
    Some(r)
}

fn push(c: &mut Compiler, ctxt: &mut PCtx, op: POp, value: Option<String>, value2: Option<String>) {
    let mut comp = None;
    if op == POp::Predicate {
        comp = c.xpath_compile(value.as_deref().unwrap_or(""));
        if comp.is_none() {
            transform_error(
                c.vm,
                false,
                true,
                Some(ctxt.elem),
                "Failed to compile predicate\n",
            );
            if let Some(s) = CUR_STYLE.with(|x| x.get()) {
                c.sheet.styles[s].errors += 1;
            }
        }
    }
    ctxt.steps.push(PStep {
        op,
        value,
        value2,
        value3: None,
        comp,
    });
}

thread_local! {
    static CUR_STYLE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

fn swap_top(ctxt: &mut PCtx) {
    let j = ctxt.steps.len();
    if j > 1 {
        ctxt.steps.swap(j - 2, j - 1);
    }
}

fn generic(c: &mut Compiler, msg: &str) {
    transform_error(c.vm, false, false, None, msg);
}

/// xsltCompileIdKeyPattern
fn compile_id_key_pattern(c: &mut Compiler, ctxt: &mut PCtx, name: &str, aid: bool, axis: u8) {
    if ctxt.c() != b'(' {
        generic(c, "xsltCompileIdKeyPattern : ( expected\n");
        ctxt.error = true;
        return;
    }
    if aid && name == "id" {
        if axis != 0 {
            generic(c, "xsltCompileIdKeyPattern : NodeTest expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        ctxt.skip_blanks();
        let lit = scan_literal(ctxt);
        if ctxt.error {
            generic(c, "xsltCompileIdKeyPattern : Literal expected\n");
            return;
        }
        ctxt.skip_blanks();
        if ctxt.c() != b')' {
            generic(c, "xsltCompileIdKeyPattern : ) expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        push(c, ctxt, POp::Id, lit, None);
    } else if aid && name == "key" {
        if axis != 0 {
            generic(c, "xsltCompileIdKeyPattern : NodeTest expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        ctxt.skip_blanks();
        let lit = scan_literal(ctxt);
        if ctxt.error {
            generic(c, "xsltCompileIdKeyPattern : Literal expected\n");
            return;
        }
        ctxt.skip_blanks();
        if ctxt.c() != b',' {
            generic(c, "xsltCompileIdKeyPattern : , expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        ctxt.skip_blanks();
        let lit2 = scan_literal(ctxt);
        if ctxt.error {
            generic(c, "xsltCompileIdKeyPattern : Literal expected\n");
            return;
        }
        ctxt.skip_blanks();
        if ctxt.c() != b')' {
            generic(c, "xsltCompileIdKeyPattern : ) expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        push(c, ctxt, POp::Key, lit, lit2);
    } else if name == "processing-instruction" {
        ctxt.next();
        ctxt.skip_blanks();
        let mut lit = None;
        if ctxt.c() != b')' {
            lit = scan_literal(ctxt);
            if ctxt.error {
                generic(c, "xsltCompileIdKeyPattern : Literal expected\n");
                return;
            }
            ctxt.skip_blanks();
            if ctxt.c() != b')' {
                generic(c, "xsltCompileIdKeyPattern : ) expected\n");
                ctxt.error = true;
                return;
            }
        }
        ctxt.next();
        push(c, ctxt, POp::Pi, lit, None);
    } else if name == "text" || name == "comment" || name == "node" {
        ctxt.next();
        ctxt.skip_blanks();
        if ctxt.c() != b')' {
            generic(c, "xsltCompileIdKeyPattern : ) expected\n");
            ctxt.error = true;
            return;
        }
        ctxt.next();
        let op = match name {
            "text" => POp::Text,
            "comment" => POp::Comment,
            _ => {
                if axis == 2 {
                    POp::Attr
                } else {
                    POp::Node
                }
            }
        };
        push(c, ctxt, op, None, None);
    } else if aid {
        generic(
            c,
            "xsltCompileIdKeyPattern : expecting 'key' or 'id' or node type\n",
        );
        ctxt.error = true;
    } else {
        generic(c, "xsltCompileIdKeyPattern : node type\n");
        ctxt.error = true;
    }
}

/// xsltCompileStepPattern
fn compile_step_pattern(c: &mut Compiler, ctxt: &mut PCtx, token_in: Option<String>) {
    let mut token = token_in;
    let mut axis: u8 = 0; // 1 child, 2 attribute
    ctxt.skip_blanks();
    if token.is_none() && ctxt.c() == b'@' {
        ctxt.next();
        axis = 2;
    }
    loop {
        // parse_node_test:
        if token.is_none() {
            token = scan_ncname(ctxt);
        }
        let tok = match token.take() {
            None => {
                if ctxt.c() == b'*' {
                    ctxt.next();
                    if axis == 2 {
                        push(c, ctxt, POp::Attr, None, None);
                    } else {
                        push(c, ctxt, POp::All, None, None);
                    }
                    break;
                } else {
                    generic(c, "xsltCompileStepPattern : Name expected\n");
                    ctxt.error = true;
                    return;
                }
            }
            Some(t) => t,
        };
        ctxt.skip_blanks();
        if ctxt.c() == b'(' {
            compile_id_key_pattern(c, ctxt, &tok, false, axis);
            if ctxt.error {
                return;
            }
        } else if ctxt.c() == b':' {
            ctxt.next();
            if ctxt.c() != b':' {
                let prefix = tok;
                let tok2 = scan_ncname(ctxt);
                let elem = ctxt.elem;
                let url = tw(|t| {
                    t.search_ns(elem, Some(&prefix))
                        .map(|n| t.nss[n].href.clone())
                });
                let url = match url {
                    None => {
                        generic(
                            c,
                            &format!(
                                "xsltCompileStepPattern : no namespace bound to prefix {}\n",
                                prefix
                            ),
                        );
                        ctxt.error = true;
                        return;
                    }
                    Some(u) => u,
                };
                match tok2 {
                    None => {
                        if ctxt.c() == b'*' {
                            ctxt.next();
                            if axis == 2 {
                                push(c, ctxt, POp::Attr, None, Some(url));
                            } else {
                                push(c, ctxt, POp::Ns, Some(url), None);
                            }
                        } else {
                            generic(c, "xsltCompileStepPattern : Name expected\n");
                            ctxt.error = true;
                            return;
                        }
                    }
                    Some(t2) => {
                        if axis == 2 {
                            push(c, ctxt, POp::Attr, Some(t2), Some(url));
                        } else {
                            push(c, ctxt, POp::Elem, Some(t2), Some(url));
                        }
                    }
                }
            } else {
                if axis != 0 {
                    generic(c, "xsltCompileStepPattern : NodeTest expected\n");
                    ctxt.error = true;
                    return;
                }
                ctxt.next();
                if tok == "child" {
                    axis = 1;
                } else if tok == "attribute" {
                    axis = 2;
                } else {
                    generic(
                        c,
                        "xsltCompileStepPattern : 'child' or 'attribute' expected\n",
                    );
                    ctxt.error = true;
                    return;
                }
                ctxt.skip_blanks();
                token = scan_ncname(ctxt);
                continue;
            }
        } else {
            // xsltGetQNameURI on an unprefixed token: no namespace
            if axis == 2 {
                push(c, ctxt, POp::Attr, Some(tok), None);
            } else {
                push(c, ctxt, POp::Elem, Some(tok), None);
            }
        }
        break;
    }
    // parse_predicate:
    ctxt.skip_blanks();
    while ctxt.c() == b'[' {
        let mut level = 1;
        ctxt.next();
        let q = ctxt.cur;
        while ctxt.cur < ctxt.s.len() {
            let ch = ctxt.c();
            if ch == b'[' {
                level += 1;
            } else if ch == b']' {
                level -= 1;
                if level == 0 {
                    break;
                }
            } else if ch == b'"' {
                ctxt.next();
                while ctxt.cur < ctxt.s.len() && ctxt.c() != b'"' {
                    ctxt.next();
                }
            } else if ch == b'\'' {
                ctxt.next();
                while ctxt.cur < ctxt.s.len() && ctxt.c() != b'\'' {
                    ctxt.next();
                }
            }
            ctxt.next();
        }
        if ctxt.cur >= ctxt.s.len() {
            generic(c, "xsltCompileStepPattern : ']' expected\n");
            ctxt.error = true;
            return;
        }
        let ret = String::from_utf8_lossy(&ctxt.s[q..ctxt.cur]).into_owned();
        push(c, ctxt, POp::Predicate, Some(ret), None);
        swap_top(ctxt);
        ctxt.next();
        ctxt.skip_blanks();
    }
}

/// xsltCompileRelativePathPattern
fn compile_relative_path_pattern(c: &mut Compiler, ctxt: &mut PCtx, token: Option<String>) {
    compile_step_pattern(c, ctxt, token);
    if ctxt.error {
        return;
    }
    ctxt.skip_blanks();
    while ctxt.cur < ctxt.s.len() && ctxt.c() != b'|' {
        if ctxt.c() == b'/' && ctxt.nxt(1) == b'/' {
            push(c, ctxt, POp::Ancestor, None, None);
            ctxt.next();
            ctxt.next();
            ctxt.skip_blanks();
            compile_step_pattern(c, ctxt, None);
        } else if ctxt.c() == b'/' {
            push(c, ctxt, POp::Parent, None, None);
            ctxt.next();
            ctxt.skip_blanks();
            compile_step_pattern(c, ctxt, None);
        } else {
            ctxt.error = true;
        }
        if ctxt.error {
            return;
        }
        ctxt.skip_blanks();
    }
}

fn is_node_type(name: &str) -> bool {
    matches!(name, "node" | "text" | "comment" | "processing-instruction")
}

/// xsltCompileLocationPathPattern
fn compile_location_path_pattern(c: &mut Compiler, ctxt: &mut PCtx) {
    ctxt.skip_blanks();
    if ctxt.c() == b'/' && ctxt.nxt(1) == b'/' {
        ctxt.next();
        ctxt.next();
        ctxt.priority = 0.5;
        compile_relative_path_pattern(c, ctxt, None);
    } else if ctxt.c() == b'/' {
        ctxt.next();
        ctxt.skip_blanks();
        push(c, ctxt, POp::Root, None, None);
        if ctxt.cur < ctxt.s.len() && ctxt.c() != b'|' {
            push(c, ctxt, POp::Parent, None, None);
            compile_relative_path_pattern(c, ctxt, None);
        }
    } else if ctxt.c() == b'*' || ctxt.c() == b'@' {
        compile_relative_path_pattern(c, ctxt, None);
    } else {
        let name = match scan_ncname(ctxt) {
            Some(n) => n,
            None => {
                generic(c, "xsltCompileLocationPathPattern : Name expected\n");
                ctxt.error = true;
                return;
            }
        };
        ctxt.skip_blanks();
        if ctxt.c() == b'(' && !is_node_type(&name) {
            compile_id_key_pattern(c, ctxt, &name, true, 0);
            if ctxt.error {
                return;
            }
            if ctxt.c() == b'/' && ctxt.nxt(1) == b'/' {
                push(c, ctxt, POp::Ancestor, None, None);
                ctxt.next();
                ctxt.next();
                ctxt.skip_blanks();
                compile_relative_path_pattern(c, ctxt, None);
            } else if ctxt.c() == b'/' {
                push(c, ctxt, POp::Parent, None, None);
                ctxt.next();
                ctxt.skip_blanks();
                compile_relative_path_pattern(c, ctxt, None);
            }
            return;
        }
        compile_relative_path_pattern(c, ctxt, Some(name));
    }
}

/// xsltCompilePatternInternal: returns the indices (in sheet.pats) of the
/// compiled alternatives.
pub fn compile_pattern(
    c: &mut Compiler,
    pattern: &str,
    doc: NodeId,
    node: NodeId,
    style: Option<usize>,
    _novar: bool,
) -> Option<Vec<usize>> {
    let _ = doc;
    let old_style = CUR_STYLE.with(|x| x.replace(style));
    let r = compile_pattern_inner(c, pattern, node, style);
    CUR_STYLE.with(|x| x.set(old_style));
    r
}

fn compile_pattern_inner(
    c: &mut Compiler,
    pattern: &str,
    node: NodeId,
    style: Option<usize>,
) -> Option<Vec<usize>> {
    let pb = pattern.as_bytes();
    let mut out: Vec<CompMatch> = Vec::new();
    let mut current = 0usize;
    let mut end = 0usize;
    let ns_list = Rc::new(tw(|t| xpath_ns_list(t, node)));
    let fail = |c: &mut Compiler| {
        let _ = c;
        None
    };
    while current < pb.len() {
        let start = current;
        while current < pb.len() && matches!(pb[current], 0x20 | 0x09 | 0x0A | 0x0D) {
            current += 1;
        }
        end = current;
        let mut level = 0;
        while end < pb.len() && (pb[end] != b'|' || level != 0) {
            if pb[end] == b'[' {
                level += 1;
            } else if pb[end] == b']' {
                level -= 1;
            } else if pb[end] == b'\'' {
                end += 1;
                while end < pb.len() && pb[end] != b'\'' {
                    end += 1;
                }
            } else if pb[end] == b'"' {
                end += 1;
                while end < pb.len() && pb[end] != b'"' {
                    end += 1;
                }
            }
            if end >= pb.len() {
                break;
            }
            end += 1;
        }
        let end_c = end.min(pb.len());
        if current == end_c {
            transform_error(
                c.vm,
                false,
                false,
                Some(node),
                "xsltCompilePattern : NULL pattern\n",
            );
            return fail(c);
        }
        let base = String::from_utf8_lossy(&pb[start..end_c]).into_owned();
        let mut ctxt = PCtx {
            s: base.as_bytes(),
            cur: current - start,
            error: false,
            elem: node,
            steps: Vec::new(),
            priority: 0.0,
        };
        compile_location_path_pattern(c, &mut ctxt);
        if ctxt.error {
            transform_error(
                c.vm,
                false,
                style.is_some(),
                Some(node),
                &format!("xsltCompilePattern : failed to compile '{}'\n", base),
            );
            if let Some(s) = style {
                c.sheet.styles[s].errors += 1;
            }
            return fail(c);
        }
        let priority = ctxt.priority;
        let mut steps = std::mem::take(&mut ctxt.steps);
        drop(ctxt);
        // xsltReverseCompMatch
        steps.reverse();
        steps.push(PStep {
            op: POp::End,
            value: None,
            value2: None,
            value3: None,
            comp: None,
        });
        let mut pat_str = base.clone();
        let mut direct = false;
        for i in 0..steps.len().saturating_sub(1) {
            if steps[i].op == POp::Predicate && steps[i + 1].op == POp::Predicate {
                direct = true;
                if !pat_str.starts_with('/') {
                    pat_str = format!("//{}", pat_str);
                }
                break;
            }
        }
        let mut m = CompMatch {
            priority,
            pattern: pat_str,
            mode: None,
            mode_uri: None,
            template: None,
            node,
            direct,
            ns_list: ns_list.clone(),
            steps,
        };
        if m.priority == 0.0 {
            let s0 = &m.steps[0];
            let s1op = m.steps.get(1).map(|x| x.op).unwrap_or(POp::End);
            if matches!(s0.op, POp::Elem | POp::Attr | POp::Pi)
                && s0.value.is_some()
                && s1op == POp::End
            {
                // default priority 0
            } else if s0.op == POp::Attr && s0.value2.is_some() && s1op == POp::End {
                m.priority = -0.25;
            } else if s0.op == POp::Ns && s0.value.is_some() && s1op == POp::End {
                m.priority = -0.25;
            } else if s0.op == POp::Attr
                && s0.value.is_none()
                && s0.value2.is_none()
                && s1op == POp::End
            {
                m.priority = -0.5;
            } else if matches!(
                s0.op,
                POp::Pi | POp::Text | POp::All | POp::Node | POp::Comment
            ) && s1op == POp::End
            {
                m.priority = -0.5;
            } else {
                m.priority = 0.5;
            }
        }
        out.push(m);
        if end < pb.len() && pb[end] == b'|' {
            end += 1;
        }
        current = end;
    }
    if end == 0 {
        transform_error(
            c.vm,
            false,
            style.is_some(),
            Some(node),
            "xsltCompilePattern : NULL pattern\n",
        );
        if let Some(s) = style {
            c.sheet.styles[s].errors += 1;
        }
        return None;
    }
    let mut idx = Vec::new();
    for m in out {
        c.sheet.pats.push(m);
        idx.push(c.sheet.pats.len() - 1);
    }
    Some(idx)
}
