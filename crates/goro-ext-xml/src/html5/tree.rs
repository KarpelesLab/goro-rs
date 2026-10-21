//! Tree construction helpers: a port of lexbor's html/tree.c,
//! html/tree/open_elements.c and html/tree/active_formatting.c.

use super::dom::{LAttr, LKind};
use super::*;

pub const CAT_ORDINARY: u8 = 0x01;
pub const CAT_SPECIAL: u8 = 0x02;
pub const CAT_SCOPE: u8 = 0x08;
pub const CAT_SCOPE_LIST_ITEM: u8 = 0x10;
pub const CAT_SCOPE_BUTTON: u8 = 0x20;
pub const CAT_SCOPE_TABLE: u8 = 0x40;
pub const CAT_SCOPE_SELECT: u8 = 0x80;

// Tree error messages (dom_lexbor_tree_error_code_to_string)
pub const UNTO: &str = "unexpected-token";
pub const UNCLTO: &str = "unexpected-closed-token";
pub const NUCH: &str = "null-character";
pub const UNTOININMO: &str = "unexpected-token-in-initial-mode";
pub const BADOTOININMO: &str = "bad-doctype-token-in-initial-mode";
pub const DOTOINBEHTMO: &str = "doctype-token-in-before-html-mode";
pub const UNCLTOINBEHTMO: &str = "unexpected-closed-token-in-before-html-mode";
pub const DOTOINBEHEMO: &str = "doctype-token-in-before-head-mode";
pub const UNCLTOINBEHEMO: &str = "unexpected-closed_token-in-before-head-mode";
pub const DOTOINHEMO: &str = "doctype-token-in-head-mode";
pub const NOVOHTELSTTAWITRSO: &str = "non-void-html-element-start-tag-with-trailing-solidus";
pub const HETOINHEMO: &str = "head-token-in-head-mode";
pub const UNCLTOINHEMO: &str = "unexpected-closed-token-in-head-mode";
pub const TECLTOWIOPINHEMO: &str = "template-closed-token-without-opening-in-head-mode";
pub const TEELISNOCUINHEMO: &str = "template-element-is-not-current-in-head-mode";
pub const DOTOINHENOMO: &str = "doctype-token-in-head-noscript-mode";
pub const DOTOAFHEMO: &str = "doctype-token-after-head-mode";
pub const HETOAFHEMO: &str = "head-token-after-head-mode";
pub const DOTOINBOMO: &str = "doctype-token-in-body-mode";
pub const BAENOPELISWR: &str = "bad-ending-open-elements-is-wrong";
pub const OPELISWR: &str = "open-elements-is-wrong";
pub const UNELINOPELST: &str = "unexpected-element-in-open-elements-stack";
pub const MIELINOPELST: &str = "missing-element-in-open-elements-stack";
pub const NOBOELINSC: &str = "no-body-element-in-scope";
pub const MIELINSC: &str = "missing-element-in-scope";
pub const UNELINSC: &str = "unexpected-element-in-scope";
pub const UNELINACFOST: &str = "unexpected-element-in-active-formatting-stack";
pub const UNENOFFI: &str = "unexpected-end-of-file";
pub const CHINTATE: &str = "characters-in-table-text";
pub const DOTOINTAMO: &str = "doctype-token-in-table-mode";
pub const DOTOINSEMO: &str = "doctype-token-in-select-mode";
pub const DOTOAFBOMO: &str = "doctype-token-after-body-mode";
pub const DOTOINFRMO: &str = "doctype-token-in-frameset-mode";
pub const DOTOAFFRMO: &str = "doctype-token-after-frameset-mode";
pub const DOTOFOCOMO: &str = "doctype-token-foreign-content-mode";

static SVG_ATTR_ADJUST: [(&str, &str); 58] = [
    ("attributename", "attributeName"),
    ("attributetype", "attributeType"),
    ("basefrequency", "baseFrequency"),
    ("baseprofile", "baseProfile"),
    ("calcmode", "calcMode"),
    ("clippathunits", "clipPathUnits"),
    ("diffuseconstant", "diffuseConstant"),
    ("edgemode", "edgeMode"),
    ("filterunits", "filterUnits"),
    ("glyphref", "glyphRef"),
    ("gradienttransform", "gradientTransform"),
    ("gradientunits", "gradientUnits"),
    ("kernelmatrix", "kernelMatrix"),
    ("kernelunitlength", "kernelUnitLength"),
    ("keypoints", "keyPoints"),
    ("keysplines", "keySplines"),
    ("keytimes", "keyTimes"),
    ("lengthadjust", "lengthAdjust"),
    ("limitingconeangle", "limitingConeAngle"),
    ("markerheight", "markerHeight"),
    ("markerunits", "markerUnits"),
    ("markerwidth", "markerWidth"),
    ("maskcontentunits", "maskContentUnits"),
    ("maskunits", "maskUnits"),
    ("numoctaves", "numOctaves"),
    ("pathlength", "pathLength"),
    ("patterncontentunits", "patternContentUnits"),
    ("patterntransform", "patternTransform"),
    ("patternunits", "patternUnits"),
    ("pointsatx", "pointsAtX"),
    ("pointsaty", "pointsAtY"),
    ("pointsatz", "pointsAtZ"),
    ("preservealpha", "preserveAlpha"),
    ("preserveaspectratio", "preserveAspectRatio"),
    ("primitiveunits", "primitiveUnits"),
    ("refx", "refX"),
    ("refy", "refY"),
    ("repeatcount", "repeatCount"),
    ("repeatdur", "repeatDur"),
    ("requiredextensions", "requiredExtensions"),
    ("requiredfeatures", "requiredFeatures"),
    ("specularconstant", "specularConstant"),
    ("specularexponent", "specularExponent"),
    ("spreadmethod", "spreadMethod"),
    ("startoffset", "startOffset"),
    ("stddeviation", "stdDeviation"),
    ("stitchtiles", "stitchTiles"),
    ("surfacescale", "surfaceScale"),
    ("systemlanguage", "systemLanguage"),
    ("tablevalues", "tableValues"),
    ("targetx", "targetX"),
    ("targety", "targetY"),
    ("textlength", "textLength"),
    ("viewbox", "viewBox"),
    ("viewtarget", "viewTarget"),
    ("xchannelselector", "xChannelSelector"),
    ("ychannelselector", "yChannelSelector"),
    ("zoomandpan", "zoomAndPan"),
];

/// (name, prefix, local name, namespace)
static FOREIGN_ATTR_ADJUST: [(&str, &str, &str, u8); 11] = [
    ("xlink:actuate", "xlink", "actuate", NS_XLINK),
    ("xlink:arcrole", "xlink", "arcrole", NS_XLINK),
    ("xlink:href", "xlink", "href", NS_XLINK),
    ("xlink:role", "xlink", "role", NS_XLINK),
    ("xlink:show", "xlink", "show", NS_XLINK),
    ("xlink:title", "xlink", "title", NS_XLINK),
    ("xlink:type", "xlink", "type", NS_XLINK),
    ("xml:lang", "xml", "lang", NS_XML),
    ("xml:space", "xml", "space", NS_XML),
    ("xmlns", "", "xmlns", NS_XMLNS),
    ("xmlns:xlink", "xmlns", "xlink", NS_XMLNS),
];

/// lxb_html_tag_is_category
pub fn tag_is_category(tag: u32, ns: u8, cat: u8) -> bool {
    if tag < _LAST_ENTRY && ns < 8 {
        return TAG_CATS[tag as usize][ns as usize] & cat != 0;
    }
    (CAT_ORDINARY | CAT_SCOPE_SELECT) & cat != 0
}

/// lxb_html_tag_is_void
pub fn tag_is_void(tag: u32) -> bool {
    matches!(
        tag,
        AREA | BASE | BR | COL | EMBED | HR | IMG | INPUT | LINK | META | SOURCE | TRACK | WBR
    )
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0C | b'\r')
}

impl Parser {
    // ---------------------------------------------------------------------
    // Entry point from the tokenizer

    pub(crate) fn tree_token(&mut self, tok: &mut Token) {
        self.tkz_token_line = tok.line;
        let mut guard = 0u32;
        while !self.construction_dispatcher(tok) {
            guard += 1;
            if guard > 100_000 {
                break;
            }
        }
    }

    fn construction_dispatcher(&mut self, tok: &mut Token) -> bool {
        let adjusted = match self.adjusted_current_node() {
            None => return self.mode_call(self.mode, tok),
            Some(a) => a,
        };
        let (ans, atag) = (self.dom.nodes[adjusted].ns, self.dom.nodes[adjusted].tag);
        if ans == NS_HTML {
            return self.mode_call(self.mode, tok);
        }
        if self.mathml_text_integration_point(adjusted) {
            if tok.typ & TOKEN_CLOSE == 0 && tok.tag_id != MGLYPH && tok.tag_id != MALIGNMARK {
                return self.mode_call(self.mode, tok);
            }
            if tok.tag_id == _TEXT {
                return self.mode_call(self.mode, tok);
            }
        }
        if atag == ANNOTATION_XML
            && ans == NS_MATH
            && tok.typ & TOKEN_CLOSE == 0
            && tok.tag_id == SVG
        {
            return self.mode_call(self.mode, tok);
        }
        if self.html_integration_point(adjusted)
            && (tok.typ & TOKEN_CLOSE == 0 || tok.tag_id == _TEXT)
        {
            return self.mode_call(self.mode, tok);
        }
        if tok.tag_id == _END_OF_FILE {
            return self.mode_call(self.mode, tok);
        }
        self.foreign_content(tok)
    }

    // ---------------------------------------------------------------------
    // Node helpers

    pub(crate) fn node_is(&self, n: NodeId, tag: u32) -> bool {
        let x = &self.dom.nodes[n];
        x.tag == tag && x.ns == NS_HTML
    }

    pub(crate) fn ntag(&self, n: NodeId) -> u32 {
        self.dom.nodes[n].tag
    }

    pub(crate) fn nns(&self, n: NodeId) -> u8 {
        self.dom.nodes[n].ns
    }

    pub(crate) fn current_node(&self) -> Option<NodeId> {
        self.open_elements.last().copied()
    }

    pub(crate) fn adjusted_current_node(&self) -> Option<NodeId> {
        if self.fragment.is_some() && self.open_elements.len() == 1 {
            return self.fragment;
        }
        self.current_node()
    }

    pub(crate) fn parse_error(&mut self, tok: &Token, msg: &'static str) {
        self.tree_errors.push(TreeError {
            msg,
            line: tok.line,
            column: tok.column,
            length: tok.end.wrapping_sub(tok.begin),
        });
    }

    pub(crate) fn tok_text(&self, tok: &Token) -> Vec<u8> {
        let s = tok.text_start.min(self.tmp.len());
        let e = tok.text_end.min(self.tmp.len()).max(s);
        self.tmp[s..e].to_vec()
    }

    /// lxb_html_token_data_skip_ws_begin (note: lexbor skips CR, not FF)
    pub(crate) fn tok_skip_ws_begin(&self, tok: &mut Token) {
        let mut i = tok.text_start;
        while i < tok.text_end {
            match self.tmp.get(i).copied().unwrap_or(0) {
                0x09 | 0x0A | 0x0D | 0x20 => {}
                _ => break,
            }
            i += 1;
        }
        tok.begin = tok.begin.wrapping_add(i - tok.text_start);
        tok.text_start = i;
    }

    /// lxb_html_token_data_skip_one_newline_begin
    pub(crate) fn tok_skip_one_newline_begin(&self, tok: &mut Token) {
        if tok.text_start < tok.text_end && self.tmp.get(tok.text_start) == Some(&0x0A) {
            tok.begin = tok.begin.wrapping_add(1);
            tok.text_start += 1;
        }
    }

    /// lxb_html_token_data_split_ws_begin: returns the whitespace token
    pub(crate) fn tok_split_ws_begin(&self, tok: &mut Token) -> Token {
        let mut ws = tok.clone();
        self.tok_skip_ws_begin(tok);
        if tok.text_start == tok.text_end {
            return ws;
        }
        if tok.text_start == ws.text_start {
            return Token::default();
        }
        ws.end = tok.begin;
        ws.text_end = tok.text_start;
        ws
    }

    // ---------------------------------------------------------------------
    // Open elements / active formatting

    pub(crate) fn oe_pop(&mut self) -> Option<NodeId> {
        self.open_elements.pop()
    }

    pub(crate) fn oe_get(&self, i: usize) -> Option<NodeId> {
        self.open_elements.get(i).copied()
    }

    pub(crate) fn oe_remove_by_node(&mut self, n: NodeId) {
        if let Some(i) = self.open_elements.iter().rposition(|&x| x == n) {
            self.open_elements.remove(i);
        }
    }

    /// lxb_html_tree_open_elements_pop_until_tag_id (exclude=true pops it too)
    pub(crate) fn oe_pop_until_tag(&mut self, tag: u32, ns: u8, exclude: bool) {
        while let Some(n) = self.open_elements.pop() {
            if self.dom.nodes[n].tag == tag && self.dom.nodes[n].ns == ns {
                if !exclude {
                    self.open_elements.push(n);
                }
                break;
            }
        }
    }

    pub(crate) fn oe_pop_until_h123456(&mut self) {
        while let Some(n) = self.open_elements.pop() {
            if matches!(self.ntag(n), H1 | H2 | H3 | H4 | H5 | H6) && self.nns(n) == NS_HTML {
                return;
            }
        }
    }

    pub(crate) fn oe_pop_until_td_th(&mut self) {
        while let Some(n) = self.open_elements.pop() {
            if matches!(self.ntag(n), TD | TH) && self.nns(n) == NS_HTML {
                return;
            }
        }
    }

    pub(crate) fn oe_pop_until_node(&mut self, node: NodeId, exclude: bool) {
        while let Some(n) = self.open_elements.pop() {
            if n == node {
                if !exclude {
                    self.open_elements.push(n);
                }
                break;
            }
        }
    }

    pub(crate) fn oe_find(&self, tag: u32, ns: u8) -> Option<(NodeId, usize)> {
        self.open_elements
            .iter()
            .enumerate()
            .find(|(_, n)| self.ntag(**n) == tag && self.nns(**n) == ns)
            .map(|(i, n)| (*n, i))
    }

    pub(crate) fn oe_find_reverse(&self, tag: u32, ns: u8) -> Option<(NodeId, usize)> {
        self.open_elements
            .iter()
            .enumerate()
            .rev()
            .find(|(_, n)| self.ntag(**n) == tag && self.nns(**n) == ns)
            .map(|(i, n)| (*n, i))
    }

    pub(crate) fn oe_find_by_node_reverse(&self, node: NodeId) -> Option<usize> {
        self.open_elements.iter().rposition(|&x| x == node)
    }

    pub(crate) fn af_find_by_node_reverse(&self, node: NodeId) -> Option<usize> {
        self.active_formatting.iter().rposition(|&x| x == node)
    }

    pub(crate) fn af_remove_by_node(&mut self, node: NodeId) {
        if let Some(i) = self.af_find_by_node_reverse(node) {
            self.active_formatting.remove(i);
        }
    }

    pub(crate) fn af_up_to_last_marker(&mut self) {
        while let Some(n) = self.active_formatting.pop() {
            if n == MARKER {
                break;
            }
        }
    }

    pub(crate) fn af_push_marker(&mut self) {
        self.active_formatting.push(MARKER);
    }

    /// lxb_html_tree_active_formatting_reconstruct_elements
    pub(crate) fn af_reconstruct(&mut self) {
        if self.active_formatting.is_empty() {
            return;
        }
        let mut idx = self.active_formatting.len() - 1;
        let last = self.active_formatting[idx];
        if last == MARKER || self.oe_find_by_node_reverse(last).is_some() {
            return;
        }
        while idx != 0 {
            idx -= 1;
            let n = self.active_formatting[idx];
            if n == MARKER || self.oe_find_by_node_reverse(n).is_some() {
                idx += 1;
                break;
            }
        }
        while idx < self.active_formatting.len() {
            let node = self.active_formatting[idx];
            let mut fake = Token {
                tag_id: self.ntag(node),
                base_element: Some(node),
                ..Default::default()
            };
            let el = self.insert_foreign_element(&mut fake, NS_HTML);
            self.active_formatting[idx] = el;
            idx += 1;
        }
    }

    /// lxb_html_tree_active_formatting_between_last_marker
    pub(crate) fn af_between_last_marker(&self, tag: u32) -> Option<NodeId> {
        for &n in self.active_formatting.iter().rev() {
            if n == MARKER {
                return None;
            }
            if self.ntag(n) == tag && self.nns(n) == NS_HTML {
                return Some(n);
            }
        }
        None
    }

    /// lxb_html_tree_active_formatting_push_with_check_dupl
    pub(crate) fn af_push_with_check_dupl(&mut self, node: NodeId) {
        let mut idx = self.active_formatting.len();
        let mut earliest = idx.saturating_sub(1);
        let mut count = 0;
        while idx != 0 {
            idx -= 1;
            let n = self.active_formatting[idx];
            if n == MARKER {
                break;
            }
            if self.ntag(n) == self.ntag(node)
                && self.nns(n) == self.nns(node)
                && self.dom.element_compare(n, node)
            {
                count += 1;
                earliest = idx;
            }
        }
        if count >= 3 && earliest < self.active_formatting.len() {
            self.active_formatting.remove(earliest);
        }
        self.active_formatting.push(node);
    }

    // ---------------------------------------------------------------------
    // Insertion

    /// lxb_html_tree_appropriate_place_inserting_node: (node, insert-before)
    pub(crate) fn appropriate_place(
        &self,
        override_target: Option<NodeId>,
    ) -> Option<(NodeId, bool)> {
        let target = override_target.or_else(|| self.current_node())?;
        let mut before = false;
        let adjusted;
        if self.foster_parenting
            && self.nns(target) == NS_HTML
            && matches!(self.ntag(target), TABLE | TBODY | TFOOT | THEAD | TR)
        {
            let last_temp = self.oe_find_reverse(TEMPLATE, NS_HTML);
            let last_table = self.oe_find_reverse(TABLE, NS_HTML);
            if let Some((t, ti)) = last_temp {
                if last_table.is_none() || ti > last_table.unwrap().1 {
                    return self.dom.nodes[t].template_content.map(|c| (c, false));
                }
            }
            match last_table {
                None => adjusted = self.oe_get(0),
                Some((tb, tbi)) => {
                    if self.dom.nodes[tb].parent.is_some() {
                        adjusted = Some(tb);
                        before = true;
                    } else {
                        adjusted = if tbi > 0 { self.oe_get(tbi - 1) } else { None };
                    }
                }
            }
        } else {
            adjusted = Some(target);
        }
        let mut adjusted = adjusted?;
        if self.node_is(adjusted, TEMPLATE) {
            if let Some(c) = self.dom.nodes[adjusted].template_content {
                adjusted = c;
            }
        }
        Some((adjusted, before))
    }

    pub(crate) fn insert_node(&mut self, to: NodeId, n: NodeId, before: bool) {
        if before {
            self.dom.insert_before(to, n);
        } else {
            self.dom.append_child(to, n);
        }
    }

    pub(crate) fn insert_html_element(&mut self, tok: &mut Token) -> NodeId {
        self.insert_foreign_element(tok, NS_HTML)
    }

    /// lxb_html_tree_insert_foreign_element
    pub(crate) fn insert_foreign_element(&mut self, tok: &mut Token, ns: u8) -> NodeId {
        let place = self.appropriate_place(None);
        let el = self.create_element_for_token(tok, ns);
        if let Some((pos, before)) = place {
            self.insert_node(pos, el, before);
        }
        self.open_elements.push(el);
        el
    }

    /// lxb_html_tree_create_element_for_token
    pub(crate) fn create_element_for_token(&mut self, tok: &Token, ns: u8) -> NodeId {
        let el = self.dom.new_element(tok.tag_id, ns);
        self.dom.nodes[el].line = tok.line;
        match tok.base_element {
            None => self.append_attributes(el, tok, ns),
            Some(from) => self.append_attributes_from_element(el, from),
        }
        el
    }

    /// lxb_html_tree_append_attributes
    pub(crate) fn append_attributes(&mut self, el: NodeId, tok: &Token, ns: u8) {
        for ta in &tok.attrs {
            let name = match &ta.name {
                Some(n) => n.clone(),
                None => continue,
            };
            if self.dom.attr_find(el, &name).is_some() {
                continue;
            }
            let mut a = LAttr {
                local: name,
                qualified: None,
                has_prefix: false,
                ns,
                value: if ta.value_begin != 0 {
                    Some(ta.value.clone().unwrap_or_default())
                } else {
                    None
                },
            };
            self.before_append(&mut a);
            self.dom.nodes[el].attrs.push(a);
        }
    }

    fn append_attributes_from_element(&mut self, el: NodeId, from: NodeId) {
        let attrs = self.dom.nodes[from].attrs.clone();
        for mut a in attrs {
            self.before_append(&mut a);
            self.dom.nodes[el].attrs.push(a);
        }
    }

    fn before_append(&self, a: &mut LAttr) {
        match self.before_append_attr {
            AttrAdjust::None => {}
            AttrAdjust::MathMl => {
                if a.local == b"definitionurl" {
                    a.qualified = Some(b"definitionURL".to_vec());
                }
                adjust_foreign(a);
            }
            AttrAdjust::Svg => {
                if let Some((_, to)) = SVG_ATTR_ADJUST
                    .iter()
                    .find(|(from, _)| a.local == from.as_bytes())
                {
                    a.qualified = Some(to.as_bytes().to_vec());
                }
                adjust_foreign(a);
            }
        }
    }

    pub(crate) fn set_attr_adjust_mathml(&mut self) {
        self.before_append_attr = AttrAdjust::MathMl;
    }
    pub(crate) fn set_attr_adjust_svg(&mut self) {
        self.before_append_attr = AttrAdjust::Svg;
    }
    pub(crate) fn clear_attr_adjust(&mut self) {
        self.before_append_attr = AttrAdjust::None;
    }

    /// lxb_html_tree_insert_character
    pub(crate) fn insert_character(&mut self, tok: &Token) {
        let data = self.tok_text(tok);
        self.insert_character_for_data(data);
    }

    /// lxb_html_tree_insert_character_for_data
    pub(crate) fn insert_character_for_data(&mut self, data: Vec<u8>) {
        let (pos, before) = match self.appropriate_place(None) {
            Some(p) => p,
            None => return,
        };
        if self.node_is(pos, _DOCUMENT) {
            return;
        }
        let existing = if before {
            self.dom.nodes[pos].prev
        } else {
            self.dom.nodes[pos].last
        };
        if let Some(t) = existing {
            if self.dom.nodes[t].kind == LKind::Text {
                self.dom.nodes[t].data.extend_from_slice(&data);
                return;
            }
        }
        let text = self.dom.new_text(data);
        self.dom.nodes[text].line = self.tkz_token_line;
        self.insert_node(pos, text, before);
    }

    /// lxb_html_tree_insert_comment
    pub(crate) fn insert_comment(&mut self, tok: &Token, pos: Option<NodeId>) {
        let (pos, before) = match pos {
            Some(p) => (p, false),
            None => match self.appropriate_place(None) {
                Some(p) => p,
                None => return,
            },
        };
        let data = self.tok_text(tok);
        let ns = self.nns(pos);
        let c = self.dom.new_comment(ns, data);
        self.dom.nodes[c].line = tok.line;
        self.insert_node(pos, c, before);
    }

    /// lxb_html_tree_create_document_type_from_token +
    /// lxb_html_token_doctype_parse
    pub(crate) fn create_doctype_from_token(&mut self, tok: &Token) -> NodeId {
        let dt = self.dom.new_doctype();
        let mut it = tok.attrs.iter();
        if let Some(first) = it.next() {
            self.dom.nodes[dt].data = first.name.clone().unwrap_or_default();
            if let Some(second) = it.next() {
                let nm = second.name.as_deref();
                if nm == Some(b"public") {
                    if second.value_begin == 0 {
                        return dt;
                    }
                    self.dom.nodes[dt].public_id = second.value.clone().unwrap_or_default();
                    if let Some(third) = it.next() {
                        self.dom.nodes[dt].system_id = third.value.clone().unwrap_or_default();
                    }
                } else if nm == Some(b"system") {
                    if second.value_begin == 0 {
                        return dt;
                    }
                    self.dom.nodes[dt].system_id = second.value.clone().unwrap_or_default();
                }
            }
        }
        dt
    }

    pub(crate) fn generic_rawtext_parsing(&mut self, tok: &mut Token) {
        self.insert_html_element(tok);
        self.set_tmp_tag_id(tok.tag_id);
        self.set_tkz_state(St::RawtextBefore);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    pub(crate) fn generic_rcdata_parsing(&mut self, tok: &mut Token) {
        self.insert_html_element(tok);
        self.set_tmp_tag_id(tok.tag_id);
        self.set_tkz_state(St::RcdataBefore);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    /// lxb_html_tree_generate_implied_end_tags
    pub(crate) fn generate_implied_end_tags(&mut self, ex_tag: u32, ex_ns: u8) {
        while let Some(n) = self.current_node() {
            match self.ntag(n) {
                DD | DT | LI | OPTGROUP | OPTION | P | RB | RP | RT | RTC => {
                    if self.ntag(n) == ex_tag && self.nns(n) == ex_ns {
                        return;
                    }
                    self.oe_pop();
                }
                _ => return,
            }
        }
    }

    pub(crate) fn generate_all_implied_end_tags_thoroughly(&mut self, ex_tag: u32, ex_ns: u8) {
        while let Some(n) = self.current_node() {
            match self.ntag(n) {
                CAPTION | COLGROUP | DD | DT | LI | OPTGROUP | OPTION | P | RB | RP | RT | RTC
                | TBODY | TD | TFOOT | TH | THEAD | TR => {
                    if self.ntag(n) == ex_tag && self.nns(n) == ex_ns {
                        return;
                    }
                    self.oe_pop();
                }
                _ => return,
            }
        }
    }

    /// lxb_html_tree_reset_insertion_mode_appropriately
    pub(crate) fn reset_insertion_mode_appropriately(&mut self) {
        let mut idx = self.open_elements.len();
        let mut last = false;
        while idx != 0 {
            idx -= 1;
            let mut node = self.open_elements[idx];
            if idx == 0 {
                last = true;
                if let Some(f) = self.fragment {
                    node = f;
                }
            }
            if self.nns(node) != NS_HTML {
                if last {
                    self.mode = Mode::InBody;
                    return;
                }
                continue;
            }
            if self.ntag(node) == SELECT {
                if last {
                    self.mode = Mode::InSelect;
                    return;
                }
                let mut ancestor = idx;
                loop {
                    if ancestor == 0 {
                        self.mode = Mode::InSelect;
                        return;
                    }
                    ancestor -= 1;
                    let an = self.open_elements[ancestor];
                    if self.node_is(an, TEMPLATE) {
                        self.mode = Mode::InSelect;
                        return;
                    } else if self.node_is(an, TABLE) {
                        self.mode = Mode::InSelectInTable;
                        return;
                    }
                }
            }
            match self.ntag(node) {
                TD | TH => {
                    if !last {
                        self.mode = Mode::InCell;
                        return;
                    }
                }
                TR => {
                    self.mode = Mode::InRow;
                    return;
                }
                TBODY | TFOOT | THEAD => {
                    self.mode = Mode::InTableBody;
                    return;
                }
                CAPTION => {
                    self.mode = Mode::InCaption;
                    return;
                }
                COLGROUP => {
                    self.mode = Mode::InColumnGroup;
                    return;
                }
                TABLE => {
                    self.mode = Mode::InTable;
                    return;
                }
                TEMPLATE => {
                    self.mode = self.template_modes.last().copied().unwrap_or(Mode::InBody);
                    return;
                }
                HEAD => {
                    if !last {
                        self.mode = Mode::InHead;
                        return;
                    }
                }
                BODY => {
                    self.mode = Mode::InBody;
                    return;
                }
                FRAMESET => {
                    self.mode = Mode::InFrameset;
                    return;
                }
                HTML => {
                    self.mode = if self.head.is_none() {
                        Mode::BeforeHead
                    } else {
                        Mode::AfterHead
                    };
                    return;
                }
                _ => {}
            }
            if last {
                self.mode = Mode::InBody;
                return;
            }
        }
    }

    /// lxb_html_tree_element_in_scope
    pub(crate) fn element_in_scope(&self, tag: u32, ns: u8, cat: u8) -> Option<NodeId> {
        for &n in self.open_elements.iter().rev() {
            if self.ntag(n) == tag && self.nns(n) == ns {
                return Some(n);
            }
            if tag_is_category(self.ntag(n), self.nns(n), cat) {
                return None;
            }
        }
        None
    }

    pub(crate) fn element_in_scope_by_node(&self, by: NodeId, cat: u8) -> Option<NodeId> {
        for &n in self.open_elements.iter().rev() {
            if n == by {
                return Some(n);
            }
            if tag_is_category(self.ntag(n), self.nns(n), cat) {
                return None;
            }
        }
        None
    }

    pub(crate) fn element_in_scope_h123456(&self) -> Option<NodeId> {
        for &n in self.open_elements.iter().rev() {
            if matches!(self.ntag(n), H1 | H2 | H3 | H4 | H5 | H6) && self.nns(n) == NS_HTML {
                return Some(n);
            }
            if tag_is_category(self.ntag(n), NS_HTML, CAT_SCOPE) {
                return None;
            }
        }
        None
    }

    pub(crate) fn element_in_scope_tbody_thead_tfoot(&self) -> Option<NodeId> {
        for &n in self.open_elements.iter().rev() {
            if matches!(self.ntag(n), TBODY | THEAD | TFOOT) && self.nns(n) == NS_HTML {
                return Some(n);
            }
            if tag_is_category(self.ntag(n), NS_HTML, CAT_SCOPE_TABLE) {
                return None;
            }
        }
        None
    }

    pub(crate) fn element_in_scope_td_th(&self) -> Option<NodeId> {
        for &n in self.open_elements.iter().rev() {
            if matches!(self.ntag(n), TD | TH) && self.nns(n) == NS_HTML {
                return Some(n);
            }
            if tag_is_category(self.ntag(n), NS_HTML, CAT_SCOPE_TABLE) {
                return None;
            }
        }
        None
    }

    /// lxb_html_tree_check_scope_element
    pub(crate) fn check_scope_element(&self) -> bool {
        self.open_elements.iter().any(|&n| {
            matches!(
                self.ntag(n),
                DD | DT
                    | LI
                    | OPTGROUP
                    | OPTION
                    | P
                    | RB
                    | RP
                    | RT
                    | RTC
                    | TBODY
                    | TD
                    | TFOOT
                    | TH
                    | THEAD
                    | TR
                    | BODY
                    | HTML
            )
        })
    }

    pub(crate) fn close_p_element(&mut self, tok: &Token) {
        self.generate_implied_end_tags(P, NS_HTML);
        let cur = self.current_node();
        if cur.map(|c| !self.node_is(c, P)).unwrap_or(true) {
            self.parse_error(tok, UNELINOPELST);
        }
        self.oe_pop_until_tag(P, NS_HTML, true);
    }

    pub(crate) fn acknowledge_self_closing(&mut self, tok: &Token) {
        if tok.typ & TOKEN_CLOSE_SELF == 0 {
            return;
        }
        if !tag_is_void(tok.tag_id) {
            self.parse_error(tok, NOVOHTELSTTAWITRSO);
        }
    }

    pub(crate) fn mathml_text_integration_point(&self, n: NodeId) -> bool {
        self.nns(n) == NS_MATH && matches!(self.ntag(n), MI | MO | MN | MS | MTEXT)
    }

    /// lxb_html_tree_html_integration_point
    pub(crate) fn html_integration_point(&self, n: NodeId) -> bool {
        let (ns, tag) = (self.nns(n), self.ntag(n));
        if ns == NS_MATH && tag == ANNOTATION_XML {
            let i = match self.dom.attr_find(n, b"encoding") {
                Some(i) => i,
                None => return false,
            };
            let v = match &self.dom.nodes[n].attrs[i].value {
                Some(v) => v,
                None => return false,
            };
            return v.eq_ignore_ascii_case(b"text/html")
                || v.eq_ignore_ascii_case(b"application/xhtml+xml");
        }
        ns == NS_SVG && matches!(tag, FOREIGNOBJECT | DESC | TITLE)
    }

    /// lxb_html_tree_adoption_agency_algorithm
    pub(crate) fn adoption_agency(&mut self, tok: &Token) -> bool {
        let subject = tok.tag_id;
        let node = match self.current_node() {
            Some(n) => n,
            None => return false,
        };
        if self.node_is(node, subject) && self.af_find_by_node_reverse(node).is_none() {
            self.oe_pop();
            return false;
        }
        let mut outer = 0;
        while outer < 8 {
            outer += 1;
            let mut formatting_index = 0;
            let mut formatting_element = None;
            for idx in (0..self.active_formatting.len()).rev() {
                let n = self.active_formatting[idx];
                if n == MARKER {
                    return true;
                } else if self.ntag(n) == subject {
                    formatting_index = idx;
                    formatting_element = Some(n);
                    break;
                }
            }
            let fe = match formatting_element {
                None => return true,
                Some(f) => f,
            };
            let oel_formatting_idx = match self.oe_find_by_node_reverse(fe) {
                None => {
                    self.parse_error(tok, MIELINOPELST);
                    self.af_remove_by_node(fe);
                    return false;
                }
                Some(i) => i,
            };
            if self.element_in_scope_by_node(fe, CAT_SCOPE).is_none() {
                self.parse_error(tok, MIELINSC);
                return false;
            }
            if self.current_node() != Some(fe) {
                self.parse_error(tok, UNELINOPELST);
            }
            let mut furthest_block = None;
            let mut furthest_block_idx = oel_formatting_idx;
            while furthest_block_idx < self.open_elements.len() {
                let n = self.open_elements[furthest_block_idx];
                if tag_is_category(self.ntag(n), self.nns(n), CAT_SPECIAL) {
                    furthest_block = Some(n);
                    break;
                }
                furthest_block_idx += 1;
            }
            let fb = match furthest_block {
                None => {
                    self.oe_pop_until_node(fe, true);
                    self.af_remove_by_node(fe);
                    return false;
                }
                Some(f) => f,
            };
            if oel_formatting_idx == 0 {
                return false;
            }
            let common_ancestor = self.open_elements[oel_formatting_idx - 1];
            let mut bookmark = formatting_index;
            let mut last = fb;
            let mut node_idx = furthest_block_idx;
            let mut inner = 0;
            loop {
                inner += 1;
                if node_idx == 0 {
                    return false;
                }
                node_idx -= 1;
                let mut node = self.open_elements[node_idx];
                if node == fe {
                    break;
                }
                let afe_node_idx = self.af_find_by_node_reverse(node);
                if inner > 3 && afe_node_idx.is_some() {
                    self.af_remove_by_node(node);
                    continue;
                }
                let afe_node_idx = match afe_node_idx {
                    None => {
                        self.oe_remove_by_node(node);
                        continue;
                    }
                    Some(i) => i,
                };
                let fake = Token {
                    tag_id: self.ntag(node),
                    base_element: Some(node),
                    ..Default::default()
                };
                let el = self.create_element_for_token(&fake, NS_HTML);
                node = el;
                self.active_formatting[afe_node_idx] = node;
                self.open_elements[node_idx] = node;
                if last == fb {
                    bookmark = afe_node_idx + 1;
                }
                if self.dom.nodes[last].parent.is_some() {
                    self.dom.remove(last);
                }
                self.dom.append_child(node, last);
                last = node;
            }
            if self.dom.nodes[last].parent.is_some() {
                self.dom.remove(last);
            }
            let (pos, before) = match self.appropriate_place(Some(common_ancestor)) {
                Some(p) => p,
                None => return false,
            };
            self.insert_node(pos, last, before);
            let fake = Token {
                tag_id: self.ntag(fe),
                base_element: Some(fe),
                ..Default::default()
            };
            let el = self.create_element_for_token(&fake, NS_HTML);
            for c in self.dom.children(fb) {
                self.dom.remove(c);
                self.dom.append_child(el, c);
            }
            self.dom.append_child(fb, el);
            if formatting_index < self.active_formatting.len() {
                self.active_formatting.remove(formatting_index);
            }
            if bookmark > self.active_formatting.len() {
                bookmark = self.active_formatting.len();
            }
            self.active_formatting.insert(bookmark, el);
            self.oe_remove_by_node(fe);
            let fbi = self
                .open_elements
                .iter()
                .position(|&x| x == fb)
                .unwrap_or(0);
            let at = (fbi + 1).min(self.open_elements.len());
            self.open_elements.insert(at, el);
        }
        false
    }
}

/// lxb_html_tree_adjust_foreign_attributes
fn adjust_foreign(a: &mut LAttr) {
    if let Some(&(name, prefix, local, ns)) = FOREIGN_ATTR_ADJUST
        .iter()
        .find(|(n, _, _, _)| a.local == n.as_bytes())
    {
        if !prefix.is_empty() {
            a.qualified = Some(name.as_bytes().to_vec());
            a.local = local.as_bytes().to_vec();
            a.has_prefix = true;
        }
        a.ns = ns;
    }
}

/// lexbor_str_whitespace_from_begin != length
pub(crate) fn has_non_ws(s: &[u8]) -> bool {
    s.iter().any(|&c| !is_ws(c))
}

pub(crate) fn stay_only_whitespace(s: &[u8]) -> Vec<u8> {
    s.iter().copied().filter(|&c| is_ws(c)).collect()
}
