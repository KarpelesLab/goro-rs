//! attrs.c: attribute dictionary, lookups, value checks and the anchor
//! list.

use std::collections::HashMap;

use crate::attrdefs::ATTRIBUTE_DEFS;
use crate::config;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::lexer;
use crate::message;
use crate::tags;
use crate::tmbstr::{cstr, tmbstrcasecmp, tmbstrcmp, tmbstrncmp};
use crate::utf8::get_utf8;

/// `AttrCheck`
pub type AttrCheckFn = fn(&mut Doc, NodeId, AttId);

/// _Attribute
#[derive(Debug)]
pub struct Attribute {
    pub id: AttrId,
    pub name: &'static [u8],
    pub attrchk: Option<AttrCheckFn>,
}

/// _Anchor
#[derive(Clone, Debug)]
pub struct Anchor {
    pub node: NodeId,
    pub name: Vec<u8>,
}

pub const ANCHOR_HASH_SIZE: usize = 1021;

/// TidyAttribImpl
#[derive(Clone, Debug, Default)]
pub struct AttribImpl {
    /// anchor/node lookup: ANCHOR_HASH_SIZE buckets, each in list order
    pub anchor_hash: Vec<Vec<Anchor>>,
    /// Prioritized list of attributes to write
    pub priority_attribs: Vec<Vec<u8>>,
}

// The attributes whose checker is CH_URL, CH_SCRIPT and CH_BOOL/CH_BORDER
// (CheckAttrType / IsBoolAttribute compare the check function pointers;
// identical function bodies may share an address in Rust, so the sets are
// spelled out from attribute_defs[]).
const URL_ATTRS: &[AttrId] = &[
    AttrId::BACKGROUND,
    AttrId::CITE,
    AttrId::CLASSID,
    AttrId::CODEBASE,
    AttrId::DATA,
    AttrId::DATASRC,
    AttrId::HREF,
    AttrId::ITEMTYPE,
    AttrId::LONGDESC,
    AttrId::LOWSRC,
    AttrId::PROFILE,
    AttrId::SRC,
    AttrId::USEMAP,
    AttrId::VOCAB,
    AttrId::XMLNSXLINK,
];

const SCRIPT_ATTRS: &[AttrId] = &[
    AttrId::OnAFTERUPDATE,
    AttrId::OnBEFOREUNLOAD,
    AttrId::OnBEFOREUPDATE,
    AttrId::OnBLUR,
    AttrId::OnCHANGE,
    AttrId::OnCLICK,
    AttrId::OnDATAAVAILABLE,
    AttrId::OnDATASETCHANGED,
    AttrId::OnDATASETCOMPLETE,
    AttrId::OnDBLCLICK,
    AttrId::OnERRORUPDATE,
    AttrId::OnFOCUS,
    AttrId::OnKEYDOWN,
    AttrId::OnKEYPRESS,
    AttrId::OnKEYUP,
    AttrId::OnLOAD,
    AttrId::OnMOUSEDOWN,
    AttrId::OnMOUSEMOVE,
    AttrId::OnMOUSEOUT,
    AttrId::OnMOUSEOVER,
    AttrId::OnMOUSEUP,
    AttrId::OnRESET,
    AttrId::OnROWENTER,
    AttrId::OnROWEXIT,
    AttrId::OnSELECT,
    AttrId::OnSUBMIT,
    AttrId::OnUNLOAD,
];

const BOOL_ATTRS: &[AttrId] = &[
    AttrId::ALLOWFULLSCREEN,
    AttrId::BORDER,
    AttrId::CHECKED,
    AttrId::COMPACT,
    AttrId::DECLARE,
    AttrId::DEFER,
    AttrId::DISABLED,
    AttrId::ISMAP,
    AttrId::ITEMSCOPE,
    AttrId::MULTIPLE,
    AttrId::NOHREF,
    AttrId::NORESIZE,
    AttrId::NOSHADE,
    AttrId::NOWRAP,
    AttrId::READONLY,
    AttrId::SELECTED,
    AttrId::SHOWGRID,
    AttrId::SHOWGRIDX,
    AttrId::SHOWGRIDY,
    AttrId::TRANSLATE,
    AttrId::ASYNC,
    AttrId::MUTED,
    AttrId::OPEN,
    AttrId::PLAYSINLINE,
    AttrId::INLIST,
];

#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `(uint)` of a (signed) `char`
#[inline]
fn sch(c: u8) -> u32 {
    c as i8 as i32 as u32
}

fn value_of(doc: &Doc, av: AttId) -> Vec<u8> {
    doc[av].value.clone().unwrap_or_default()
}

/// AttributeVersions()
fn attribute_versions(doc: &Doc, node: Option<NodeId>, attval: Option<AttId>) -> u32 {
    if let Some(a) = attval {
        if let Some(name) = doc[a].attribute.as_deref() {
            if tmbstrncmp(name, b"data-", 5) == 0 {
                return XH50 | HT50;
            }
        }
    }
    let Some(a) = attval else {
        return VERS_UNKNOWN;
    };
    let Some(dict) = doc[a].dict else {
        return VERS_UNKNOWN;
    };
    if let Some(n) = node {
        if let Some(t) = doc[n].tag {
            for v in doc.dict_attrvers(t) {
                if v.attribute == dict.id {
                    return v.versions;
                }
            }
        }
    }
    VERS_PROPRIETARY
}

/// NodeAttributeVersions(): the version of attribute "id" of element "node"
pub fn node_attribute_versions(doc: &Doc, node: NodeId, id: AttrId) -> u32 {
    let Some(t) = doc[node].tag else {
        return VERS_UNKNOWN;
    };
    for v in doc.dict_attrvers(t) {
        if v.attribute == id {
            return v.versions;
        }
    }
    VERS_UNKNOWN
}

/// AttributeIsProprietary()
pub fn attribute_is_proprietary(doc: &Doc, node: NodeId, attval: AttId) -> bool {
    let Some(t) = doc[node].tag else {
        return false;
    };
    if doc.dict_versions(t) & VERS_ALL == 0 {
        return false;
    }
    if attribute_versions(doc, Some(node), Some(attval)) & VERS_ALL != 0 {
        return false;
    }
    true
}

/// AttributeIsMismatched()
pub fn attribute_is_mismatched(doc: &Doc, node: NodeId, attval: AttId) -> bool {
    let Some(t) = doc[node].tag else {
        return false;
    };
    if doc.dict_versions(t) & VERS_ALL == 0 {
        return false;
    }
    let lx = doc.lxr();
    let doctype = if lx.version_emitted == 0 {
        lx.doctype
    } else {
        lx.version_emitted
    };
    if attribute_versions(doc, Some(node), Some(attval)) & doctype != 0 {
        return false;
    }
    true
}

const COLORS: &[(&[u8], &[u8])] = &[
    (b"black", b"#000000"),
    (b"green", b"#008000"),
    (b"silver", b"#C0C0C0"),
    (b"lime", b"#00FF00"),
    (b"gray", b"#808080"),
    (b"olive", b"#808000"),
    (b"white", b"#FFFFFF"),
    (b"yellow", b"#FFFF00"),
    (b"maroon", b"#800000"),
    (b"navy", b"#000080"),
    (b"red", b"#FF0000"),
    (b"blue", b"#0000FF"),
    (b"purple", b"#800080"),
    (b"teal", b"#008080"),
    (b"fuchsia", b"#FF00FF"),
    (b"aqua", b"#00FFFF"),
];

const EXTENDED_COLORS: &[(&[u8], &[u8])] = &[
    (b"aliceblue", b"#f0f8ff"),
    (b"antiquewhite", b"#faebd7"),
    (b"aquamarine", b"#7fffd4"),
    (b"azure", b"#f0ffff"),
    (b"beige", b"#f5f5dc"),
    (b"bisque", b"#ffe4c4"),
    (b"blanchedalmond", b"#ffebcd"),
    (b"blueviolet", b"#8a2be2"),
    (b"brown", b"#a52a2a"),
    (b"burlywood", b"#deb887"),
    (b"cadetblue", b"#5f9ea0"),
    (b"chartreuse", b"#7fff00"),
    (b"chocolate", b"#d2691e"),
    (b"coral", b"#ff7f50"),
    (b"cornflowerblue", b"#6495ed"),
    (b"cornsilk", b"#fff8dc"),
    (b"crimson", b"#dc143c"),
    (b"cyan", b"#00ffff"),
    (b"darkblue", b"#00008b"),
    (b"darkcyan", b"#008b8b"),
    (b"darkgoldenrod", b"#b8860b"),
    (b"darkgray", b"#a9a9a9"),
    (b"darkgreen", b"#006400"),
    (b"darkgrey", b"#a9a9a9"),
    (b"darkkhaki", b"#bdb76b"),
    (b"darkmagenta", b"#8b008b"),
    (b"darkolivegreen", b"#556b2f"),
    (b"darkorange", b"#ff8c00"),
    (b"darkorchid", b"#9932cc"),
    (b"darkred", b"#8b0000"),
    (b"darksalmon", b"#e9967a"),
    (b"darkseagreen", b"#8fbc8f"),
    (b"darkslateblue", b"#483d8b"),
    (b"darkslategray", b"#2f4f4f"),
    (b"darkslategrey", b"#2f4f4f"),
    (b"darkturquoise", b"#00ced1"),
    (b"darkviolet", b"#9400d3"),
    (b"deeppink", b"#ff1493"),
    (b"deepskyblue", b"#00bfff"),
    (b"dimgray", b"#696969"),
    (b"dimgrey", b"#696969"),
    (b"dodgerblue", b"#1e90ff"),
    (b"firebrick", b"#b22222"),
    (b"floralwhite", b"#fffaf0"),
    (b"forestgreen", b"#228b22"),
    (b"gainsboro", b"#dcdcdc"),
    (b"ghostwhite", b"#f8f8ff"),
    (b"gold", b"#ffd700"),
    (b"goldenrod", b"#daa520"),
    (b"greenyellow", b"#adff2f"),
    (b"grey", b"#808080"),
    (b"honeydew", b"#f0fff0"),
    (b"hotpink", b"#ff69b4"),
    (b"indianred", b"#cd5c5c"),
    (b"indigo", b"#4b0082"),
    (b"ivory", b"#fffff0"),
    (b"khaki", b"#f0e68c"),
    (b"lavender", b"#e6e6fa"),
    (b"lavenderblush", b"#fff0f5"),
    (b"lawngreen", b"#7cfc00"),
    (b"lemonchiffon", b"#fffacd"),
    (b"lightblue", b"#add8e6"),
    (b"lightcoral", b"#f08080"),
    (b"lightcyan", b"#e0ffff"),
    (b"lightgoldenrodyellow", b"#fafad2"),
    (b"lightgray", b"#d3d3d3"),
    (b"lightgreen", b"#90ee90"),
    (b"lightgrey", b"#d3d3d3"),
    (b"lightpink", b"#ffb6c1"),
    (b"lightsalmon", b"#ffa07a"),
    (b"lightseagreen", b"#20b2aa"),
    (b"lightskyblue", b"#87cefa"),
    (b"lightslategray", b"#778899"),
    (b"lightslategrey", b"#778899"),
    (b"lightsteelblue", b"#b0c4de"),
    (b"lightyellow", b"#ffffe0"),
    (b"limegreen", b"#32cd32"),
    (b"linen", b"#faf0e6"),
    (b"magenta", b"#ff00ff"),
    (b"mediumaquamarine", b"#66cdaa"),
    (b"mediumblue", b"#0000cd"),
    (b"mediumorchid", b"#ba55d3"),
    (b"mediumpurple", b"#9370db"),
    (b"mediumseagreen", b"#3cb371"),
    (b"mediumslateblue", b"#7b68ee"),
    (b"mediumspringgreen", b"#00fa9a"),
    (b"mediumturquoise", b"#48d1cc"),
    (b"mediumvioletred", b"#c71585"),
    (b"midnightblue", b"#191970"),
    (b"mintcream", b"#f5fffa"),
    (b"mistyrose", b"#ffe4e1"),
    (b"moccasin", b"#ffe4b5"),
    (b"navajowhite", b"#ffdead"),
    (b"oldlace", b"#fdf5e6"),
    (b"olivedrab", b"#6b8e23"),
    (b"orange", b"#ffa500"),
    (b"orangered", b"#ff4500"),
    (b"orchid", b"#da70d6"),
    (b"palegoldenrod", b"#eee8aa"),
    (b"palegreen", b"#98fb98"),
    (b"paleturquoise", b"#afeeee"),
    (b"palevioletred", b"#db7093"),
    (b"papayawhip", b"#ffefd5"),
    (b"peachpuff", b"#ffdab9"),
    (b"peru", b"#cd853f"),
    (b"pink", b"#ffc0cb"),
    (b"plum", b"#dda0dd"),
    (b"powderblue", b"#b0e0e6"),
    (b"rebeccapurple", b"#663399"),
    (b"rosybrown", b"#bc8f8f"),
    (b"royalblue", b"#4169e1"),
    (b"saddlebrown", b"#8b4513"),
    (b"salmon", b"#fa8072"),
    (b"sandybrown", b"#f4a460"),
    (b"seagreen", b"#2e8b57"),
    (b"seashell", b"#fff5ee"),
    (b"sienna", b"#a0522d"),
    (b"skyblue", b"#87ceeb"),
    (b"slateblue", b"#6a5acd"),
    (b"slategray", b"#708090"),
    (b"slategrey", b"#708090"),
    (b"snow", b"#fffafa"),
    (b"springgreen", b"#00ff7f"),
    (b"steelblue", b"#4682b4"),
    (b"tan", b"#d2b48c"),
    (b"thistle", b"#d8bfd8"),
    (b"tomato", b"#ff6347"),
    (b"turquoise", b"#40e0d0"),
    (b"violet", b"#ee82ee"),
    (b"wheat", b"#f5deb3"),
    (b"whitesmoke", b"#f5f5f5"),
    (b"yellowgreen", b"#9acd32"),
];

/// GetColorCode()
fn get_color_code(name: &[u8], use_css_colors: bool) -> Option<&'static [u8]> {
    for &(n, h) in COLORS {
        if tmbstrcasecmp(name, n) == 0 {
            return Some(h);
        }
    }
    if use_css_colors {
        for &(n, h) in EXTENDED_COLORS {
            if tmbstrcasecmp(name, n) == 0 {
                return Some(h);
            }
        }
    }
    None
}

/// GetColorName()
fn get_color_name(code: &[u8], use_css_colors: bool) -> Option<&'static [u8]> {
    for &(n, h) in COLORS {
        if tmbstrcasecmp(code, h) == 0 {
            return Some(n);
        }
    }
    if use_css_colors {
        for &(n, h) in EXTENDED_COLORS {
            if tmbstrcasecmp(code, h) == 0 {
                return Some(n);
            }
        }
    }
    None
}

/// attrsLookup(): case-insensitive lookup of the attribute dictionary
fn attrs_lookup(atnam: Option<&[u8]>) -> Option<&'static Attribute> {
    thread_local! {
        static MAP: HashMap<Vec<u8>, usize> = {
            let mut m = HashMap::new();
            for (i, a) in ATTRIBUTE_DEFS.iter().enumerate() {
                m.entry(a.name.to_ascii_lowercase()).or_insert(i);
            }
            m
        };
    }
    let atnam = cstr(atnam?);
    let key = atnam.to_ascii_lowercase();
    MAP.with(|m| m.get(&key).map(|&i| &ATTRIBUTE_DEFS[i]))
}

/// AttrGetById(): locate attributes by type
pub fn attr_get_by_id(doc: &Doc, node: NodeId, id: AttrId) -> Option<AttId> {
    let mut av = doc[node].attributes;
    while let Some(a) = av {
        if doc.attr_is(Some(a), id) {
            return Some(a);
        }
        av = doc[a].next;
    }
    None
}

/// FindAttribute(): public method for finding attribute definition by name
pub fn find_attribute(doc: &mut Doc, attval: AttId) -> Option<&'static Attribute> {
    attrs_lookup(doc[attval].attribute.as_deref())
}

/// GetAttrByName()
pub fn get_attr_by_name(doc: &Doc, node: NodeId, name: &[u8]) -> Option<AttId> {
    let mut av = doc[node].attributes;
    while let Some(a) = av {
        if let Some(n) = doc[a].attribute.as_deref() {
            if tmbstrcmp(n, name) == 0 {
                return Some(a);
            }
        }
        av = doc[a].next;
    }
    None
}

/// DropAttrByName()
pub fn drop_attr_by_name(doc: &mut Doc, node: NodeId, name: &[u8]) {
    let mut prev: Option<AttId> = None;
    let mut attr = doc[node].attributes;
    while let Some(a) = attr {
        let next = doc[a].next;
        let hit = doc[a]
            .attribute
            .as_deref()
            .is_some_and(|n| tmbstrcmp(n, name) == 0);
        if hit {
            match prev {
                Some(p) => doc[p].next = next,
                None => doc[node].attributes = next,
            }
            lexer::free_attribute(doc, a);
            break;
        }
        prev = Some(a);
        attr = next;
    }
}

fn add_attribute_opt(doc: &mut Doc, node: NodeId, name: &[u8], value: Option<&[u8]>) -> AttId {
    let av = lexer::new_attribute(doc);
    doc[av].delim = '"' as u32;
    doc[av].attribute = Some(cstr(name).to_vec());
    doc[av].value = value.map(|v| cstr(v).to_vec());
    doc[av].dict = attrs_lookup(Some(name));
    lexer::insert_attribute_at_end(doc, node, av);
    av
}

/// AddAttribute(): add an attribute at the end of the node's list
pub fn add_attribute(doc: &mut Doc, node: NodeId, name: &[u8], value: &[u8]) -> AttId {
    add_attribute_opt(doc, node, name, Some(value))
}

/// RepairAttrValue(): set or add an attribute
pub fn repair_attr_value(doc: &mut Doc, node: NodeId, name: &[u8], value: Option<&[u8]>) -> AttId {
    if let Some(old) = get_attr_by_name(doc, node, name) {
        doc[old].value = value.map(|v| cstr(v).to_vec());
        old
    } else {
        add_attribute_opt(doc, node, name, value)
    }
}

/// FreeAttrPriorityList()
pub fn free_attr_priority_list(doc: &mut Doc) {
    doc.attribs.priority_attribs.clear();
}

/// DefinePriorityAttribute()
pub fn define_priority_attribute(doc: &mut Doc, name: &[u8]) {
    doc.attribs.priority_attribs.push(cstr(name).to_vec());
}

/// getPriorityAttrList() / getNextPriorityAttr()
pub fn get_priority_attrs(doc: &Doc) -> Vec<Vec<u8>> {
    doc.attribs.priority_attribs.clone()
}

/// IsUrl()
pub fn is_url(doc: &mut Doc, attrname: &[u8]) -> bool {
    let _ = doc;
    attrs_lookup(Some(attrname)).is_some_and(|a| URL_ATTRS.contains(&a.id))
}

/// IsScript()
pub fn is_script(doc: &mut Doc, attrname: &[u8]) -> bool {
    let _ = doc;
    attrs_lookup(Some(attrname)).is_some_and(|a| SCRIPT_ATTRS.contains(&a.id))
}

/// IsAnchorElement(): may id or name serve as anchor?
pub fn is_anchor_element(doc: &mut Doc, node: NodeId) -> bool {
    matches!(
        doc.tag_id(Some(node)),
        TagId::A
            | TagId::APPLET
            | TagId::FORM
            | TagId::FRAME
            | TagId::IFRAME
            | TagId::IMG
            | TagId::MAP
    )
}

/// IsCSS1Selector()
pub fn is_css1_selector(buf: &[u8]) -> bool {
    let mut valid = true;
    let mut esclen = 0;
    let buf = cstr(buf);
    let mut pos = 0usize;
    while valid && pos < buf.len() {
        let c = buf[pos];
        if c == b'\\' {
            esclen = 1;
        } else if c.is_ascii_digit() {
            if esclen > 0 {
                esclen += 1;
                valid = esclen < 6;
            }
            if valid {
                valid = pos > 0 || esclen > 0;
            }
        } else {
            valid = esclen > 0 || (pos > 0 && c == b'-') || c.is_ascii_alphabetic() || c >= 161;
            esclen = 0;
        }
        pos += 1;
    }
    valid
}

/// anchorNameHash()
fn anchor_name_hash(s: &[u8]) -> usize {
    let mut hashval: u32 = 0;
    for &b in cstr(s) {
        let c = lexer::to_lower(sch(b)) as u8;
        hashval = sch(c).wrapping_add(hashval.wrapping_mul(31));
    }
    (hashval % ANCHOR_HASH_SIZE as u32) as usize
}

/// anchorNameHash5(): HTML5 ids are case-sensitive
fn anchor_name_hash5(s: &[u8]) -> usize {
    let mut hashval: u32 = 0;
    for &b in cstr(s) {
        hashval = sch(b).wrapping_add(hashval.wrapping_mul(31));
    }
    (hashval % ANCHOR_HASH_SIZE as u32) as usize
}

fn ensure_buckets(doc: &mut Doc) {
    if doc.attribs.anchor_hash.len() != ANCHOR_HASH_SIZE {
        doc.attribs.anchor_hash = vec![Vec::new(); ANCHOR_HASH_SIZE];
    }
}

fn anchor_hash_for(doc: &mut Doc, name: &[u8]) -> usize {
    if lexer::html_version(doc) == HT50 {
        anchor_name_hash5(name)
    } else {
        anchor_name_hash(name)
    }
}

/// RemoveAnchorByNode(): removes anchor for specific node
pub fn remove_anchor_by_node(doc: &mut Doc, name: &[u8], node: NodeId) {
    let h = anchor_hash_for(doc, name);
    ensure_buckets(doc);
    let bucket = &mut doc.attribs.anchor_hash[h];
    if let Some(i) = bucket.iter().position(|a| a.node == node) {
        bucket.remove(i);
    }
}

/// AddAnchor(): add new anchor to namespace
fn add_anchor(doc: &mut Doc, name: &[u8], node: NodeId) {
    // NewAnchor()
    let mut aname = cstr(name).to_vec();
    if !tags::is_html5_mode(doc) {
        aname.make_ascii_lowercase();
    }
    let h = anchor_hash_for(doc, name);
    ensure_buckets(doc);
    doc.attribs.anchor_hash[h].push(Anchor { node, name: aname });
}

/// GetNodeByAnchor(): return node associated with anchor
fn get_node_by_anchor(doc: &mut Doc, name: &[u8]) -> Option<NodeId> {
    let mut lname = cstr(name).to_vec();
    let h = if lexer::html_version(doc) == HT50 {
        anchor_name_hash5(name)
    } else {
        lname.make_ascii_lowercase();
        anchor_name_hash(name)
    };
    ensure_buckets(doc);
    doc.attribs.anchor_hash[h]
        .iter()
        .find(|a| tmbstrcmp(&a.name, &lname) == 0)
        .map(|a| a.node)
}

/// FreeAnchors(): free all anchors
pub fn free_anchors(doc: &mut Doc) {
    for b in doc.attribs.anchor_hash.iter_mut() {
        b.clear();
    }
}

/// InitAttrs(): public method for inititializing attribute dictionary
pub fn init_attrs(doc: &mut Doc) {
    doc.attribs = AttribImpl::default();
    ensure_buckets(doc);
}

/// FreeAttrTable()
pub fn free_attr_table(doc: &mut Doc) {
    free_anchors(doc);
}

/// AppendToClassAttr()
pub fn append_to_class_attr(doc: &mut Doc, classattr: AttId, classname: &[u8]) {
    let mut s = Vec::new();
    if let Some(v) = doc[classattr].value.as_deref() {
        s.extend_from_slice(cstr(v));
        s.push(b' ');
    }
    s.extend_from_slice(cstr(classname));
    doc[classattr].value = Some(s);
}

/// AppendToStyleAttr(): concatenate styles
fn append_to_style_attr(doc: &mut Doc, styleattr: AttId, styleprop: &[u8]) {
    let mut v = cstr(doc[styleattr].value.as_deref().unwrap_or(b"")).to_vec();
    let styleprop = cstr(styleprop);
    let end = v.len();
    if end > 0 && v[end - 1] == b';' {
        v.push(b' ');
        v.extend_from_slice(styleprop);
    } else if end > 0 && v[end - 1] == b'}' {
        v.extend_from_slice(b" { ");
        v.extend_from_slice(styleprop);
        v.extend_from_slice(b" }");
    } else {
        if end > 0 {
            v.extend_from_slice(b"; ");
        }
        v.extend_from_slice(styleprop);
    }
    doc[styleattr].value = Some(v);
}

/// AttrsHaveSameName(): the same attribute name can't be used more than
/// once in each element
fn attrs_have_same_name(doc: &Doc, av1: AttId, av2: AttId) -> bool {
    let id1 = doc.attr_id(av1);
    let id2 = doc.attr_id(av2);
    if id1 != AttrId::UNKNOWN && id2 != AttrId::UNKNOWN {
        // AttrsHaveSameId()
        return id1 == id2;
    }
    if id1 != AttrId::UNKNOWN || id2 != AttrId::UNKNOWN {
        return false;
    }
    match (doc[av1].attribute.as_deref(), doc[av2].attribute.as_deref()) {
        (Some(a), Some(b)) => tmbstrcmp(a, b) == 0,
        _ => false,
    }
}

/// RepairDuplicateAttributes()
pub fn repair_duplicate_attributes(doc: &mut Doc, node: NodeId, is_xml: bool) {
    let mut first = doc[node].attributes;
    while let Some(f) = first {
        let mut first_redefined = false;
        if !(doc[f].asp.is_none() && doc[f].php.is_none()) {
            first = doc[f].next;
            continue;
        }
        let mut cur_first = f;
        let mut second = doc[cur_first].next;
        while let Some(s) = second {
            if !(doc[s].asp.is_none()
                && doc[s].php.is_none()
                && attrs_have_same_name(doc, cur_first, s))
            {
                second = doc[s].next;
                continue;
            }
            let fv = doc.attr_has_value(Some(cur_first));
            let sv = doc.attr_has_value(Some(s));
            if !is_xml
                && doc.attr_is(Some(cur_first), AttrId::CLASS)
                && doc.cfg_bool(OptId::JoinClasses)
                && fv
                && sv
            {
                let v = value_of(doc, s);
                append_to_class_attr(doc, cur_first, &v);
                let temp = doc[s].next;
                message::report_attr_error(doc, node, Some(s), JOINING_ATTRIBUTE);
                lexer::remove_attribute(doc, node, s);
                second = temp;
            } else if !is_xml
                && doc.attr_is(Some(cur_first), AttrId::STYLE)
                && doc.cfg_bool(OptId::JoinStyles)
                && fv
                && sv
            {
                let v = value_of(doc, s);
                append_to_style_attr(doc, cur_first, &v);
                let temp = doc[s].next;
                message::report_attr_error(doc, node, Some(s), JOINING_ATTRIBUTE);
                lexer::remove_attribute(doc, node, s);
                second = temp;
            } else if doc.cfg(OptId::DuplicateAttrs) == TIDY_KEEP_LAST {
                let temp = doc[cur_first].next;
                message::report_attr_error(doc, node, Some(cur_first), REPEATED_ATTRIBUTE);
                lexer::remove_attribute(doc, node, cur_first);
                first_redefined = true;
                // `first = temp`: C keeps comparing against the new first
                // (temp is non-NULL since second follows it)
                first = temp;
                if let Some(t) = temp {
                    cur_first = t;
                }
                second = doc[s].next;
            } else {
                let temp = doc[s].next;
                message::report_attr_error(doc, node, Some(s), REPEATED_ATTRIBUTE);
                lexer::remove_attribute(doc, node, s);
                second = temp;
            }
        }
        if !first_redefined {
            first = doc[cur_first].next;
        }
    }
}

/// CheckAttribute(): ignore unknown attributes for proprietary elements
pub fn check_attribute(doc: &mut Doc, node: NodeId, attval: AttId) -> Option<&'static Attribute> {
    let attribute = doc[attval].dict;
    if let Some(attr) = attribute {
        if doc.attr_is(Some(attval), AttrId::XML_LANG)
            || doc.attr_is(Some(attval), AttrId::XML_SPACE)
        {
            doc.lx().isvoyager = true;
            if !doc.cfg_bool(OptId::HtmlOut) {
                config::set_option_bool(doc, OptId::XhtmlOut, true);
                config::set_option_bool(doc, OptId::XmlOut, true);
            }
        }
        let v = attribute_versions(doc, Some(node), Some(attval));
        lexer::constrain_version(doc, v);
        if let Some(chk) = attr.attrchk {
            chk(doc, node, attval);
        }
    }
    attribute
}

/// IsBoolAttribute()
pub fn is_bool_attribute(doc: &Doc, attval: AttId) -> bool {
    match doc[attval].dict {
        Some(a) => a.attrchk.is_some() && BOOL_ATTRS.contains(&a.id),
        None => false,
    }
}

/// attrIsEvent()
pub fn attr_is_event(doc: &Doc, attval: AttId) -> bool {
    let atid = doc.attr_id(attval);
    SCRIPT_ATTRS.contains(&atid)
}

/// CheckLowerCaseAttrValue()
fn check_lower_case_attr_value(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        return;
    }
    let has_upper = cstr(doc[attval].value.as_deref().unwrap_or(b""))
        .iter()
        .any(|&c| lexer::is_upper(sch(c)));
    if has_upper {
        let isvoyager = doc.lxr().isvoyager;
        if isvoyager {
            message::report_attr_error(doc, node, Some(attval), ATTR_VALUE_NOT_LCASE);
        }
        if isvoyager || doc.cfg_bool(OptId::LowerLiterals) {
            if let Some(v) = doc[attval].value.as_mut() {
                v.make_ascii_lowercase();
            }
        }
    }
}

fn isalnum(c: u32) -> bool {
    (b'A' as u32..=b'Z' as u32).contains(&c)
        || (b'a' as u32..=b'z' as u32).contains(&c)
        || (b'0' as u32..=b'9' as u32).contains(&c)
}

/// IsURLCodePoint(): returns (valid, increment)
fn is_url_code_point(p: &[u8]) -> (bool, usize) {
    let (n, c) = get_utf8(p);
    let inc = n + 1;
    let ok = isalnum(c)
        || b"%#!$&'()*+,-./:;=?@_~".iter().any(|&x| x as u32 == c)
        || (0x00A0..=0xD7FF).contains(&c)
        || (0xE000..=0xFDCF).contains(&c)
        || (0xFDF0..=0xFFEF).contains(&c)
        || (0x10000..=0x1FFFD).contains(&c)
        || (0x20000..=0x2FFFD).contains(&c)
        || (0x30000..=0x3FFFD).contains(&c)
        || (0x40000..=0x4FFFD).contains(&c)
        || (0x50000..=0x5FFFD).contains(&c)
        || (0x60000..=0x6FFFD).contains(&c)
        || (0x70000..=0x7FFFD).contains(&c)
        || (0x80000..=0x8FFFD).contains(&c)
        || (0x90000..=0x9FFFD).contains(&c)
        || (0xA0000..=0xAFFFD).contains(&c)
        || (0xB0000..=0xBFFFD).contains(&c)
        || (0xC0000..=0xCFFFD).contains(&c)
        || (0xD0000..=0xDFFFD).contains(&c)
        || (0xE0000..=0xEFFFD).contains(&c)
        || (0xF0000..=0xFFFFD).contains(&c)
        || (0x100000..=0x10FFFD).contains(&c);
    (ok, inc)
}

/// `(c > 0x7e) || (c <= 0x20) || strchr("<>", c)` with a signed char
fn url_needs_escape(c: u8) -> bool {
    let sc = c as i8 as i32;
    sc > 0x7e || sc <= 0x20 || c == b'<' || c == b'>'
}

/// CheckUrl()
pub fn check_url(doc: &mut Doc, node: NodeId, attval: AttId) {
    let mut escape_count = 0u32;
    let mut backslash_count = 0u32;
    let mut bad_codepoint_count = 0u32;

    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let fix_backslash = doc.cfg_bool(OptId::FixBackslash);
    let mut p = cstr(doc[attval].value.as_deref().unwrap_or(b"")).to_vec();
    let is_javascript = tmbstrncmp(&p, b"javascript:", 11) == 0;

    for c in p.iter_mut() {
        if *c == b'\\' {
            backslash_count += 1;
            if fix_backslash && !is_javascript {
                *c = b'/';
            }
        } else if url_needs_escape(*c) {
            escape_count += 1;
        }
    }

    let mut i = 0;
    while i < p.len() {
        let (ok, inc) = is_url_code_point(&p[i..]);
        if !ok {
            bad_codepoint_count += 1;
        }
        i += inc;
    }

    if doc.cfg_bool(OptId::FixUri) && escape_count > 0 {
        let mut hadnonspace = false;
        let mut dest = Vec::with_capacity(p.len() + escape_count as usize * 2);
        for &c in &p {
            if url_needs_escape(c) {
                if c == 0x20 {
                    if hadnonspace {
                        dest.extend_from_slice(format!("%{:02X}", c).as_bytes());
                    }
                } else {
                    dest.extend_from_slice(format!("%{:02X}", c).as_bytes());
                    hadnonspace = true;
                }
            } else {
                hadnonspace = true;
                dest.push(c);
            }
        }
        p = dest;
    }
    doc[attval].value = Some(p);

    if backslash_count > 0 {
        if fix_backslash && !is_javascript {
            message::report_attr_error(doc, node, Some(attval), FIXED_BACKSLASH);
        } else {
            message::report_attr_error(doc, node, Some(attval), BACKSLASH_IN_URI);
        }
    }
    if escape_count > 0 {
        if doc.cfg_bool(OptId::FixUri) {
            message::report_attr_error(doc, node, Some(attval), ESCAPED_ILLEGAL_URI);
        } else if lexer::html_version(doc) & VERS_HTML5 == 0 {
            message::report_attr_error(doc, node, Some(attval), ILLEGAL_URI_REFERENCE);
        }
        doc.bad_chars |= BC_INVALID_URI;
    }
    if bad_codepoint_count > 0 {
        message::report_attr_error(doc, node, Some(attval), ILLEGAL_URI_CODEPOINT);
    }
}

/// CheckAction()
pub fn check_action(doc: &mut Doc, node: NodeId, attval: AttId) {
    if doc.attr_has_value(Some(attval)) {
        check_url(doc, node, attval);
    }
}

/// CheckScript()
pub fn check_script(doc: &mut Doc, node: NodeId, attval: AttId) {
    let _ = (doc, node, attval);
}

/// IsValidHTMLID()
pub fn is_valid_html_id(id: &[u8]) -> bool {
    !cstr(id).iter().any(|&c| lexer::is_html_space(sch(c)))
}

/// IsValidXMLID()
pub fn is_valid_xml_id(id: &[u8]) -> bool {
    let s = cstr(id);
    let mut pos = 0usize;
    let mut c = sch(at(s, pos));
    pos += 1;
    if c > 0x7F {
        // (sic) decodes from the byte after the first one
        let (n, ch) = get_utf8(s.get(pos..).unwrap_or(&[]));
        pos += n;
        c = ch;
    }
    if !(lexer::is_xml_letter(c) || c == '_' as u32 || c == ':' as u32) {
        return false;
    }
    while pos < s.len() {
        let mut c = s[pos] as u32;
        if c > 0x7F {
            let (n, ch) = get_utf8(&s[pos..]);
            pos += n;
            c = ch;
        }
        pos += 1;
        if !lexer::is_xml_namechar(c) {
            return false;
        }
    }
    true
}

/// IsValidNMTOKEN()
fn is_valid_nmtoken(name: &[u8]) -> bool {
    let s = cstr(name);
    let mut pos = 0usize;
    while pos < s.len() {
        let mut c = s[pos] as u32;
        if c > 0x7F {
            let (n, ch) = get_utf8(&s[pos..]);
            pos += n;
            c = ch;
        }
        pos += 1;
        if !lexer::is_xml_namechar(c) {
            return false;
        }
    }
    true
}

/// AttrValueIsAmong()
fn attr_value_is_among(doc: &Doc, attval: AttId, list: &[&[u8]]) -> bool {
    list.iter().any(|v| doc.attr_value_is(Some(attval), v))
}

/// CheckAttrValidity()
fn check_attr_validity(doc: &mut Doc, node: NodeId, attval: AttId, list: &[&[u8]]) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    check_lower_case_attr_value(doc, node, attval);
    if !attr_value_is_among(doc, attval, list) {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckName()
pub fn check_name(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    if is_anchor_element(doc, node) {
        let v = value_of(doc, attval);
        if doc.cfg_bool(OptId::XmlOut) && !is_valid_nmtoken(&v) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
        }
        match get_node_by_anchor(doc, &v) {
            Some(old) if old != node => {
                if doc[node].implicit {
                    message::report_attr_error(doc, node, Some(attval), ANCHOR_DUPLICATED);
                } else {
                    message::report_attr_error(doc, node, Some(attval), ANCHOR_NOT_UNIQUE);
                }
            }
            _ => add_anchor(doc, &v, node),
        }
    }
}

/// CheckId()
pub fn check_id(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let v = value_of(doc, attval);
    if !is_valid_html_id(&v) {
        if doc.lxr().isvoyager && is_valid_xml_id(&v) {
            message::report_attr_error(doc, node, Some(attval), XML_ID_SYNTAX);
        } else {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
        }
    }
    match get_node_by_anchor(doc, &v) {
        Some(old) if old != node => {
            if doc[node].implicit {
                message::report_attr_error(doc, node, Some(attval), ANCHOR_DUPLICATED);
            } else {
                message::report_attr_error(doc, node, Some(attval), ANCHOR_NOT_UNIQUE);
            }
        }
        _ => add_anchor(doc, &v, node),
    }
}

/// `strchr(s, ch)` position
fn strchr(s: &[u8], ch: u8) -> Option<usize> {
    cstr(s).iter().position(|&c| c == ch)
}

/// CheckIs()
pub fn check_is(doc: &mut Doc, node: NodeId, attval: AttId) {
    let el = doc[node].element.clone().unwrap_or_default();
    if matches!(strchr(&el, b'-'), Some(p) if p > 0) {
        message::report_attr_error(doc, node, Some(attval), ATTRIBUTE_IS_NOT_ALLOWED);
    }
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let v = value_of(doc, attval);
    let mut go = matches!(strchr(&v, b'-'), Some(p) if p > 0);
    go &= strchr(&v, b' ').is_none();
    if !go {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckBool()
pub fn check_bool(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        return;
    }
    check_lower_case_attr_value(doc, node, attval);
}

/// CheckAlign()
pub fn check_align(doc: &mut Doc, node: NodeId, attval: AttId) {
    const VALUES: &[&[u8]] = &[b"left", b"right", b"center", b"justify"];
    if doc[node].tag.is_some() && (doc.model(node) & CM_IMG) != 0 {
        check_valign(doc, node, attval);
        return;
    }
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    check_lower_case_attr_value(doc, node, attval);
    if doc.is(node, TagId::CAPTION) {
        return;
    }
    if !attr_value_is_among(doc, attval, VALUES)
        && !(doc.attr_value_is(Some(attval), b"char")
            && tags::node_has_cm(doc, Some(node), CM_TABLE | CM_ROW))
    {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckValign()
pub fn check_valign(doc: &mut Doc, node: NodeId, attval: AttId) {
    const VALUES: &[&[u8]] = &[b"top", b"middle", b"bottom", b"baseline"];
    const VALUES2: &[&[u8]] = &[b"left", b"right"];
    const VALUESP: &[&[u8]] = &[b"texttop", b"absmiddle", b"absbottom", b"textbottom"];
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    check_lower_case_attr_value(doc, node, attval);
    if attr_value_is_among(doc, attval, VALUES) {
        // all is fine
    } else if attr_value_is_among(doc, attval, VALUES2) {
        if !(doc[node].tag.is_some() && (doc.model(node) & CM_IMG) != 0) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
        }
    } else if attr_value_is_among(doc, attval, VALUESP) {
        lexer::constrain_version(doc, VERS_PROPRIETARY);
        message::report_attr_error(doc, node, Some(attval), PROPRIETARY_ATTR_VALUE);
    } else {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckLength()
pub fn check_length(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    if doc.attr_is(Some(attval), AttrId::WIDTH)
        && (doc.is(node, TagId::COL) || doc.is(node, TagId::COLGROUP))
    {
        return;
    }
    let p = value_of(doc, attval);
    let p = cstr(&p);
    if !lexer::is_digit(sch(at(p, 0))) {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    } else {
        let mut percent_found = false;
        for &c in &p[1..] {
            if !percent_found && c == b'%' {
                percent_found = true;
            } else if percent_found || !lexer::is_digit(sch(c)) {
                message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
                break;
            }
        }
    }
}

/// CheckTarget()
pub fn check_target(doc: &mut Doc, node: NodeId, attval: AttId) {
    const VALUES: &[&[u8]] = &[b"_blank", b"_self", b"_parent", b"_top"];
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let first = at(doc[attval].value.as_deref().unwrap_or(b""), 0);
    if lexer::is_letter(sch(first)) {
        return;
    }
    if !attr_value_is_among(doc, attval, VALUES) {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckFsubmit()
pub fn check_fsubmit(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(doc, node, attval, &[b"get", b"post"]);
}

/// CheckClear()
pub fn check_clear(doc: &mut Doc, node: NodeId, attval: AttId) {
    const VALUES: &[&[u8]] = &[b"none", b"left", b"right", b"all"];
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        if doc[attval].value.is_none() {
            doc[attval].value = Some(b"none".to_vec());
        }
        return;
    }
    check_lower_case_attr_value(doc, node, attval);
    if !attr_value_is_among(doc, attval, VALUES) {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckShape()
pub fn check_shape(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(
        doc,
        node,
        attval,
        &[b"rect", b"default", b"circle", b"poly"],
    );
}

/// CheckScope()
pub fn check_scope(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(
        doc,
        node,
        attval,
        &[b"row", b"rowgroup", b"col", b"colgroup"],
    );
}

/// CheckNumber()
pub fn check_number(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    if doc.is(node, TagId::FRAMESET)
        && (doc.attr_is(Some(attval), AttrId::COLS) || doc.attr_is(Some(attval), AttrId::ROWS))
    {
        return;
    }
    let v = value_of(doc, attval);
    let v = cstr(&v);
    let mut p = 0usize;
    if doc.is(node, TagId::FONT) && (at(v, p) == b'+' || at(v, p) == b'-') {
        p += 1;
    }
    let is_tabindex = doc[attval]
        .attribute
        .as_deref()
        .is_some_and(|a| cstr(a) == b"tabindex");
    if is_tabindex && at(v, p) == b'-' {
        p += 1;
    }
    while p < v.len() {
        if !lexer::is_digit(sch(v[p])) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
            break;
        }
        p += 1;
    }
}

/// IsValidColorCode(): check hexadecimal color value
fn is_valid_color_code(color: &[u8]) -> bool {
    let color = cstr(color);
    if color.len() != 6 {
        return false;
    }
    for &c in color {
        let l = lexer::to_lower(sch(c));
        if !lexer::is_digit(sch(c)) && !(b'a' as u32..=b'f' as u32).contains(&l) {
            return false;
        }
    }
    true
}

/// CheckColor(): check color syntax and beautify value by option
pub fn check_color(doc: &mut Doc, node: NodeId, attval: AttId) {
    let mut valid = false;
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let mut given = cstr(&value_of(doc, attval)).to_vec();

    if at(&given, 0) != b'#' {
        valid = is_valid_color_code(&given);
        if valid {
            let mut s = vec![b'#'];
            s.extend_from_slice(&given);
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE_REPLACED);
            doc[attval].value = Some(s.clone());
            given = s;
        }
    }
    if !valid && at(&given, 0) == b'#' {
        valid = is_valid_color_code(&given[1..]);
    }
    if valid && at(&given, 0) == b'#' && doc.cfg_bool(OptId::ReplaceColor) {
        if let Some(new_name) = get_color_name(&given, tags::is_html5_mode(doc)) {
            given = new_name.to_vec();
            doc[attval].value = Some(given.clone());
        }
    }
    if !valid {
        valid = get_color_code(&given, tags::is_html5_mode(doc)).is_some();
    }
    if valid && at(&given, 0) == b'#' {
        if let Some(v) = doc[attval].value.as_mut() {
            v.make_ascii_uppercase();
        }
    } else if valid {
        if let Some(v) = doc[attval].value.as_mut() {
            v.make_ascii_lowercase();
        }
    }
    if !valid {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckVType(): check valuetype attribute for element param
pub fn check_v_type(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(doc, node, attval, &[b"data", b"object", b"ref"]);
}

/// CheckScroll(): checks scrolling attribute
pub fn check_scroll(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(doc, node, attval, &[b"no", b"auto", b"yes"]);
}

/// CheckTextDir(): checks dir attribute
pub fn check_text_dir(doc: &mut Doc, node: NodeId, attval: AttId) {
    if tags::is_html5_mode(doc) {
        check_attr_validity(doc, node, attval, &[b"rtl", b"ltr", b"auto"]);
    } else {
        check_attr_validity(doc, node, attval, &[b"rtl", b"ltr"]);
    }
}

/// CheckLang(): checks lang and xml:lang attributes
pub fn check_lang(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval))
        && !doc.attr_is(Some(attval), AttrId::XML_LANG)
        && doc.cfg(OptId::AccessibilityCheckLevel) == 0
    {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
    }
}

/// CheckLoading(): checks loading attribute
pub fn check_loading(doc: &mut Doc, node: NodeId, attval: AttId) {
    check_attr_validity(doc, node, attval, &[b"lazy", b"eager"]);
}

/// CheckType(): checks type attribute
pub fn check_type(doc: &mut Doc, node: NodeId, attval: AttId) {
    const VALUES_INPUT: &[&[u8]] = &[
        b"text",
        b"password",
        b"checkbox",
        b"radio",
        b"submit",
        b"reset",
        b"file",
        b"hidden",
        b"image",
        b"button",
        b"color",
        b"date",
        b"datetime",
        b"datetime-local",
        b"email",
        b"month",
        b"number",
        b"range",
        b"search",
        b"tel",
        b"time",
        b"url",
        b"week",
    ];
    const VALUES_BUTTON: &[&[u8]] = &[b"button", b"submit", b"reset"];
    const VALUES_UL: &[&[u8]] = &[b"disc", b"square", b"circle"];
    const VALUES_OL: &[&[u8]] = &[b"1", b"a", b"i"];

    if doc.is(node, TagId::INPUT) {
        check_attr_validity(doc, node, attval, VALUES_INPUT);
    } else if doc.is(node, TagId::BUTTON) {
        check_attr_validity(doc, node, attval, VALUES_BUTTON);
    } else if doc.is(node, TagId::UL) {
        check_attr_validity(doc, node, attval, VALUES_UL);
    } else if doc.is(node, TagId::OL) {
        if !doc.attr_has_value(Some(attval)) {
            message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
            return;
        }
        if !attr_value_is_among(doc, attval, VALUES_OL) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
        }
    } else if doc.is(node, TagId::LI) {
        if !doc.attr_has_value(Some(attval)) {
            message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
            return;
        }
        if attr_value_is_among(doc, attval, VALUES_UL) {
            check_lower_case_attr_value(doc, node, attval);
        } else if !attr_value_is_among(doc, attval, VALUES_OL) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
        }
    }
}

/// CheckDecimal()
pub fn check_decimal(doc: &mut Doc, node: NodeId, attval: AttId) {
    let v = value_of(doc, attval);
    let v = cstr(&v);
    let mut p = 0usize;
    if at(v, p) == b'+' || at(v, p) == b'-' {
        p += 1;
    }
    while p < v.len() {
        if v[p] == b'.' {
            // (sic) tidy's `break` is outside the `else` reporting a second
            // point, so scanning always stops at the first point.
            break;
        }
        if !lexer::is_digit(sch(v[p])) {
            message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
            break;
        }
        p += 1;
    }
}

/// IsSvgPaintAttr()
fn is_svg_paint_attr(doc: &Doc, attval: AttId) -> bool {
    matches!(
        doc.attr_id(attval),
        AttrId::COLOR
            | AttrId::FILL
            | AttrId::FILLRULE
            | AttrId::STROKE
            | AttrId::STROKEDASHARRAY
            | AttrId::STROKEDASHOFFSET
            | AttrId::STROKELINECAP
            | AttrId::STROKELINEJOIN
            | AttrId::STROKEMITERLIMIT
            | AttrId::STROKEWIDTH
            | AttrId::COLORINTERPOLATION
            | AttrId::COLORRENDERING
            | AttrId::OPACITY
            | AttrId::STROKEOPACITY
            | AttrId::FILLOPACITY
    )
}

fn among_lc_or_bad(doc: &mut Doc, node: NodeId, attval: AttId, values: &[&[u8]]) {
    if attr_value_is_among(doc, attval, values) {
        check_lower_case_attr_value(doc, node, attval);
    } else {
        message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckSvgAttr(): check SVG attributes
pub fn check_svg_attr(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.is(node, TagId::SVG) {
        message::report_attr_error(doc, node, Some(attval), ATTRIBUTE_IS_NOT_ALLOWED);
        return;
    }
    if !is_svg_paint_attr(doc, attval) {
        return;
    }
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    if doc.attr_value_is(Some(attval), b"inherit") {
        return;
    }
    match doc.attr_id(attval) {
        AttrId::FILL | AttrId::STROKE => {
            if attr_value_is_among(doc, attval, &[b"none", b"currentColor"]) {
                check_lower_case_attr_value(doc, node, attval);
            } else {
                check_color(doc, node, attval);
            }
        }
        AttrId::FILLRULE => among_lc_or_bad(doc, node, attval, &[b"nonzero", b"evenodd"]),
        AttrId::STROKEDASHARRAY => {
            if attr_value_is_among(doc, attval, &[b"none"]) {
                check_lower_case_attr_value(doc, node, attval);
            }
        }
        AttrId::STROKEDASHOFFSET => check_length(doc, node, attval),
        AttrId::STROKELINECAP => {
            among_lc_or_bad(doc, node, attval, &[b"butt", b"round", b"square"])
        }
        AttrId::STROKELINEJOIN => {
            among_lc_or_bad(doc, node, attval, &[b"miter", b"round", b"bevel"])
        }
        AttrId::STROKEMITERLIMIT => check_number(doc, node, attval),
        AttrId::STROKEWIDTH => check_length(doc, node, attval),
        AttrId::COLORINTERPOLATION => {
            among_lc_or_bad(doc, node, attval, &[b"auto", b"sRGB", b"linearRGB"])
        }
        AttrId::COLORRENDERING => among_lc_or_bad(
            doc,
            node,
            attval,
            &[b"auto", b"optimizeSpeed", b"optimizeQuality"],
        ),
        AttrId::OPACITY | AttrId::STROKEOPACITY | AttrId::FILLOPACITY => {
            check_decimal(doc, node, attval)
        }
        _ => {}
    }
}

/// SortAttributes()
pub fn sort_attributes(doc: &mut Doc, node: Option<NodeId>, strat: u32) {
    let mut node = node;
    while let Some(n) = node {
        let attrs = doc[n].attributes;
        doc[n].attributes = sort_att_val(doc, attrs, strat);
        if let Some(c) = doc[n].content {
            sort_attributes(doc, Some(c), strat);
        }
        node = doc[n].next;
    }
}

/// indexof(): index of the item in the list, or -1
fn indexof(item: &[u8], list: &[Vec<u8>]) -> i32 {
    for (i, l) in list.iter().enumerate() {
        if tmbstrcasecmp(item, l) == 0 {
            return i as i32;
        }
    }
    -1
}

#[derive(Clone, Copy)]
enum Comparator {
    Alpha,
    Priority,
}

fn compare(doc: &Doc, cmp: Comparator, one: AttId, two: AttId, list: &[Vec<u8>]) -> i32 {
    let a = doc[one].attribute.as_deref().unwrap_or(b"");
    let b = doc[two].attribute.as_deref().unwrap_or(b"");
    let one_index = indexof(a, list);
    let two_index = indexof(b, list);
    if one_index >= 0 && two_index >= 0 {
        return if one_index < two_index { -1 } else { 1 };
    }
    if one_index >= 0 && two_index == -1 {
        return -1;
    }
    if one_index == -1 && two_index >= 0 {
        return 1;
    }
    match cmp {
        Comparator::Alpha => tmbstrcmp(a, b),
        Comparator::Priority => 0,
    }
}

/// SortAttVal(): Simon Tatham's linked-list merge sort
fn sort_att_val(doc: &mut Doc, list: Option<AttId>, strat: u32) -> Option<AttId> {
    let priority_list = doc.attribs.priority_attribs.clone();
    let cmp = match strat {
        TIDY_SORT_ATTR_ALPHA => Comparator::Alpha,
        TIDY_SORT_ATTR_NONE if !priority_list.is_empty() => Comparator::Priority,
        _ => return list,
    };
    let mut list = list?;
    let mut insize = 1;
    loop {
        let mut p = Some(list);
        let mut head: Option<AttId> = None;
        let mut tail: Option<AttId> = None;
        let mut nmerges = 0;
        while p.is_some() {
            nmerges += 1;
            let mut q = p;
            let mut psize = 0;
            for _ in 0..insize {
                psize += 1;
                q = q.and_then(|x| doc[x].next);
                if q.is_none() {
                    break;
                }
            }
            let mut qsize = insize;
            while psize > 0 || (qsize > 0 && q.is_some()) {
                let e;
                if psize == 0 {
                    e = q.unwrap();
                    q = doc[e].next;
                    qsize -= 1;
                } else if qsize == 0
                    || q.is_none()
                    || compare(doc, cmp, p.unwrap(), q.unwrap(), &priority_list) <= 0
                {
                    // q is empty, or the first element of p is lower (or
                    // the same): e must come from p
                    e = p.unwrap();
                    p = doc[e].next;
                    psize -= 1;
                } else {
                    e = q.unwrap();
                    q = doc[e].next;
                    qsize -= 1;
                }
                match tail {
                    Some(t) => doc[t].next = Some(e),
                    None => head = Some(e),
                }
                tail = Some(e);
            }
            p = q;
        }
        if let Some(t) = tail {
            doc[t].next = None;
        }
        list = head.expect("non-empty list");
        if nmerges <= 1 {
            return Some(list);
        }
        insize *= 2;
    }
}

/// CheckRDFaPrefix(): ensure the prefix attribute value is correct
pub fn check_rd_fa_prefix(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
        return;
    }
    let v = value_of(doc, attval);
    let mut is_prefix = true;
    // strtok(s, " ")
    for t in cstr(&v).split(|&c| c == b' ').filter(|t| !t.is_empty()) {
        if is_prefix {
            match t.iter().position(|&c| c == b':') {
                None => message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE),
                Some(i) if i != t.len() - 1 => {
                    message::report_attr_error(doc, node, Some(attval), BAD_ATTRIBUTE_VALUE)
                }
                _ => {}
            }
        }
        is_prefix = !is_prefix;
    }
}

/// CheckRDFaTerm(): are terms valid
pub fn check_rd_fa_term(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
    }
}

/// CheckRDFaSafeCURIE(): is a CURIE legal
pub fn check_rd_fa_safe_curie(doc: &mut Doc, node: NodeId, attval: AttId) {
    if !doc.attr_has_value(Some(attval)) {
        message::report_attr_error(doc, node, Some(attval), MISSING_ATTR_VALUE);
    }
}
