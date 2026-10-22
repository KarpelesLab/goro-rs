//! Stylesheet compilation: a port of libxslt's xslt.c (top-level parsing,
//! preprocessing), preproc.c (instruction precomputation), imports.c,
//! the parsing halves of keys.c, attributes.c, namespaces.c and attrvt.c.

use crate::pattern::{self, CompMatch, POp};
use crate::util::*;
use crate::xpath::{self, CompExpr};
use goro_core::vm::Vm;
use goro_ext_xml::tree::{NType, NodeId, Tree, XML_XML_NAMESPACE};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub const XSLT_PAT_NO_PRIORITY: f32 = -12345789.0;
pub const XSLT_MAX_NESTING: usize = 40;
pub const XSLT_PARSE_OPTIONS: i64 = goro_ext_xml::parser::XML_PARSE_NOENT
    | goro_ext_xml::parser::XML_PARSE_DTDLOAD
    | goro_ext_xml::parser::XML_PARSE_DTDATTR
    | goro_ext_xml::parser::XML_PARSE_NOCDATA;

pub const EXSLT_COMMON_NAMESPACE: &str = "http://exslt.org/common";
pub const EXSLT_FUNCTIONS_NAMESPACE: &str = "http://exslt.org/functions";
pub const XSLT_LIBXSLT_NAMESPACE: &str = "http://xmlsoft.org/XSLT/namespace";
pub const XSLT_SAXON_NAMESPACE: &str = "http://icl.com/saxon";
pub const XSLT_XT_NAMESPACE: &str = "http://www.jclark.com/xt";
pub const XSLT_XALAN_NAMESPACE: &str = "org.apache.xalan.xslt.extensions.Redirect";

/// Instruction kinds (xsltStyleType)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Copy,
    Sort,
    Text,
    Element,
    Attribute,
    Comment,
    Pi,
    CopyOf,
    ValueOf,
    Number,
    ApplyImports,
    CallTemplate,
    ApplyTemplates,
    Choose,
    If,
    ForEach,
    Document,
    WithParam,
    Param,
    Variable,
    When,
    /// an extension element (EXSLT/libxslt): its handler by name
    Extension,
}

/// xsltNumberData
#[derive(Clone, Debug, Default)]
pub struct NumberData {
    pub level: Option<String>,
    pub count: Option<String>,
    pub from: Option<String>,
    pub value: Option<String>,
    pub format: Option<String>,
    pub has_format: bool,
    pub digits_per_group: i32,
    pub grouping_character: u32,
    pub grouping_character_len: usize,
    pub count_pat: Option<Vec<usize>>,
    pub from_pat: Option<Vec<usize>>,
}

/// xsltStylePreComp
#[derive(Debug, Default)]
pub struct PreComp {
    pub kind: Kind,
    pub inst: NodeId,
    pub stype: Option<String>,
    pub has_stype: bool,
    pub number: bool,
    pub order: Option<String>,
    pub has_order: bool,
    pub descending: bool,
    pub lang: Option<String>,
    pub has_lang: bool,
    pub case_order: Option<String>,
    pub lower_first: bool,
    pub use_: Option<String>,
    pub has_use: bool,
    pub noescape: bool,
    pub name: Option<String>,
    pub has_name: bool,
    pub ns: Option<String>,
    pub has_ns: bool,
    pub mode: Option<String>,
    pub mode_uri: Option<String>,
    pub test: Option<String>,
    pub select: Option<String>,
    pub ver11: bool,
    pub filename: Option<String>,
    pub has_filename: bool,
    pub numdata: NumberData,
    pub comp: Option<Rc<CompExpr>>,
    pub ns_list: Rc<Vec<(String, String)>>,
    /// extension element: (namespace URI, local name)
    pub ext: Option<(String, String)>,
}

/// An xsl:template
#[derive(Debug)]
pub struct Template {
    pub style: usize,
    pub match_: Option<String>,
    pub priority: f32,
    pub name: Option<String>,
    pub name_uri: Option<String>,
    pub mode: Option<String>,
    pub mode_uri: Option<String>,
    /// first node of the template body
    pub content: Option<NodeId>,
    /// the xsl:template element (or the document of a simplified stylesheet)
    pub elem: NodeId,
    /// inherited namespaces (prefix, href)
    pub inherited_ns: Vec<(Option<String>, String)>,
    pub position: i32,
}

/// A global variable or parameter declaration (xsltStackElem in
/// style->variables)
#[derive(Debug, Clone)]
pub struct GlobalDef {
    pub name: String,
    pub name_uri: Option<String>,
    pub select: Option<String>,
    pub tree: Option<NodeId>,
    pub comp: Rc<PreComp>,
}

#[derive(Debug, Clone)]
pub struct KeyDef {
    pub name: String,
    pub name_uri: Option<String>,
    pub match_: String,
    pub use_: String,
    pub inst: NodeId,
    pub ns_list: Rc<Vec<(String, String)>>,
    pub comp: Option<Rc<CompExpr>>,
    pub usecomp: Option<Rc<CompExpr>>,
}

#[derive(Debug, Clone)]
pub struct DecimalFormat {
    pub name: Option<String>,
    pub ns_uri: Option<String>,
    pub digit: String,
    pub pattern_separator: String,
    pub minus_sign: String,
    pub infinity: String,
    pub no_number: String,
    pub decimal_point: String,
    pub grouping: String,
    pub percent: String,
    pub permille: String,
    pub zero_digit: String,
}

impl DecimalFormat {
    pub fn new(ns_uri: Option<String>, name: Option<String>) -> Self {
        DecimalFormat {
            name,
            ns_uri,
            digit: "#".into(),
            pattern_separator: ";".into(),
            minus_sign: "-".into(),
            infinity: "Infinity".into(),
            no_number: "NaN".into(),
            decimal_point: ".".into(),
            grouping: ",".into(),
            percent: "%".into(),
            permille: "\u{2030}".into(),
            zero_digit: "0".into(),
        }
    }
}

/// An attribute set being resolved (ATTRSET_RESOLVING), as nested
/// lookups see it.
fn placeholder(set: &AttrSet) -> AttrSet {
    AttrSet {
        state: 1,
        attrs: set.attrs.clone(),
        use_attr_sets: Vec::new(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct AttrSet {
    /// 0 unresolved, 1 resolving, 2 resolved
    pub state: u8,
    /// xsl:attribute elements
    pub attrs: Vec<NodeId>,
    pub use_attr_sets: Vec<(String, Option<String>)>,
}

/// An AVT (attribute value template) of a literal result element
#[derive(Debug)]
pub struct Avt {
    pub strstart: bool,
    pub segments: Vec<AvtSeg>,
    pub ns_list: Rc<Vec<(String, String)>>,
}

#[derive(Debug)]
pub enum AvtSeg {
    Str(String),
    Expr(Option<Rc<CompExpr>>),
}

/// One stylesheet module (xsltStylesheet): the principal one and imports.
#[derive(Debug, Default)]
pub struct Style {
    pub doc: NodeId,
    /// xsltGatherNamespaces (style->nsHash): prefix -> namespace name
    pub ns_hash: HashMap<String, String>,
    pub parent: Option<usize>,
    /// imported stylesheets, most recent first
    pub imports: Vec<usize>,
    pub strip_spaces: Option<HashMap<(String, Option<String>), bool>>,
    pub strip_all: i32,
    pub cdata_section: Option<HashSet<(String, Option<String>)>>,
    /// global variables/params in declaration order
    pub variables: Vec<GlobalDef>,
    /// template indices, in declaration order
    pub templates: Vec<usize>,
    pub templates_hash: HashMap<(String, Option<String>, Option<String>), Vec<usize>>,
    pub root_match: Vec<usize>,
    pub key_match: Vec<usize>,
    pub elem_match: Vec<usize>,
    pub attr_match: Vec<usize>,
    pub text_match: Vec<usize>,
    pub pi_match: Vec<usize>,
    pub comment_match: Vec<usize>,
    /// literal namespace -> target (None: UNDEFINED_DEFAULT_NS)
    pub ns_aliases: Vec<(String, Option<String>)>,
    pub attribute_sets: HashMap<(String, Option<String>), AttrSet>,
    pub keys: Vec<KeyDef>,
    pub method: Option<String>,
    pub method_uri: Option<String>,
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub omit_xml_declaration: i32,
    pub decimal_formats: Vec<DecimalFormat>,
    pub standalone: i32,
    pub doctype_public: Option<String>,
    pub doctype_system: Option<String>,
    pub indent: i32,
    pub media_type: Option<String>,
    pub warnings: i32,
    pub errors: i32,
    pub excl_prefix_tab: Vec<String>,
    /// extension prefixes (prefix, uri) (style->nsDefs), most recent first
    pub ns_defs: Vec<(Option<String>, String)>,
    /// URIs of extension modules initialized for this stylesheet
    pub ext_infos: HashSet<String>,
    /// documents included (for cycle detection)
    pub includes: Vec<NodeId>,
    /// style->docList: loaded documents (URL, doc)
    pub doc_list: Vec<(String, NodeId)>,
    pub default_alias: Option<String>,
    pub nopreproc: bool,
    pub literal_result: bool,
    pub forwards_compatible: bool,
    pub named_templates: HashMap<(String, Option<String>), usize>,
}

/// The compiled stylesheet with all its modules.
#[derive(Debug, Default)]
pub struct Sheet {
    pub styles: Vec<Style>,
    pub templates: Vec<Template>,
    pub pats: Vec<CompMatch>,
    /// psvi of XSLT instructions / extension elements
    pub precomp: HashMap<NodeId, Rc<PreComp>>,
    /// extension elements with no compile-time handler (xsltExtMarker)
    pub ext_marker: HashSet<NodeId>,
    /// precompiled AVTs of literal result element attributes
    pub avts: HashMap<NodeId, Rc<Avt>>,
    /// attribute sets (all moved to the principal stylesheet)
    pub attr_sets: HashMap<(String, Option<String>), AttrSet>,
    /// EXSLT func:function definitions: (uri, name) -> element
    pub ext_functions: HashMap<(String, String), NodeId>,
    /// the stylesheet module declaring each of ext_functions
    pub ext_function_style: HashMap<(String, String), usize>,
    /// profiler data of the templates (setProfiling())
    pub prof: std::cell::RefCell<Vec<crate::profile::ProfStat>>,
}

impl Sheet {
    /// xsltNextImport
    pub fn next_import(&self, cur: usize) -> Option<usize> {
        let s = &self.styles[cur];
        if let Some(&i) = s.imports.first() {
            return Some(i);
        }
        let mut c = cur;
        loop {
            let p = self.styles[c].parent?;
            let sib = &self.styles[p].imports;
            let pos = sib.iter().position(|&x| x == c)?;
            if pos + 1 < sib.len() {
                return Some(sib[pos + 1]);
            }
            c = p;
        }
    }

    /// All stylesheets in import-precedence order (style, then
    /// xsltNextImport...).
    pub fn import_order(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut cur = Some(0);
        while let Some(c) = cur {
            v.push(c);
            cur = self.next_import(c);
        }
        v
    }

    /// XSLT_GET_IMPORT_PTR
    pub fn get_import_str(&self, f: impl Fn(&Style) -> Option<&String>) -> Option<String> {
        for s in self.import_order() {
            if let Some(v) = f(&self.styles[s]) {
                return Some(v.clone());
            }
        }
        None
    }

    /// XSLT_GET_IMPORT_INT
    pub fn get_import_int(&self, f: impl Fn(&Style) -> i32) -> i32 {
        for s in self.import_order() {
            let v = f(&self.styles[s]);
            if v != -1 {
                return v;
            }
        }
        -1
    }

    pub fn has_keys(&self) -> bool {
        // whether the principal stylesheet has xsl:key top-level elements
        // is checked by the PHP glue directly on the tree
        self.styles.iter().any(|s| !s.keys.is_empty())
    }

    /// xsltDecimalFormatGetByQName
    pub fn decimal_format_by_qname(
        &self,
        ns_uri: Option<&str>,
        name: &str,
    ) -> Option<&DecimalFormat> {
        for s in self.import_order() {
            for f in self.styles[s].decimal_formats.iter().skip(1) {
                if f.ns_uri.as_deref() == ns_uri && f.name.as_deref() == Some(name) {
                    return Some(f);
                }
            }
        }
        None
    }

    /// xsltCheckExtPrefix(style, prefix)
    pub fn check_ext_prefix(&self, style: usize, prefix: Option<&str>) -> bool {
        let p = prefix.unwrap_or("#default");
        self.styles[style]
            .ns_defs
            .iter()
            .any(|(dp, _)| dp.as_deref() == Some(p))
    }

    /// xsltCheckExtURI
    pub fn check_ext_uri(&self, style: usize, uri: Option<&str>) -> bool {
        match uri {
            None => false,
            Some(u) => self.styles[style].ns_defs.iter().any(|(_, du)| du == u),
        }
    }
}

/// Is there a registered extension module for this namespace (EXSLT and
/// libxslt's own)?
pub fn is_ext_module(uri: &str) -> bool {
    matches!(
        uri,
        EXSLT_COMMON_NAMESPACE
            | EXSLT_FUNCTIONS_NAMESPACE
            | crate::exslt::EXSLT_MATH_NAMESPACE
            | crate::exslt::EXSLT_SETS_NAMESPACE
            | crate::exslt::EXSLT_STRINGS_NAMESPACE
            | crate::exslt::EXSLT_DATE_NAMESPACE
            | crate::exslt::EXSLT_DYNAMIC_NAMESPACE
            | crate::exslt::EXSLT_CRYPTO_NAMESPACE
            | crate::exslt::SAXON_NAMESPACE
    )
}

/// Registered extension elements (xsltElementsHash): returns whether
/// (name, uri) has a module element.
pub fn ext_module_element(name: &str, uri: &str) -> bool {
    matches!(
        (uri, name),
        (XSLT_LIBXSLT_NAMESPACE, "debug")
            | (XSLT_SAXON_NAMESPACE, "output")
            | (XSLT_XALAN_NAMESPACE, "write")
            | (XSLT_XT_NAMESPACE, "document")
            | (XSLT_NAMESPACE, "document")
            | (EXSLT_COMMON_NAMESPACE, "document")
            | (EXSLT_FUNCTIONS_NAMESPACE, "result")
    )
}

/// The compiler state.
pub struct Compiler<'a> {
    pub vm: &'a mut Vm,
    pub sheet: Sheet,
}

impl<'a> Compiler<'a> {
    fn err(&mut self, style: Option<usize>, node: Option<NodeId>, msg: &str) {
        transform_error(self.vm, false, style.is_some(), node, msg);
    }

    fn style_error(&mut self, s: usize, node: Option<NodeId>, msg: &str) {
        self.err(Some(s), node, msg);
        self.sheet.styles[s].errors += 1;
    }

    fn style_warning(&mut self, s: usize, node: Option<NodeId>, msg: &str) {
        self.err(Some(s), node, msg);
        self.sheet.styles[s].warnings += 1;
    }

    fn new_style(&mut self, parent: Option<usize>) -> usize {
        let st = Style {
            parent,
            omit_xml_declaration: -1,
            standalone: -1,
            indent: -1,
            decimal_formats: vec![DecimalFormat::new(None, None)],
            ..Default::default()
        };
        self.sheet.styles.push(st);
        self.sheet.styles.len() - 1
    }

    /// xsltCompilePattern for the parsing code
    pub fn compile_pattern(
        &mut self,
        pat: &str,
        doc: NodeId,
        node: NodeId,
        style: Option<usize>,
        novar: bool,
    ) -> Option<Vec<usize>> {
        pattern::compile_pattern(self, pat, doc, node, style, novar)
    }

    /// xsltXPathCompile
    pub fn xpath_compile(&mut self, expr: &str) -> Option<Rc<CompExpr>> {
        match xpath::compile(expr) {
            Ok(c) => Some(Rc::new(c)),
            Err(msgs) => {
                for m in msgs {
                    php_error(self.vm, &format!("{}\n", m));
                }
                None
            }
        }
    }

    // ------------------------------------------------------------------
    // Entry point

    /// xsltParseStylesheetDoc: compile `doc` (which the stylesheet takes
    /// ownership of). None when the stylesheet had errors.
    pub fn parse_stylesheet_doc(vm: &'a mut Vm, doc: NodeId) -> Option<Sheet> {
        let mut c = Compiler {
            vm,
            sheet: Sheet::default(),
        };
        let s = c.new_style(None);
        if c.parse_stylesheet_user(s, doc).is_err() {
            return None;
        }
        Some(c.sheet)
    }

    /// xsltParseStylesheetUser
    fn parse_stylesheet_user(&mut self, s: usize, doc: NodeId) -> Result<(), ()> {
        // xsltGatherNamespaces(style) runs before style->doc is set in
        // libxslt 1.1.45: it never finds the root element, so style->nsHash
        // stays NULL (ext/xsl fills it for modern DOM documents).
        self.sheet.styles[s].doc = doc;
        if self.parse_stylesheet_process(s, doc).is_none() {
            return Err(());
        }
        if self.sheet.styles[s].parent.is_none() {
            self.resolve_stylesheet_attribute_set(s);
        }
        if self.sheet.styles[s].errors != 0 {
            return Err(());
        }
        Ok(())
    }

    /// xsltGatherNamespaces: warn about a prefix bound to several URIs.
    #[allow(dead_code)]
    fn gather_namespaces(&mut self, s: usize, doc: NodeId) {
        let mut hash: HashMap<String, String> = HashMap::new();
        let nodes: Vec<(NodeId, Vec<(String, String)>)> = tw(|t| {
            let mut out = Vec::new();
            let root = match t.root_element(doc) {
                Some(r) => r,
                None => return out,
            };
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                if t.nodes[n].t == NType::Element {
                    let defs: Vec<(String, String)> = t.nodes[n]
                        .ns_def
                        .iter()
                        .filter_map(|&ns| {
                            t.nss[ns]
                                .prefix
                                .clone()
                                .map(|p| (p, t.nss[ns].href.clone()))
                        })
                        .collect();
                    if !defs.is_empty() {
                        out.push((n, defs));
                    }
                }
                let kids = t.children_of(n);
                if let Some(&f) = kids.first() {
                    if t.nodes[f].t == NType::EntityDecl {
                        continue;
                    }
                }
                for k in kids.into_iter().rev() {
                    stack.push(k);
                }
            }
            out
        });
        for (n, defs) in nodes {
            for (p, h) in defs {
                match hash.get(&p) {
                    Some(u) if *u != h => {
                        self.style_warning(
                            s,
                            Some(n),
                            &format!("Namespaces prefix {} used for multiple namespaces\n", p),
                        );
                    }
                    Some(_) => {}
                    None => {
                        hash.insert(p, h);
                    }
                }
            }
        }
        self.sheet.styles[s].ns_hash = hash;
    }

    /// xsltParseStylesheetProcess
    fn parse_stylesheet_process(&mut self, s: usize, doc: NodeId) -> Option<()> {
        let cur = match tw(|t| t.root_element(doc)) {
            Some(c) => c,
            None => {
                self.err(
                    Some(s),
                    Some(doc),
                    "xsltParseStylesheetProcess : empty stylesheet\n",
                );
                return None;
            }
        };
        let is_ss = tw(|t| {
            is_xslt_elem(t, cur)
                && (is_xslt_name(t, cur, "stylesheet") || is_xslt_name(t, cur, "transform"))
        });
        if is_ss {
            self.sheet.styles[s].literal_result = false;
            self.parse_exclude_prefix(s, cur, true);
            self.parse_ext_prefix(s, cur, true);
        } else {
            self.parse_exclude_prefix(s, cur, false);
            self.parse_ext_prefix(s, cur, false);
            self.sheet.styles[s].literal_result = true;
        }
        if !self.sheet.styles[s].nopreproc {
            self.preprocess_stylesheet(s, cur);
        }
        if !self.sheet.styles[s].literal_result {
            self.parse_stylesheet_top(s, cur);
        } else {
            let prop = tw(|t| get_ns_prop(t, cur, "version", Some(XSLT_NAMESPACE)));
            let prop = match prop {
                None => {
                    self.err(
                        Some(s),
                        Some(cur),
                        "xsltParseStylesheetProcess : document is not a stylesheet\n",
                    );
                    return None;
                }
                Some(p) => p,
            };
            if prop != "1.0" && prop != "1.1" {
                self.err(
                    Some(s),
                    Some(cur),
                    "xsl:version: only 1.1 features are supported\n",
                );
                self.sheet.styles[s].forwards_compatible = true;
                self.sheet.styles[s].warnings += 1;
            }
            let ti = self.sheet.templates.len();
            let position = self.sheet.styles[s].templates.len() as i32;
            self.sheet.templates.push(Template {
                style: s,
                match_: Some("/".into()),
                priority: XSLT_PAT_NO_PRIORITY,
                name: None,
                name_uri: None,
                mode: None,
                mode_uri: None,
                content: None,
                elem: doc,
                inherited_ns: Vec::new(),
                position,
            });
            self.sheet.styles[s].templates.push(ti);
            self.parse_template_content(s, doc);
            let content = tw(|t| t.nodes[doc].children);
            self.sheet.templates[ti].content = content;
            self.add_template(s, ti);
            self.sheet.styles[s].literal_result = true;
        }
        Some(())
    }

    /// xsltParseStylesheetExcludePrefix: returns the number pushed.
    fn parse_exclude_prefix(&mut self, s: usize, cur: NodeId, is_xslt: bool) -> usize {
        let prefixes = tw(|t| {
            if is_xslt {
                get_ns_prop(t, cur, "exclude-result-prefixes", None)
            } else {
                get_ns_prop(t, cur, "exclude-result-prefixes", Some(XSLT_NAMESPACE))
            }
        });
        let prefixes = match prefixes {
            None => return 0,
            Some(p) => p,
        };
        let mut nb = 0;
        for prefix in prefixes
            .split([' ', '\t', '\n', '\r'])
            .filter(|x| !x.is_empty())
        {
            let ns = tw(|t| {
                let p = if prefix == "#default" {
                    None
                } else {
                    Some(prefix)
                };
                t.search_ns(cur, p).map(|n| t.nss[n].href.clone())
            });
            match ns {
                None => self.style_warning(
                    s,
                    Some(cur),
                    &format!(
                        "xsl:exclude-result-prefixes : undefined namespace {}\n",
                        prefix
                    ),
                ),
                Some(h) => {
                    let st = &mut self.sheet.styles[s];
                    if !st.excl_prefix_tab.contains(&h) {
                        st.excl_prefix_tab.push(h);
                        nb += 1;
                    }
                }
            }
        }
        nb
    }

    /// xsltParseStylesheetExtPrefix
    fn parse_ext_prefix(&mut self, s: usize, cur: NodeId, is_xslt: bool) {
        let prefixes = tw(|t| {
            if is_xslt {
                get_ns_prop(t, cur, "extension-element-prefixes", None)
            } else {
                get_ns_prop(t, cur, "extension-element-prefixes", Some(XSLT_NAMESPACE))
            }
        });
        let prefixes = match prefixes {
            None => return,
            Some(p) => p,
        };
        for prefix in prefixes
            .split([' ', '\t', '\n', '\r'])
            .filter(|x| !x.is_empty())
        {
            let ns = tw(|t| {
                let p = if prefix == "#default" {
                    None
                } else {
                    Some(prefix)
                };
                t.search_ns(cur, p).map(|n| t.nss[n].href.clone())
            });
            match ns {
                None => self.style_warning(
                    s,
                    Some(cur),
                    &format!(
                        "xsl:extension-element-prefix : undefined namespace {}\n",
                        prefix
                    ),
                ),
                Some(h) => self.register_ext_prefix(s, prefix, &h),
            }
        }
    }

    /// xsltRegisterExtPrefix
    fn register_ext_prefix(&mut self, s: usize, prefix: &str, uri: &str) {
        let st = &mut self.sheet.styles[s];
        if st.ns_defs.iter().any(|(p, _)| p.as_deref() == Some(prefix)) {
            return;
        }
        st.ns_defs
            .insert(0, (Some(prefix.to_string()), uri.to_string()));
        if is_ext_module(uri) {
            st.ext_infos.insert(uri.to_string());
        }
    }

    /// xsltPreprocessStylesheet (including its quirk: the recursive call
    /// made for an element with exclude-result-prefixes walks on past the
    /// element, up to the end of the document)
    fn preprocess_stylesheet(&mut self, s: usize, start: NodeId) {
        let styleelem = tw(|t| {
            if is_xslt_elem(t, start) && is_xslt_name(t, start, "stylesheet") {
                Some(start)
            } else {
                None
            }
        });
        let mut delete: Option<NodeId> = None;
        let mut cur = Some(start);
        while let Some(c) = cur {
            if let Some(d) = delete.take() {
                tw(|t| t.unlink(d));
            }
            let ctype = tw(|t| t.nodes[c].t);
            let mut skip_children = false;
            if ctype == NType::Element {
                let is_x = tw(|t| is_xslt_elem(t, c));
                let mut excl = 0;
                if is_x {
                    if tw(|t| is_xslt_name(t, c, "text")) {
                        skip_children = true;
                    }
                } else {
                    excl = self.parse_exclude_prefix(s, c, false);
                }
                if !skip_children {
                    let excl_tab = self.sheet.styles[s].excl_prefix_tab.clone();
                    if !excl_tab.is_empty() {
                        tw(|t| {
                            let root = t.nodes[c].doc.and_then(|d| t.root_element(d));
                            if let Some(root) = root {
                                if root != c && !t.nodes[c].ns_def.is_empty() {
                                    let defs = t.nodes[c].ns_def.clone();
                                    let mut keep = Vec::new();
                                    let mut moved = Vec::new();
                                    for ns in defs {
                                        let n = &t.nss[ns];
                                        if n.prefix.is_some() && excl_tab.contains(&n.href) {
                                            moved.push(ns);
                                        } else {
                                            keep.push(ns);
                                        }
                                    }
                                    if !moved.is_empty() {
                                        t.nodes[c].ns_def = keep;
                                        for ns in moved {
                                            t.nss[ns].owner = Some(root);
                                            t.nodes[root].ns_def.insert(0, ns);
                                        }
                                    }
                                }
                            }
                        });
                    }
                    if excl > 0 {
                        if let Some(f) = tw(|t| t.nodes[c].children) {
                            self.preprocess_stylesheet(s, f);
                        }
                        for _ in 0..excl {
                            self.sheet.styles[s].excl_prefix_tab.pop();
                        }
                        skip_children = true;
                    }
                }
            } else if ctype == NType::Text {
                let (blank, parent) = tw(|t| (is_blank_node(t, c), t.nodes[c].parent));
                if blank {
                    let preserve = parent
                        .map(|p| tw(|t| get_space_preserve(t, p)))
                        .unwrap_or(-1);
                    if preserve != 1 {
                        delete = Some(c);
                    }
                }
            } else if ctype != NType::CData {
                delete = Some(c);
                skip_children = true;
            }
            if !skip_children {
                let (is_foreign, href, children) = tw(|t| {
                    let nd = &t.nodes[c];
                    let foreign = nd.t == NType::Element
                        && nd.ns.is_some()
                        && styleelem.is_some()
                        && nd.parent == styleelem
                        && t.ns_href(nd.ns) != Some(XSLT_NAMESPACE);
                    (
                        foreign,
                        t.ns_href(nd.ns).map(|s| s.to_string()),
                        nd.children,
                    )
                });
                let foreign = is_foreign && !self.sheet.check_ext_uri(s, href.as_deref());
                if !foreign {
                    if let Some(ch) = children {
                        cur = Some(ch);
                        continue;
                    }
                }
            }
            cur = tw(|t| {
                if let Some(n) = t.nodes[c].next {
                    return Some(n);
                }
                let mut x = c;
                loop {
                    x = t.nodes[x].parent?;
                    if matches!(t.nodes[x].t, NType::Document | NType::HtmlDocument) {
                        return None;
                    }
                    if let Some(n) = t.nodes[x].next {
                        return Some(n);
                    }
                }
            });
        }
        if let Some(d) = delete {
            tw(|t| t.unlink(d));
        }
    }

    /// xsltParseContentError
    fn parse_content_error(&mut self, s: usize, node: NodeId) {
        let (is_x, name) = tw(|t| (is_xslt_elem(t, node), t.nodes[node].name.clone()));
        if is_x {
            self.err(
                Some(s),
                Some(node),
                &format!(
                    "The XSLT-element '{}' is not allowed at this position.\n",
                    name
                ),
            );
        } else {
            self.err(
                Some(s),
                Some(node),
                &format!("The element '{}' is not allowed at this position.\n", name),
            );
        }
        self.sheet.styles[s].errors += 1;
    }

    /// xsltParseStylesheetTop
    fn parse_stylesheet_top(&mut self, s: usize, top: NodeId) {
        let prop = tw(|t| get_ns_prop(t, top, "version", None));
        match prop {
            None => self.style_warning(
                s,
                Some(top),
                "xsl:version is missing: document may not be a stylesheet\n",
            ),
            Some(p) => {
                if p != "1.0" && p != "1.1" {
                    self.err(
                        Some(s),
                        Some(top),
                        "xsl:version: only 1.1 features are supported\n",
                    );
                    self.sheet.styles[s].forwards_compatible = true;
                    self.sheet.styles[s].warnings += 1;
                }
            }
        }
        let mut cur = tw(|t| t.nodes[top].children);
        // xsl:import elements
        while let Some(c) = cur {
            if tw(|t| is_blank_node(t, c)) {
                cur = tw(|t| t.nodes[c].next);
                continue;
            }
            if tw(|t| is_xslt_elem_named(t, c, "import")) {
                if self.parse_stylesheet_import(s, c).is_err() {
                    self.sheet.styles[s].errors += 1;
                }
            } else {
                break;
            }
            cur = tw(|t| t.nodes[c].next);
        }
        while let Some(c) = cur {
            let next = tw(|t| t.nodes[c].next);
            if tw(|t| is_blank_node(t, c)) {
                cur = next;
                continue;
            }
            let (ctype, has_ns, is_x, name, content) = tw(|t| {
                let nd = &t.nodes[c];
                (
                    nd.t,
                    nd.ns.is_some(),
                    is_xslt_elem(t, c),
                    nd.name.clone(),
                    nd.content.clone(),
                )
            });
            if ctype == NType::Text {
                self.err(
                    Some(s),
                    Some(c),
                    &format!("misplaced text node: '{}'\n", content),
                );
                self.sheet.styles[s].errors += 1;
                cur = next;
                continue;
            }
            if ctype == NType::Element && !has_ns {
                php_error(
                    self.vm,
                    &format!(
                        "Found a top-level element {} with null namespace URI\n",
                        name
                    ),
                );
                self.sheet.styles[s].errors += 1;
                cur = next;
                continue;
            }
            if ctype == NType::Element && !is_x {
                let uri = tw(|t| t.ns_href(t.nodes[c].ns).unwrap_or("").to_string());
                if uri == EXSLT_FUNCTIONS_NAMESPACE && name == "function" {
                    crate::exslt::func_function_comp(self, s, c);
                }
                cur = next;
                continue;
            }
            match name.as_str() {
                "import" => {
                    self.err(
                        Some(s),
                        Some(c),
                        "xsltParseStylesheetTop: ignoring misplaced import element\n",
                    );
                    self.sheet.styles[s].errors += 1;
                }
                "include" => {
                    if self.parse_stylesheet_include(s, c).is_err() {
                        self.sheet.styles[s].errors += 1;
                    }
                }
                "strip-space" => self.parse_strip_space(s, c, true),
                "preserve-space" => self.parse_strip_space(s, c, false),
                "output" => self.parse_output(s, c),
                "key" => self.parse_key(s, c),
                "decimal-format" => self.parse_decimal_format(s, c),
                "attribute-set" => self.parse_attribute_set(s, c),
                "variable" | "param" => self.parse_global_variable(s, c, name == "param"),
                "template" => self.parse_template(s, c),
                "namespace-alias" => self.namespace_alias(s, c),
                _ => {
                    if !self.sheet.styles[s].forwards_compatible {
                        self.err(
                            Some(s),
                            Some(c),
                            &format!("xsltParseStylesheetTop: unknown {} element\n", name),
                        );
                        self.sheet.styles[s].errors += 1;
                    }
                }
            }
            cur = next;
        }
    }

    /// xsltCheckCycle
    fn check_cycle(&mut self, s: usize, cur: NodeId, uri: &str) -> Result<(), ()> {
        let mut depth = 0;
        let mut anc = Some(s);
        while let Some(a) = anc {
            depth += 1;
            if depth >= XSLT_MAX_NESTING {
                self.err(
                    Some(s),
                    Some(cur),
                    &format!("maximum nesting depth exceeded: {}\n", uri),
                );
                return Err(());
            }
            let durl = tw(|t| doc_url(t, self.sheet.styles[a].doc));
            if durl.as_deref() == Some(uri) {
                self.err(
                    Some(s),
                    Some(cur),
                    &format!("recursion detected on imported URL {}\n", uri),
                );
                return Err(());
            }
            let incs = self.sheet.styles[a].includes.clone();
            for d in incs {
                depth += 1;
                if depth >= XSLT_MAX_NESTING {
                    self.err(
                        Some(s),
                        Some(cur),
                        &format!("maximum nesting depth exceeded: {}\n", uri),
                    );
                    return Err(());
                }
                if tw(|t| doc_url(t, d)).as_deref() == Some(uri) {
                    self.err(
                        Some(s),
                        Some(cur),
                        &format!("recursion detected on included URL {}\n", uri),
                    );
                    return Err(());
                }
            }
            anc = self.sheet.styles[a].parent;
        }
        Ok(())
    }

    /// xsltDocDefaultLoader for stylesheets
    fn load_doc(&mut self, uri: &str) -> Option<NodeId> {
        crate::transform::load_xml_document(self.vm, uri, XSLT_PARSE_OPTIONS)
    }

    /// xsltParseStylesheetImport
    fn parse_stylesheet_import(&mut self, s: usize, cur: NodeId) -> Result<(), ()> {
        let href = match tw(|t| get_ns_prop(t, cur, "href", None)) {
            Some(h) => h,
            None => {
                self.err(Some(s), Some(cur), "xsl:import : missing href attribute\n");
                return Err(());
            }
        };
        let base = tw(|t| node_get_base(t, cur));
        let uri = match build_uri(&href, base.as_deref()) {
            Some(u) => u,
            None => {
                self.err(
                    Some(s),
                    Some(cur),
                    &format!("xsl:import : invalid URI reference {}\n", href),
                );
                return Err(());
            }
        };
        self.check_cycle(s, cur, &uri)?;
        let import = match self.load_doc(&uri) {
            Some(d) => d,
            None => {
                self.err(
                    Some(s),
                    Some(cur),
                    &format!("xsl:import : unable to load {}\n", uri),
                );
                return Err(());
            }
        };
        let r = self.new_style(Some(s));
        if self.parse_stylesheet_user(r, import).is_ok() {
            self.sheet.styles[s].imports.insert(0, r);
            Ok(())
        } else {
            Err(())
        }
    }

    /// xsltParseStylesheetInclude
    fn parse_stylesheet_include(&mut self, s: usize, cur: NodeId) -> Result<(), ()> {
        let href = match tw(|t| get_ns_prop(t, cur, "href", None)) {
            Some(h) => h,
            None => {
                self.err(Some(s), Some(cur), "xsl:include : missing href attribute\n");
                return Err(());
            }
        };
        let base = tw(|t| node_get_base(t, cur));
        let uri = match build_uri(&href, base.as_deref()) {
            Some(u) => u,
            None => {
                self.err(
                    Some(s),
                    Some(cur),
                    &format!("xsl:include : invalid URI reference {}\n", href),
                );
                return Err(());
            }
        };
        self.check_cycle(s, cur, &uri)?;
        // xsltLoadStyleDocument
        let (doc, preproc) = match self.sheet.styles[s]
            .doc_list
            .iter()
            .find(|(u, _)| *u == uri)
            .map(|(_, d)| *d)
        {
            Some(d) => (d, true),
            None => match self.load_doc(&uri) {
                Some(d) => {
                    self.sheet.styles[s].doc_list.push((uri.clone(), d));
                    (d, false)
                }
                None => {
                    self.err(
                        Some(s),
                        Some(cur),
                        &format!("xsl:include : unable to load {}\n", uri),
                    );
                    return Err(());
                }
            },
        };
        let old_doc = self.sheet.styles[s].doc;
        self.sheet.styles[s].doc = doc;
        self.sheet.styles[s].includes.push(doc);
        let old_nopreproc = self.sheet.styles[s].nopreproc;
        self.sheet.styles[s].nopreproc = preproc;
        let result = self.parse_stylesheet_process(s, doc);
        self.sheet.styles[s].nopreproc = old_nopreproc;
        self.sheet.styles[s].includes.pop();
        self.sheet.styles[s].doc = old_doc;
        if result.is_none() {
            return Err(());
        }
        Ok(())
    }

    /// xsltParseStylesheetStripSpace / PreserveSpace
    fn parse_strip_space(&mut self, s: usize, cur: NodeId, strip: bool) {
        if strip && self.sheet.styles[s].strip_spaces.is_none() {
            self.sheet.styles[s].strip_spaces = Some(HashMap::new());
        }
        let elements = match tw(|t| get_ns_prop(t, cur, "elements", None)) {
            Some(e) => e,
            None => {
                let msg = if strip {
                    "xsltParseStylesheetStripSpace: missing elements attribute\n"
                } else {
                    "xsltParseStylesheetPreserveSpace: missing elements attribute\n"
                };
                self.style_warning(s, Some(cur), msg);
                return;
            }
        };
        if self.sheet.styles[s].strip_spaces.is_none() {
            self.sheet.styles[s].strip_spaces = Some(HashMap::new());
        }
        for el in elements
            .split([' ', '\t', '\n', '\r'])
            .filter(|x| !x.is_empty())
        {
            if el == "*" {
                self.sheet.styles[s].strip_all = if strip { 1 } else { -1 };
            } else {
                let (uri, local) = match get_qname_uri(self.vm, cur, el) {
                    Ok(x) => x,
                    Err(()) => continue,
                };
                let h = self.sheet.styles[s].strip_spaces.as_mut().unwrap();
                h.entry((local, uri)).or_insert(strip);
            }
        }
        if let Some(ch) = tw(|t| t.nodes[cur].children) {
            self.parse_content_error(s, ch);
        }
    }

    /// xsltParseStylesheetOutput
    fn parse_output(&mut self, s: usize, cur: NodeId) {
        let get = |name: &str| tw(|t| get_ns_prop(t, cur, name, None));
        if let Some(p) = get("version") {
            self.sheet.styles[s].version = Some(p);
        }
        if let Some(p) = get("encoding") {
            self.sheet.styles[s].encoding = Some(p);
        }
        if let Some(p) = get("method") {
            self.sheet.styles[s].method = None;
            self.sheet.styles[s].method_uri = None;
            match get_qname_uri(self.vm, cur, &p) {
                Err(()) => self.sheet.styles[s].errors += 1,
                Ok((None, local)) => {
                    if local == "xml" || local == "html" || local == "text" {
                        self.sheet.styles[s].method = Some(local);
                    } else {
                        self.style_warning(
                            s,
                            Some(cur),
                            &format!("invalid value for method: {}\n", local),
                        );
                    }
                }
                Ok((Some(u), local)) => {
                    self.sheet.styles[s].method = Some(local);
                    self.sheet.styles[s].method_uri = Some(u);
                }
            }
        }
        if let Some(p) = get("doctype-system") {
            self.sheet.styles[s].doctype_system = Some(p);
        }
        if let Some(p) = get("doctype-public") {
            self.sheet.styles[s].doctype_public = Some(p);
        }
        for (attr, field) in [
            ("standalone", 0),
            ("indent", 1),
            ("omit-xml-declaration", 2),
        ] {
            if let Some(p) = get(attr) {
                let v = match p.as_str() {
                    "yes" => 1,
                    "no" => 0,
                    _ => {
                        self.style_error(
                            s,
                            Some(cur),
                            &format!("invalid value for {}: {}\n", attr, p),
                        );
                        continue;
                    }
                };
                let st = &mut self.sheet.styles[s];
                match field {
                    0 => st.standalone = v,
                    1 => st.indent = v,
                    _ => st.omit_xml_declaration = v,
                }
            }
        }
        if let Some(elements) = get("cdata-section-elements") {
            if self.sheet.styles[s].cdata_section.is_none() {
                self.sheet.styles[s].cdata_section = Some(HashSet::new());
            }
            for el in elements
                .split([' ', '\t', '\n', '\r'])
                .filter(|x| !x.is_empty())
            {
                if !valid_qname(el) {
                    self.style_error(
                        s,
                        Some(cur),
                        &format!(
                            "Attribute 'cdata-section-elements': The value '{}' is not a valid QName.\n",
                            el
                        ),
                    );
                    continue;
                }
                match get_qname_uri(self.vm, cur, el) {
                    Err(()) => {
                        self.style_error(
                            s,
                            Some(cur),
                            "Attribute 'cdata-section-elements': Not a valid QName.\n",
                        );
                    }
                    Ok((uri, local)) => {
                        let uri = match uri {
                            Some(u) => Some(u),
                            None => tw(|t| t.search_ns(cur, None).map(|n| t.nss[n].href.clone())),
                        };
                        self.sheet.styles[s]
                            .cdata_section
                            .as_mut()
                            .unwrap()
                            .insert((local, uri));
                    }
                }
            }
        }
        if let Some(p) = get("media-type") {
            self.sheet.styles[s].media_type = Some(p);
        }
        if let Some(ch) = tw(|t| t.nodes[cur].children) {
            self.parse_content_error(s, ch);
        }
    }

    /// xsltParseStylesheetDecimalFormat
    fn parse_decimal_format(&mut self, s: usize, cur: NodeId) {
        let get = |name: &str| tw(|t| get_ns_prop(t, cur, name, None));
        let mut idx = 0usize;
        if let Some(prop) = get("name") {
            if !valid_qname(&prop) {
                self.style_warning(
                    s,
                    Some(cur),
                    &format!("xsl:decimal-format: Invalid QName '{}'.\n", prop),
                );
                return;
            }
            let (uri, local) = match get_qname_uri(self.vm, cur, &prop) {
                Ok(x) => x,
                Err(()) => {
                    self.sheet.styles[s].warnings += 1;
                    return;
                }
            };
            // xsltDecimalFormatGetByQName on this style and its imports
            let exists = {
                let mut found = false;
                let mut c = Some(s);
                while let Some(x) = c {
                    if self.sheet.styles[x]
                        .decimal_formats
                        .iter()
                        .skip(1)
                        .any(|f| f.ns_uri == uri && f.name.as_deref() == Some(local.as_str()))
                    {
                        found = true;
                        break;
                    }
                    c = self.sheet.next_import(x);
                }
                found
            };
            if exists {
                self.style_warning(
                    s,
                    Some(cur),
                    &format!(
                        "xsltParseStylestyleDecimalFormat: {} already exists\n",
                        local
                    ),
                );
                return;
            }
            self.sheet.styles[s]
                .decimal_formats
                .push(DecimalFormat::new(uri, Some(local)));
            idx = self.sheet.styles[s].decimal_formats.len() - 1;
        }
        let fields = [
            "decimal-separator",
            "grouping-separator",
            "infinity",
            "minus-sign",
            "NaN",
            "percent",
            "per-mille",
            "zero-digit",
            "digit",
            "pattern-separator",
        ];
        for f in fields {
            if let Some(p) = get(f) {
                let df = &mut self.sheet.styles[s].decimal_formats[idx];
                match f {
                    "decimal-separator" => df.decimal_point = p,
                    "grouping-separator" => df.grouping = p,
                    "infinity" => df.infinity = p,
                    "minus-sign" => df.minus_sign = p,
                    "NaN" => df.no_number = p,
                    "percent" => df.percent = p,
                    "per-mille" => df.permille = p,
                    "zero-digit" => df.zero_digit = p,
                    "digit" => df.digit = p,
                    _ => df.pattern_separator = p,
                }
            }
        }
        if let Some(ch) = tw(|t| t.nodes[cur].children) {
            self.parse_content_error(s, ch);
        }
    }

    /// xsltParseStylesheetKey + xsltAddKey
    fn parse_key(&mut self, s: usize, key: NodeId) {
        let get = |name: &str| tw(|t| get_ns_prop(t, key, name, None));
        let mut body = || -> Option<()> {
            let prop = match get("name") {
                Some(p) => p,
                None => {
                    self.style_error(s, Some(key), "xsl:key : error missing name\n");
                    return None;
                }
            };
            let (uri, name) = match get_qname_uri(self.vm, key, &prop) {
                Ok(x) => x,
                Err(()) => {
                    self.sheet.styles[s].errors += 1;
                    return None;
                }
            };
            let match_ = match get("match") {
                Some(m) => m,
                None => {
                    self.style_error(s, Some(key), "xsl:key : error missing match\n");
                    return None;
                }
            };
            let use_ = match get("use") {
                Some(u) => u,
                None => {
                    self.style_error(s, Some(key), "xsl:key : error missing use\n");
                    return None;
                }
            };
            self.add_key(s, &name, uri, &match_, &use_, key);
            Some(())
        };
        let _ = body();
        if let Some(ch) = tw(|t| t.nodes[key].children) {
            self.parse_content_error(s, ch);
        }
    }

    /// xsltAddKey
    fn add_key(
        &mut self,
        s: usize,
        name: &str,
        name_uri: Option<String>,
        match_: &str,
        use_: &str,
        inst: NodeId,
    ) {
        let ns_list = Rc::new(tw(|t| xpath_ns_list(t, inst)));
        let b = match_.as_bytes();
        let mut pattern = String::new();
        let mut current = 0usize;
        let mut have_pattern = false;
        while current < b.len() {
            let start = current;
            while current < b.len() && matches!(b[current], b' ' | b'\t' | b'\n' | b'\r') {
                current += 1;
            }
            let mut end = current;
            while end < b.len() && b[end] != b'|' {
                if b[end] == b'[' {
                    match skip_predicate(b, end) {
                        Some(e) => end = e,
                        None => {
                            self.style_error(
                                s,
                                Some(inst),
                                &format!("xsl:key : 'match' pattern is malformed: {}", match_),
                            );
                            return;
                        }
                    }
                } else {
                    end += 1;
                }
            }
            if current == end {
                self.style_error(s, Some(inst), "xsl:key : 'match' pattern is empty\n");
                return;
            }
            if b[start] != b'/' {
                pattern.push_str("//");
            }
            pattern.push_str(&match_[start..end]);
            have_pattern = true;
            if end < b.len() && b[end] == b'|' {
                pattern.push('|');
                end += 1;
            }
            current = end;
        }
        if !have_pattern {
            self.style_error(s, Some(inst), "xsl:key : 'match' pattern is empty\n");
            return;
        }
        let comp = self.xpath_compile(&pattern);
        if comp.is_none() {
            self.style_error(
                s,
                Some(inst),
                &format!(
                    "xsl:key : 'match' pattern compilation failed '{}'\n",
                    pattern
                ),
            );
        }
        let usecomp = self.xpath_compile(use_);
        if usecomp.is_none() {
            self.style_error(
                s,
                Some(inst),
                &format!("xsl:key : 'use' expression compilation failed '{}'\n", use_),
            );
        }
        self.sheet.styles[s].keys.push(KeyDef {
            name: name.to_string(),
            name_uri,
            match_: match_.to_string(),
            use_: use_.to_string(),
            inst,
            ns_list,
            comp,
            usecomp,
        });
    }

    /// xsltParseStylesheetAttributeSet
    fn parse_attribute_set(&mut self, s: usize, cur: NodeId) {
        let value = tw(|t| get_ns_prop(t, cur, "name", None));
        let value = match value {
            Some(v) if !v.is_empty() => v,
            _ => {
                php_error(self.vm, "xsl:attribute-set : name is missing\n");
                return;
            }
        };
        if !valid_qname(&value) {
            self.style_error(
                s,
                Some(cur),
                &format!(
                    "xsl:attribute-set : The name '{}' is not a valid QName.\n",
                    value
                ),
            );
            return;
        }
        let (prefix, ncname) = split_qname(&value);
        let mut ns_uri = None;
        if let Some(p) = &prefix {
            match tw(|t| t.search_ns(cur, Some(p)).map(|n| t.nss[n].href.clone())) {
                None => {
                    self.style_error(
                        s,
                        Some(cur),
                        &format!(
                            "xsl:attribute-set : No namespace found for QName '{}:{}'\n",
                            p, ncname
                        ),
                    );
                    return;
                }
                Some(h) => ns_uri = Some(h),
            }
        }
        let key = (ncname.clone(), ns_uri.clone());
        self.sheet.styles[s]
            .attribute_sets
            .entry(key.clone())
            .or_default();
        let children = tw(|t| t.children_of(cur));
        for child in children {
            let (ctype, has_ns, is_x, cname) = tw(|t| {
                let nd = &t.nodes[child];
                (
                    nd.t,
                    nd.ns.is_some(),
                    is_xslt_elem(t, child),
                    nd.name.clone(),
                )
            });
            if ctype != NType::Element || !has_ns || !is_x {
                if ctype == NType::Element {
                    self.err(
                        Some(s),
                        Some(child),
                        &format!("xsl:attribute-set : unexpected child {}\n", cname),
                    );
                } else {
                    self.err(
                        Some(s),
                        Some(child),
                        "xsl:attribute-set : child of unexpected type\n",
                    );
                }
            } else if cname != "attribute" {
                self.err(
                    Some(s),
                    Some(child),
                    &format!("xsl:attribute-set : unexpected child xsl:{}\n", cname),
                );
            } else {
                self.style_pre_compute(s, child);
                if tw(|t| t.nodes[child].children.is_some()) {
                    self.parse_template_content(s, child);
                }
                if !self.sheet.precomp.contains_key(&child) {
                    self.err(
                        Some(s),
                        Some(child),
                        &format!(
                            "xsl:attribute-set : internal error, attribute {} not compiled\n",
                            cname
                        ),
                    );
                } else {
                    self.sheet.styles[s]
                        .attribute_sets
                        .get_mut(&key)
                        .unwrap()
                        .attrs
                        .push(child);
                }
            }
        }
        if let Some(value) = tw(|t| get_ns_prop(t, cur, "use-attribute-sets", None)) {
            for curval in value
                .split([' ', '\t', '\n', '\r'])
                .filter(|x| !x.is_empty())
            {
                if !valid_qname(curval) {
                    self.style_error(
                        s,
                        Some(cur),
                        &format!(
                            "xsl:attribute-set : The name '{}' in use-attribute-sets is not a valid QName.\n",
                            curval
                        ),
                    );
                    return;
                }
                let (p2, n2) = split_qname(curval);
                let mut u2 = None;
                if let Some(p) = &p2 {
                    match tw(|t| t.search_ns(cur, Some(p)).map(|n| t.nss[n].href.clone())) {
                        None => {
                            self.style_error(
                                s,
                                Some(cur),
                                &format!(
                                    "xsl:attribute-set : No namespace found for QName '{}:{}' in use-attribute-sets\n",
                                    p, n2
                                ),
                            );
                            return;
                        }
                        Some(h) => u2 = Some(h),
                    }
                }
                let set = self.sheet.styles[s].attribute_sets.get_mut(&key).unwrap();
                if !set.use_attr_sets.iter().any(|(a, b)| *a == n2 && *b == u2) {
                    set.use_attr_sets.push((n2, u2));
                }
            }
        }
    }

    fn merge_attr_sets(&mut self, set: &mut AttrSet, other: &AttrSet) {
        for &old in &other.attrs {
            let oc = self.sheet.precomp.get(&old).cloned();
            let mut add = true;
            for &c in &set.attrs {
                let cc = self.sheet.precomp.get(&c).cloned();
                if let (Some(cc), Some(oc)) = (&cc, &oc) {
                    if cc.name == oc.name && cc.ns == oc.ns {
                        add = false;
                        break;
                    }
                }
            }
            if add {
                set.attrs.push(old);
            }
        }
    }

    /// xsltResolveUseAttrSets
    fn resolve_use_attr_sets(&mut self, set: &mut AttrSet, top: usize, depth: i32) {
        let uses = std::mem::take(&mut set.use_attr_sets);
        for (ncname, ns) in uses {
            let mut cur = Some(top);
            while let Some(c) = cur {
                let key = (ncname.clone(), ns.clone());
                if self.sheet.styles[c].attribute_sets.contains_key(&key) {
                    let mut other = self.sheet.styles[c].attribute_sets.remove(&key).unwrap();
                    // the set stays visible (as being resolved) to nested lookups
                    self.sheet.styles[c]
                        .attribute_sets
                        .insert(key.clone(), placeholder(&other));
                    self.resolve_attr_set(&mut other, top, c, &ncname, ns.as_deref(), depth + 1);
                    self.merge_attr_sets(set, &other);
                    self.sheet.styles[c].attribute_sets.insert(key, other);
                    break;
                }
                // sets already moved to the principal stylesheet
                if c == top {
                    if let Some(other) = self.sheet.attr_sets.get(&key).cloned() {
                        self.merge_attr_sets(set, &other);
                        break;
                    }
                }
                cur = self.sheet.next_import(c);
            }
        }
    }

    /// xsltResolveAttrSet
    fn resolve_attr_set(
        &mut self,
        set: &mut AttrSet,
        top: usize,
        style: usize,
        name: &str,
        ns: Option<&str>,
        depth: i32,
    ) {
        if set.state == 2 {
            return;
        }
        if set.state == 1 {
            self.err(
                Some(top),
                None,
                &format!(
                    "xsl:attribute-set : use-attribute-sets recursion detected on {}\n",
                    name
                ),
            );
            self.sheet.styles[top].errors += 1;
            set.state = 2;
            return;
        }
        if depth > 100 {
            self.err(
                Some(top),
                None,
                &format!(
                    "xsl:attribute-set : use-attribute-sets maximum recursion depth exceeded on {}\n",
                    name
                ),
            );
            self.sheet.styles[top].errors += 1;
            return;
        }
        set.state = 1;
        self.resolve_use_attr_sets(set, top, depth);
        let mut cur = self.sheet.next_import(style);
        while let Some(c) = cur {
            let key = (name.to_string(), ns.map(|x| x.to_string()));
            if let Some(mut other) = self.sheet.styles[c].attribute_sets.remove(&key) {
                self.resolve_use_attr_sets(&mut other, top, depth);
                self.merge_attr_sets(set, &other);
            }
            cur = self.sheet.next_import(c);
        }
        set.state = 2;
    }

    /// xsltResolveStylesheetAttributeSet
    fn resolve_stylesheet_attribute_set(&mut self, top: usize) {
        let mut cur = Some(top);
        while let Some(c) = cur {
            let mut keys: Vec<(String, Option<String>)> = self.sheet.styles[c]
                .attribute_sets
                .keys()
                .cloned()
                .collect();
            keys.sort();
            for key in keys {
                let mut set = match self.sheet.styles[c].attribute_sets.remove(&key) {
                    Some(x) => x,
                    None => continue,
                };
                self.sheet.styles[c]
                    .attribute_sets
                    .insert(key.clone(), placeholder(&set));
                self.resolve_attr_set(&mut set, top, c, &key.0, key.1.as_deref(), 1);
                if c != top {
                    self.sheet.styles[c].attribute_sets.remove(&key);
                }
                if c == top {
                    self.sheet.styles[c]
                        .attribute_sets
                        .insert(key.clone(), set.clone());
                }
                self.sheet.attr_sets.entry(key).or_insert(set);
            }
            if c != top {
                self.sheet.styles[c].attribute_sets.clear();
            }
            cur = self.sheet.next_import(c);
        }
    }

    /// xsltParseGlobalVariable / xsltParseGlobalParam
    fn parse_global_variable(&mut self, s: usize, cur: NodeId, param: bool) {
        self.style_pre_compute(s, cur);
        let comp = match self.sheet.precomp.get(&cur).cloned() {
            Some(c) => c,
            None => {
                let m = if param {
                    "xsl:param : compilation failed\n"
                } else {
                    "xsl:variable : compilation failed\n"
                };
                self.err(Some(s), Some(cur), m);
                return;
            }
        };
        let name = match &comp.name {
            Some(n) => n.clone(),
            None => {
                let m = if param {
                    "xsl:param : missing name attribute\n"
                } else {
                    "xsl:variable : missing name attribute\n"
                };
                self.err(Some(s), Some(cur), m);
                return;
            }
        };
        if tw(|t| t.nodes[cur].children.is_some()) {
            self.parse_template_content(s, cur);
        }
        let tree = tw(|t| t.nodes[cur].children);
        // xsltRegisterGlobalVariable
        if comp.kind == Kind::Variable {
            let dup = self.sheet.styles[s]
                .variables
                .iter()
                .any(|v| v.comp.kind == Kind::Variable && v.name == name && v.name_uri == comp.ns);
            if dup {
                self.style_error(
                    s,
                    Some(comp.inst),
                    &format!("redefinition of global variable {}\n", name),
                );
            }
        }
        self.sheet.styles[s].variables.push(GlobalDef {
            name,
            name_uri: comp.ns.clone(),
            select: comp.select.clone(),
            tree,
            comp,
        });
    }

    /// xsltGetInheritedNsList
    fn inherited_ns_list(&self, s: usize, node: NodeId) -> Vec<(Option<String>, String)> {
        let excl = self.sheet.styles[s].excl_prefix_tab.clone();
        let defs: Vec<(Option<String>, String)> = tw(|t| {
            let mut v = Vec::new();
            let mut cur = Some(node);
            while let Some(c) = cur {
                if t.nodes[c].t == NType::Element {
                    for &ns in &t.nodes[c].ns_def {
                        v.push((t.nss[ns].prefix.clone(), t.nss[ns].href.clone()));
                    }
                }
                cur = t.nodes[c].parent;
            }
            v
        });
        let mut out: Vec<(Option<String>, String)> = Vec::new();
        for (p, h) in defs {
            if h == XSLT_NAMESPACE {
                continue;
            }
            if p.is_some() && self.sheet.check_ext_prefix(s, p.as_deref()) {
                continue;
            }
            if excl.contains(&h) {
                continue;
            }
            if out.iter().any(|(op, _)| *op == p) {
                continue;
            }
            out.push((p, h));
        }
        out
    }

    /// xsltParseStylesheetTemplate
    fn parse_template(&mut self, s: usize, template: NodeId) {
        let ti = self.sheet.templates.len();
        let position = self.sheet.styles[s].templates.len() as i32;
        let inherited = self.inherited_ns_list(s, template);
        self.sheet.templates.push(Template {
            style: s,
            match_: None,
            priority: XSLT_PAT_NO_PRIORITY,
            name: None,
            name_uri: None,
            mode: None,
            mode_uri: None,
            content: None,
            elem: template,
            inherited_ns: inherited,
            position,
        });
        self.sheet.styles[s].templates.push(ti);
        let get = |name: &str| tw(|t| get_ns_prop(t, template, name, None));
        if let Some(prop) = get("mode") {
            match get_qname_uri(self.vm, template, &prop) {
                Err(()) => {
                    self.sheet.styles[s].errors += 1;
                    return;
                }
                Ok((uri, local)) => {
                    self.sheet.templates[ti].mode = Some(local);
                    self.sheet.templates[ti].mode_uri = uri;
                }
            }
        }
        if let Some(m) = get("match") {
            self.sheet.templates[ti].match_ = Some(m);
        }
        if let Some(p) = get("priority") {
            self.sheet.templates[ti].priority = xpath::string_eval_number(&p) as f32;
        }
        if let Some(prop) = get("name") {
            match get_qname_uri(self.vm, template, &prop) {
                Err(()) => {
                    self.sheet.styles[s].errors += 1;
                    return;
                }
                Ok((uri, local)) => {
                    if !valid_ncname(&local) {
                        self.style_error(
                            s,
                            Some(template),
                            &format!("xsl:template : error invalid name '{}'\n", local),
                        );
                        return;
                    }
                    self.sheet.templates[ti].name = Some(local);
                    self.sheet.templates[ti].name_uri = uri;
                }
            }
        }
        self.parse_template_content(s, template);
        let content = tw(|t| t.nodes[template].children);
        self.sheet.templates[ti].content = content;
        self.add_template(s, ti);
    }

    /// xsltAddTemplate
    fn add_template(&mut self, s: usize, ti: usize) {
        let (name, name_uri, match_, elem, mode, mode_uri, priority) = {
            let t = &self.sheet.templates[ti];
            (
                t.name.clone(),
                t.name_uri.clone(),
                t.match_.clone(),
                t.elem,
                t.mode.clone(),
                t.mode_uri.clone(),
                t.priority,
            )
        };
        if let Some(n) = &name {
            let key = (n.clone(), name_uri.clone());
            if self.sheet.styles[s].named_templates.contains_key(&key) {
                self.style_error(
                    s,
                    Some(elem),
                    &format!("xsl:template: error duplicate name '{}'\n", n),
                );
                return;
            }
            self.sheet.styles[s].named_templates.insert(key, ti);
        }
        let m = match match_ {
            None => {
                if name.is_none() {
                    self.style_error(
                        s,
                        Some(elem),
                        "xsl:template: need to specify match or name attribute\n",
                    );
                }
                return;
            }
            Some(m) => m,
        };
        let doc = self.sheet.styles[s].doc;
        let pats = match self.compile_pattern(&m, doc, elem, Some(s), true) {
            Some(p) => p,
            None => return,
        };
        for pi in pats {
            {
                let p = &mut self.sheet.pats[pi];
                p.template = Some(ti);
                p.mode = mode.clone();
                p.mode_uri = mode_uri.clone();
                if priority != XSLT_PAT_NO_PRIORITY {
                    p.priority = priority;
                }
            }
            let (op, value) = {
                let p = &self.sheet.pats[pi];
                (p.steps[0].op, p.steps[0].value.clone())
            };
            let mut name: Option<String> = None;
            let top: Option<u8> = match op {
                POp::Attr => {
                    if value.is_some() {
                        name = value;
                        None
                    } else {
                        Some(3)
                    }
                }
                POp::Parent | POp::Ancestor => Some(2),
                POp::Root => Some(0),
                POp::Key => Some(1),
                POp::Id | POp::Ns | POp::All => Some(2),
                POp::End | POp::Predicate => {
                    self.style_error_noctx(s, "xsltAddTemplate: invalid compiled pattern\n");
                    return;
                }
                POp::Pi => {
                    if value.is_some() {
                        name = value;
                        None
                    } else {
                        Some(5)
                    }
                }
                POp::Comment => Some(6),
                POp::Text => Some(4),
                POp::Elem | POp::Node => {
                    if value.is_some() {
                        name = value;
                        None
                    } else {
                        Some(2)
                    }
                }
            };
            let pprio = self.sheet.pats[pi].priority;
            let pats = &self.sheet.pats;
            let insert = |list: &mut Vec<usize>| {
                // sorted by decreasing priority; equal priority: later first
                let pos = list
                    .iter()
                    .position(|&x| pats[x].priority <= pprio)
                    .unwrap_or(list.len());
                list.insert(pos, pi);
            };
            let st = &mut self.sheet.styles[s];
            if let Some(n) = name {
                let list = st
                    .templates_hash
                    .entry((n, mode.clone(), mode_uri.clone()))
                    .or_default();
                insert(list);
            } else if let Some(tp) = top {
                let list = match tp {
                    0 => &mut st.root_match,
                    1 => &mut st.key_match,
                    2 => &mut st.elem_match,
                    3 => &mut st.attr_match,
                    4 => &mut st.text_match,
                    5 => &mut st.pi_match,
                    _ => &mut st.comment_match,
                };
                insert(list);
            }
        }
    }

    fn style_error_noctx(&mut self, s: usize, msg: &str) {
        self.err(Some(s), None, msg);
        self.sheet.styles[s].errors += 1;
    }

    /// xsltNamespaceAlias
    fn namespace_alias(&mut self, s: usize, node: NodeId) {
        let get = |name: &str| tw(|t| get_ns_prop(t, node, name, None));
        let style_prefix = match get("stylesheet-prefix") {
            Some(p) => p,
            None => {
                self.err(
                    Some(s),
                    Some(node),
                    "namespace-alias: stylesheet-prefix attribute missing\n",
                );
                return;
            }
        };
        let result_prefix = match get("result-prefix") {
            Some(p) => p,
            None => {
                self.err(
                    Some(s),
                    Some(node),
                    "namespace-alias: result-prefix attribute missing\n",
                );
                return;
            }
        };
        let literal_ns_name: Option<String>;
        if style_prefix == "#default" {
            literal_ns_name = tw(|t| t.search_ns(node, None).map(|n| t.nss[n].href.clone()));
        } else {
            match tw(|t| {
                t.search_ns(node, Some(&style_prefix))
                    .map(|n| t.nss[n].href.clone())
            }) {
                None => {
                    self.err(
                        Some(s),
                        Some(node),
                        &format!(
                            "namespace-alias: prefix {} not bound to any namespace\n",
                            style_prefix
                        ),
                    );
                    return;
                }
                Some(h) => literal_ns_name = Some(h),
            }
        }
        let target: Option<String>;
        let target_ns_exists;
        if result_prefix == "#default" {
            let t = tw(|t| t.search_ns(node, None).map(|n| t.nss[n].href.clone()));
            target_ns_exists = t.is_some();
            target = t;
        } else {
            match tw(|t| {
                t.search_ns(node, Some(&result_prefix))
                    .map(|n| t.nss[n].href.clone())
            }) {
                None => {
                    self.err(
                        Some(s),
                        Some(node),
                        &format!(
                            "namespace-alias: prefix {} not bound to any namespace\n",
                            result_prefix
                        ),
                    );
                    return;
                }
                Some(h) => {
                    target_ns_exists = true;
                    target = Some(h);
                }
            }
        }
        match literal_ns_name {
            None => {
                if target_ns_exists {
                    self.sheet.styles[s].default_alias = target;
                }
            }
            Some(l) => {
                let st = &mut self.sheet.styles[s];
                if !st.ns_aliases.iter().any(|(k, _)| *k == l) {
                    st.ns_aliases.push((l, target));
                }
            }
        }
    }

    /// xsltParseTemplateContent
    pub fn parse_template_content(&mut self, s: usize, templ: NodeId) {
        if tw(|t| t.nodes[templ].t == NType::NamespaceDecl) {
            return;
        }
        let mut cur = tw(|t| t.nodes[templ].children);
        let mut delete: Option<NodeId> = None;
        while let Some(c) = cur {
            if let Some(d) = delete.take() {
                tw(|t| t.unlink(d));
            }
            let mut skip_children = false;
            let (is_x, ctype) = tw(|t| (is_xslt_elem(t, c), t.nodes[c].t));
            if is_x {
                self.style_pre_compute(s, c);
                if tw(|t| is_xslt_name(t, c, "text")) {
                    if tw(|t| t.nodes[c].children.is_some()) {
                        let prop = tw(|t| get_ns_prop(t, c, "disable-output-escaping", None));
                        let mut noesc = false;
                        if let Some(p) = prop {
                            if p == "yes" {
                                noesc = true;
                            } else if p != "no" {
                                self.style_warning(
                                    s,
                                    Some(c),
                                    "xsl:text: disable-output-escaping allows only yes or no\n",
                                );
                            }
                        }
                        let kids = tw(|t| t.children_of(c));
                        let mut ok = true;
                        for k in &kids {
                            let kt = tw(|t| t.nodes[*k].t);
                            if kt == NType::Comment {
                                continue;
                            }
                            if kt != NType::Text && kt != NType::CData {
                                self.style_error(
                                    s,
                                    Some(c),
                                    "xsltParseTemplateContent: xslt:text content problem\n",
                                );
                                ok = false;
                                break;
                            }
                            if noesc && kt != NType::CData {
                                tw(|t| {
                                    t.nodes[*k].name = "textnoenc".into();
                                    t.nodes[*k].flags |= 1;
                                });
                            }
                        }
                        if ok {
                            for k in kids {
                                tw(|t| {
                                    t.unlink(k);
                                    t.insert_before_raw(c, k);
                                });
                            }
                        }
                    }
                    delete = Some(c);
                    skip_children = true;
                }
            } else if ctype == NType::Element
                && tw(|t| t.nodes[c].ns.is_some())
                && !self.sheet.styles[s].ns_defs.is_empty()
                && {
                    let p = tw(|t| t.ns_prefix(t.nodes[c].ns).map(|x| x.to_string()));
                    self.sheet.check_ext_prefix(s, p.as_deref())
                }
            {
                self.style_pre_compute(s, c);
            } else if ctype == NType::Element {
                if let Some(alias) = self.sheet.styles[s].default_alias.clone() {
                    tw(|t| {
                        if t.nodes[c].ns.is_none() {
                            let ns = t.search_ns_by_href(c, &alias);
                            t.nodes[c].ns = ns;
                        }
                    });
                }
                let attrs = tw(|t| t.attrs_of(c));
                for a in attrs {
                    self.compile_attr(s, a);
                }
            }
            if !skip_children {
                let ch = tw(|t| {
                    t.nodes[c]
                        .children
                        .filter(|&x| t.nodes[x].t != NType::EntityDecl)
                });
                if let Some(ch) = ch {
                    cur = Some(ch);
                    continue;
                }
            }
            cur = tw(|t| {
                if let Some(n) = t.nodes[c].next {
                    return Some(n);
                }
                let mut x = c;
                loop {
                    x = t.nodes[x].parent?;
                    if x == templ {
                        return None;
                    }
                    if let Some(n) = t.nodes[x].next {
                        return Some(n);
                    }
                }
            });
        }
        if let Some(d) = delete {
            tw(|t| t.unlink(d));
        }
        // skip the first params, remove misplaced ones
        let kids = tw(|t| t.children_of(templ));
        let mut i = 0;
        while i < kids.len() {
            let k = kids[i];
            if tw(|t| is_xslt_elem(t, k) && !is_xslt_name(t, k, "param")) {
                break;
            }
            i += 1;
        }
        while i < kids.len() {
            let k = kids[i];
            if tw(|t| is_xslt_elem_named(t, k, "param")) {
                self.style_warning(
                    s,
                    Some(k),
                    "xsltParseTemplateContent: ignoring misplaced param element\n",
                );
                tw(|t| t.unlink(k));
                i += 1;
            } else {
                break;
            }
        }
    }

    /// xsltCompileAttr
    fn compile_attr(&mut self, s: usize, attr: NodeId) {
        let (children_ok, str_, name, parent) = tw(|t| {
            let a = &t.nodes[attr];
            let ok = match a.children {
                None => None,
                Some(c) => Some(t.nodes[c].t == NType::Text && t.nodes[c].next.is_none()),
            };
            let s = a
                .children
                .map(|c| t.nodes[c].content.clone())
                .unwrap_or_default();
            (ok, s, a.name.clone(), a.parent)
        });
        match children_ok {
            None => return,
            Some(false) => {
                self.style_error(
                    s,
                    parent,
                    &format!(
                        "Attribute '{}': The content is expected to be a single text node when compiling an AVT.\n",
                        name
                    ),
                );
                return;
            }
            Some(true) => {}
        }
        if !str_.contains('{') && !str_.contains('}') {
            return;
        }
        if self.sheet.avts.contains_key(&attr) {
            return;
        }
        let ns_list = Rc::new(tw(|t| {
            parent.map(|p| xpath_ns_list(t, p)).unwrap_or_default()
        }));
        let mut avt = Avt {
            strstart: false,
            segments: Vec::new(),
            ns_list,
        };
        let b = str_.as_bytes();
        let mut ret: Option<String> = None;
        let mut start = 0usize;
        let mut cur = 0usize;
        let mut lastavt = false;
        let mut failed = false;
        while cur < b.len() {
            if b[cur] == b'{' {
                if cur + 1 < b.len() && b[cur + 1] == b'{' {
                    cur += 1;
                    ret.get_or_insert_with(String::new)
                        .push_str(&str_[start..cur]);
                    cur += 1;
                    start = cur;
                    continue;
                }
                if cur + 1 < b.len() && b[cur + 1] == b'}' {
                    ret.get_or_insert_with(String::new)
                        .push_str(&str_[start..cur]);
                    cur += 2;
                    start = cur;
                    continue;
                }
                if ret.is_some() || cur > start {
                    let mut r = ret.take().unwrap_or_default();
                    r.push_str(&str_[start..cur]);
                    start = cur;
                    if avt.segments.is_empty() {
                        avt.strstart = true;
                    }
                    avt.segments.push(AvtSeg::Str(r));
                    lastavt = false;
                }
                cur += 1;
                while cur < b.len() && b[cur] != b'}' {
                    if b[cur] == b'\'' || b[cur] == b'"' {
                        let delim = b[cur];
                        cur += 1;
                        while cur < b.len() && b[cur] != delim {
                            cur += 1;
                        }
                        if cur < b.len() {
                            cur += 1;
                        }
                    } else {
                        cur += 1;
                    }
                }
                if cur >= b.len() {
                    self.style_error(
                        s,
                        parent,
                        &format!("Attribute '{}': The AVT has an unmatched '{{'.\n", name),
                    );
                    failed = true;
                    break;
                }
                start += 1;
                let expr = &str_[start..cur];
                match self.xpath_compile(expr) {
                    None => {
                        self.style_error(
                            s,
                            parent,
                            &format!(
                                "Attribute '{}': Failed to compile the expression '{}' in the AVT.\n",
                                name, expr
                            ),
                        );
                        failed = true;
                        break;
                    }
                    Some(comp) => {
                        if avt.segments.is_empty() {
                            avt.strstart = false;
                        }
                        if lastavt {
                            avt.segments.push(AvtSeg::Str(String::new()));
                        }
                        avt.segments.push(AvtSeg::Expr(Some(comp)));
                        lastavt = true;
                    }
                }
                cur += 1;
                start = cur;
            } else if b[cur] == b'}' {
                cur += 1;
                if cur < b.len() && b[cur] == b'}' {
                    ret.get_or_insert_with(String::new)
                        .push_str(&str_[start..cur]);
                    cur += 1;
                    start = cur;
                    continue;
                } else {
                    self.err(
                        Some(s),
                        parent,
                        &format!("Attribute '{}': The AVT has an unmatched '}}'.\n", name),
                    );
                    failed = true;
                    break;
                }
            } else {
                cur += 1;
            }
        }
        if !failed && (ret.is_some() || cur > start) {
            let mut r = ret.take().unwrap_or_default();
            r.push_str(&str_[start.min(b.len())..cur.min(b.len())]);
            if avt.segments.is_empty() {
                avt.strstart = true;
            }
            avt.segments.push(AvtSeg::Str(r));
        }
        self.sheet.avts.insert(attr, Rc::new(avt));
    }

    // ------------------------------------------------------------------
    // preproc.c

    fn check_top_level_element(&mut self, s: usize, inst: NodeId, err: bool) -> bool {
        let (parent_ok, name) = tw(|t| {
            let nd = &t.nodes[inst];
            let ok = match nd.parent {
                None => None,
                Some(p) => {
                    let pn = &t.nodes[p];
                    Some(
                        !(pn.ns.is_none()
                            || pn.t != NType::Element
                            || (pn.ns != nd.ns && t.ns_href(pn.ns) != t.ns_href(nd.ns))
                            || (pn.name != "stylesheet" && pn.name != "transform")),
                    )
                }
            };
            (ok, nd.name.clone())
        });
        match parent_ok {
            None => {
                if err {
                    self.style_error(s, Some(inst), "internal problem: element has no parent\n");
                }
                false
            }
            Some(false) => {
                if err {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!("element {} only allowed as child of stylesheet\n", name),
                    );
                }
                false
            }
            Some(true) => true,
        }
    }

    fn check_instruction_element(&mut self, s: usize, inst: NodeId) {
        if self.sheet.styles[s].literal_result {
            return;
        }
        let has_ext = !self.sheet.styles[s].ext_infos.is_empty();
        let ext_infos = self.sheet.styles[s].ext_infos.clone();
        let r = tw(|t| {
            let nd = &t.nodes[inst];
            let mut parent = match nd.parent {
                None => return Err(()),
                Some(p) => Some(p),
            };
            let inst_href = t.ns_href(nd.ns).map(|s| s.to_string());
            while let Some(p) = parent {
                let pn = &t.nodes[p];
                if pn.t == NType::Document {
                    break;
                }
                if (pn.ns == nd.ns
                    || (pn.ns.is_some() && t.ns_href(pn.ns).map(|s| s.to_string()) == inst_href))
                    && matches!(
                        pn.name.as_str(),
                        "template" | "param" | "attribute" | "variable"
                    )
                {
                    return Ok(true);
                }
                if has_ext && pn.ns.is_some() {
                    if let Some(h) = t.ns_href(pn.ns) {
                        if ext_infos.contains(h) {
                            return Ok(true);
                        }
                    }
                }
                parent = pn.parent;
            }
            Ok(false)
        });
        match r {
            Err(()) => self.style_error(s, Some(inst), "internal problem: element has no parent\n"),
            Ok(true) => {}
            Ok(false) => {
                let name = tw(|t| t.nodes[inst].name.clone());
                self.style_error(
                    s,
                    Some(inst),
                    &format!(
                        "element {} only allowed within a template, variable or param\n",
                        name
                    ),
                );
            }
        }
    }

    fn check_parent_element(&mut self, s: usize, inst: NodeId, allow1: &str, allow2: Option<&str>) {
        if self.sheet.styles[s].literal_result {
            return;
        }
        let ext_infos = self.sheet.styles[s].ext_infos.clone();
        let r = tw(|t| {
            let nd = &t.nodes[inst];
            let p = match nd.parent {
                None => return Err(()),
                Some(p) => p,
            };
            let pn = &t.nodes[p];
            if (pn.ns == nd.ns || (pn.ns.is_some() && t.ns_href(pn.ns) == t.ns_href(nd.ns)))
                && (pn.name == allow1 || Some(pn.name.as_str()) == allow2)
            {
                return Ok(true);
            }
            if !ext_infos.is_empty() {
                let mut parent = Some(p);
                while let Some(q) = parent {
                    let qn = &t.nodes[q];
                    if qn.t == NType::Document {
                        break;
                    }
                    if let Some(h) = t.ns_href(qn.ns) {
                        if ext_infos.contains(h) {
                            return Ok(true);
                        }
                    }
                    parent = qn.parent;
                }
            }
            Ok(false)
        });
        match r {
            Err(()) => self.style_error(s, Some(inst), "internal problem: element has no parent\n"),
            Ok(true) => {}
            Ok(false) => {
                let name = tw(|t| t.nodes[inst].name.clone());
                self.style_error(
                    s,
                    Some(inst),
                    &format!("element {} is not allowed within that context\n", name),
                );
            }
        }
    }

    /// xsltEvalStaticAttrValueTemplate: (value if no '{', found)
    fn static_avt(&self, inst: NodeId, name: &str, ns: Option<&str>) -> (Option<String>, bool) {
        match tw(|t| xslt_ns_prop(t, inst, name, ns)) {
            None => (None, false),
            Some(v) => {
                if v.contains('{') {
                    (None, true)
                } else {
                    (Some(v), true)
                }
            }
        }
    }

    fn cns_prop(&self, inst: NodeId, name: &str) -> Option<String> {
        tw(|t| xslt_ns_prop(t, inst, name, Some(XSLT_NAMESPACE)))
    }

    /// xsltGetQNameProperty: (has_prop, ns, local)
    fn qname_property(
        &mut self,
        s: usize,
        inst: NodeId,
        prop_name: &str,
        mandatory: bool,
    ) -> (bool, Option<String>, Option<String>) {
        let prop = self.cns_prop(inst, prop_name);
        match prop {
            None => {
                if mandatory {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!("The attribute '{}' is missing.\n", prop_name),
                    );
                }
                (false, None, None)
            }
            Some(p) => {
                if !valid_qname(&p) {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!(
                            "The value '{}' of the attribute '{}' is not a valid QName.\n",
                            p, prop_name
                        ),
                    );
                    return (false, None, None);
                }
                // xsltGetQNameURI2
                if let Some(i) = p.find(':') {
                    if let Some(rest) = p.strip_prefix("xml:") {
                        if !rest.is_empty() {
                            return (
                                true,
                                Some(XML_XML_NAMESPACE.to_string()),
                                Some(rest.to_string()),
                            );
                        }
                    }
                    let prefix = &p[..i];
                    match tw(|t| {
                        t.search_ns(inst, Some(prefix))
                            .map(|n| t.nss[n].href.clone())
                    }) {
                        None => {
                            self.style_error(
                                s,
                                Some(inst),
                                &format!("No namespace bound to prefix '{}'.\n", prefix),
                            );
                            (false, None, None)
                        }
                        Some(h) => (true, Some(h), Some(p[i + 1..].to_string())),
                    }
                } else {
                    (true, None, Some(p))
                }
            }
        }
    }

    /// xsltStylePreCompute
    pub fn style_pre_compute(&mut self, s: usize, inst: NodeId) {
        if tw(|t| t.nodes[inst].t != NType::Element) {
            return;
        }
        if self.sheet.precomp.contains_key(&inst) || self.sheet.ext_marker.contains(&inst) {
            return;
        }
        let (is_x, name) = tw(|t| (is_xslt_elem(t, inst), t.nodes[inst].name.clone()));
        if !is_x {
            // xsltPreComputeExtModuleElement
            let uri = tw(|t| t.ns_href(t.nodes[inst].ns).map(|s| s.to_string()));
            if let Some(u) = uri {
                if ext_module_element(&name, &u) {
                    let mut comp = if (u == EXSLT_COMMON_NAMESPACE && name == "document")
                        || (u == XSLT_XT_NAMESPACE && name == "document")
                        || (u == XSLT_SAXON_NAMESPACE && name == "output")
                        || (u == XSLT_XALAN_NAMESPACE && name == "write")
                    {
                        self.document_comp(inst)
                    } else if u == EXSLT_FUNCTIONS_NAMESPACE && name == "result" {
                        crate::exslt::func::func_result_comp(self, s, inst).unwrap_or(PreComp {
                            kind: Kind::Extension,
                            inst,
                            ..Default::default()
                        })
                    } else {
                        PreComp {
                            kind: Kind::Extension,
                            inst,
                            ..Default::default()
                        }
                    };
                    if comp.kind == Kind::Document {
                        comp.kind = Kind::Document;
                    }
                    comp.ext = Some((u, name));
                    self.sheet.precomp.insert(inst, Rc::new(comp));
                    return;
                }
            }
            self.sheet.ext_marker.insert(inst);
            return;
        }
        let comp: Option<PreComp> = match name.as_str() {
            "apply-templates" => {
                self.check_instruction_element(s, inst);
                Some(self.apply_templates_comp(s, inst))
            }
            "with-param" => {
                self.check_parent_element(s, inst, "apply-templates", Some("call-template"));
                Some(self.with_param_comp(s, inst, Kind::WithParam))
            }
            "value-of" => {
                self.check_instruction_element(s, inst);
                Some(self.value_of_comp(s, inst))
            }
            "copy" => {
                self.check_instruction_element(s, inst);
                let u = self.cns_prop(inst, "use-attribute-sets");
                Some(PreComp {
                    kind: Kind::Copy,
                    inst,
                    has_use: u.is_some(),
                    use_: u,
                    ..Default::default()
                })
            }
            "copy-of" => {
                self.check_instruction_element(s, inst);
                Some(self.select_comp(s, inst, Kind::CopyOf, "xsl:copy-of"))
            }
            "if" => {
                self.check_instruction_element(s, inst);
                Some(self.test_comp(s, inst, Kind::If, "xsl:if"))
            }
            "when" => {
                self.check_parent_element(s, inst, "choose", None);
                Some(self.test_comp(s, inst, Kind::When, "xsl:when"))
            }
            "choose" => {
                self.check_instruction_element(s, inst);
                Some(PreComp {
                    kind: Kind::Choose,
                    inst,
                    ..Default::default()
                })
            }
            "for-each" => {
                self.check_instruction_element(s, inst);
                Some(self.for_each_comp(s, inst))
            }
            "apply-imports" => {
                self.check_instruction_element(s, inst);
                Some(PreComp {
                    kind: Kind::ApplyImports,
                    inst,
                    ..Default::default()
                })
            }
            "attribute" => {
                let in_set = tw(|t| {
                    let nd = &t.nodes[inst];
                    match nd.parent {
                        Some(p) => {
                            let pn = &t.nodes[p];
                            pn.t == NType::Element
                                && pn.ns.is_some()
                                && (pn.ns == nd.ns || t.ns_href(pn.ns) == t.ns_href(nd.ns))
                                && pn.name == "attribute-set"
                        }
                        None => false,
                    }
                });
                if !in_set {
                    self.check_instruction_element(s, inst);
                }
                Some(self.attribute_comp(s, inst))
            }
            "element" => {
                self.check_instruction_element(s, inst);
                Some(self.element_comp(s, inst))
            }
            "text" => {
                self.check_instruction_element(s, inst);
                let mut c = PreComp {
                    kind: Kind::Text,
                    inst,
                    ..Default::default()
                };
                if let Some(p) = self.cns_prop(inst, "disable-output-escaping") {
                    if p == "yes" {
                        c.noescape = true;
                    } else if p != "no" {
                        self.style_warning(
                            s,
                            Some(inst),
                            "xsl:text: disable-output-escaping allows only yes or no\n",
                        );
                    }
                }
                Some(c)
            }
            "sort" => {
                self.check_parent_element(s, inst, "apply-templates", Some("for-each"));
                Some(self.sort_comp(s, inst))
            }
            "comment" => {
                self.check_instruction_element(s, inst);
                Some(PreComp {
                    kind: Kind::Comment,
                    inst,
                    ..Default::default()
                })
            }
            "number" => {
                self.check_instruction_element(s, inst);
                Some(self.number_comp(s, inst))
            }
            "processing-instruction" => {
                self.check_instruction_element(s, inst);
                let (n, has) = self.static_avt(inst, "name", Some(XSLT_NAMESPACE));
                Some(PreComp {
                    kind: Kind::Pi,
                    inst,
                    name: n,
                    has_name: has,
                    ..Default::default()
                })
            }
            "call-template" => {
                self.check_instruction_element(s, inst);
                let (has, ns, n) = self.qname_property(s, inst, "name", true);
                Some(PreComp {
                    kind: Kind::CallTemplate,
                    inst,
                    has_name: has,
                    has_ns: ns.is_some(),
                    ns,
                    name: n,
                    ..Default::default()
                })
            }
            "param" => {
                if !self.check_top_level_element(s, inst, false) {
                    self.check_instruction_element(s, inst);
                }
                Some(self.with_param_comp(s, inst, Kind::Param))
            }
            "variable" => {
                if !self.check_top_level_element(s, inst, false) {
                    self.check_instruction_element(s, inst);
                }
                Some(self.with_param_comp(s, inst, Kind::Variable))
            }
            "otherwise" => {
                self.check_parent_element(s, inst, "choose", None);
                self.check_instruction_element(s, inst);
                None
            }
            "template" | "output" | "preserve-space" | "strip-space" | "key" | "attribute-set"
            | "namespace-alias" | "include" | "import" | "decimal-format" => {
                self.check_top_level_element(s, inst, true);
                None
            }
            "stylesheet" | "transform" => {
                let ok = tw(|t| {
                    t.nodes[inst]
                        .parent
                        .map(|p| t.nodes[p].t == NType::Document)
                        .unwrap_or(false)
                });
                if !ok {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!("element {} only allowed only as root element\n", name),
                    );
                }
                None
            }
            "message" | "fallback" => {
                self.check_instruction_element(s, inst);
                None
            }
            "document" => {
                self.check_instruction_element(s, inst);
                let mut c = self.document_comp(inst);
                c.ext = Some((XSLT_NAMESPACE.to_string(), "document".to_string()));
                Some(c)
            }
            _ => {
                if !self.sheet.styles[s].forwards_compatible {
                    self.style_warning(
                        s,
                        Some(inst),
                        &format!("xsltStylePreCompute: unknown xsl:{}\n", name),
                    );
                }
                None
            }
        };
        if let Some(mut c) = comp {
            c.ns_list = Rc::new(tw(|t| xpath_ns_list(t, inst)));
            self.sheet.precomp.insert(inst, Rc::new(c));
        }
    }

    /// xsltDocumentComp
    fn document_comp(&mut self, inst: NodeId) -> PreComp {
        let mut comp = PreComp {
            kind: Kind::Document,
            inst,
            ..Default::default()
        };
        let (name, href) = tw(|t| {
            (
                t.nodes[inst].name.clone(),
                t.ns_href(t.nodes[inst].ns).map(|s| s.to_string()),
            )
        });
        if name == "output" {
            let (f, has) = self.static_avt(inst, "file", None);
            comp.filename = f;
            comp.has_filename = has;
        } else if name == "write" {
        } else if name == "document" {
            if href.as_deref() == Some(XSLT_NAMESPACE) {
                comp.ver11 = true;
            }
            let (f, has) = self.static_avt(inst, "href", None);
            comp.filename = f;
            comp.has_filename = has;
        }
        if !comp.has_filename {
            comp.filename = None;
        }
        comp
    }

    fn sort_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::Sort,
            inst,
            ..Default::default()
        };
        let (v, h) = self.static_avt(inst, "data-type", None);
        c.stype = v;
        c.has_stype = h;
        if let Some(st) = &c.stype {
            if st == "text" {
                c.number = false;
            } else if st == "number" {
                c.number = true;
            } else {
                let m = format!("xsltSortComp: no support for data-type = {}\n", st);
                self.style_warning(s, Some(inst), &m);
                c.number = false;
            }
        }
        let (v, h) = self.static_avt(inst, "order", None);
        c.order = v;
        c.has_order = h;
        if let Some(o) = &c.order {
            if o == "ascending" {
                c.descending = false;
            } else if o == "descending" {
                c.descending = true;
            } else {
                let m = format!("xsltSortComp: invalid value {} for order\n", o);
                self.style_warning(s, Some(inst), &m);
                c.descending = false;
            }
        }
        let (v, h) = self.static_avt(inst, "case-order", None);
        c.case_order = v;
        c.has_use = h;
        if let Some(o) = &c.case_order {
            if o == "upper-first" {
                c.lower_first = false;
            } else if o == "lower-first" {
                c.lower_first = true;
            } else {
                let m = format!(
                    "xsltSortComp: invalid value {} for order\n",
                    c.order.clone().unwrap_or_else(|| "(null)".into())
                );
                self.style_warning(s, Some(inst), &m);
                c.lower_first = false;
            }
        }
        let (v, h) = self.static_avt(inst, "lang", None);
        c.lang = v;
        c.has_lang = h;
        c.select = Some(self.cns_prop(inst, "select").unwrap_or_else(|| ".".into()));
        let sel = c.select.clone().unwrap();
        c.comp = self.xpath_compile(&sel);
        if c.comp.is_none() {
            self.style_error(
                s,
                Some(inst),
                &format!(
                    "xsltSortComp: could not compile select expression '{}'\n",
                    sel
                ),
            );
        }
        if tw(|t| t.nodes[inst].children.is_some()) {
            self.style_error(s, Some(inst), "xsl:sort : is not empty\n");
        }
        c
    }

    fn element_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::Element,
            inst,
            ..Default::default()
        };
        let (n, has) = self.static_avt(inst, "name", None);
        c.name = n;
        c.has_name = has;
        if !has {
            self.style_error(
                s,
                Some(inst),
                "xsl:element: The attribute 'name' is missing.\n",
            );
            return c;
        }
        let (ns, has_ns) = self.static_avt(inst, "namespace", None);
        c.ns = ns;
        c.has_ns = has_ns;
        if let Some(name) = c.name.clone() {
            if !valid_qname(&name) {
                self.style_error(
                    s,
                    Some(inst),
                    &format!(
                        "xsl:element: The value '{}' of the attribute 'name' is not a valid QName.\n",
                        name
                    ),
                );
            } else {
                let (prefix, _local) = split_qname(&name);
                if !c.has_ns {
                    let ns = tw(|t| {
                        t.search_ns(inst, prefix.as_deref())
                            .map(|n| t.nss[n].href.clone())
                    });
                    if let Some(h) = ns {
                        c.ns = Some(h);
                        c.has_ns = true;
                    } else if prefix.is_some() {
                        self.style_error(
                            s,
                            Some(inst),
                            &format!(
                                "xsl:element: The prefixed QName '{}' has no namespace binding in scope in the stylesheet; this is an error, since the namespace was not specified by the instruction itself.\n",
                                name
                            ),
                        );
                    }
                }
                if let Some(p) = &prefix {
                    if p.len() >= 3 && p[..3].eq_ignore_ascii_case("xml") {
                        c.has_name = false;
                    }
                }
            }
        }
        let (u, hu) = self.static_avt(inst, "use-attribute-sets", None);
        c.use_ = u;
        c.has_use = hu;
        c
    }

    fn attribute_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::Attribute,
            inst,
            ..Default::default()
        };
        let (n, has) = self.static_avt(inst, "name", None);
        c.name = n;
        c.has_name = has;
        if !has {
            self.style_error(
                s,
                Some(inst),
                "XSLT-attribute: The attribute 'name' is missing.\n",
            );
            return c;
        }
        let (ns, has_ns) = self.static_avt(inst, "namespace", None);
        c.ns = ns;
        c.has_ns = has_ns;
        if let Some(name) = c.name.clone() {
            if !valid_qname(&name) {
                self.style_error(
                    s,
                    Some(inst),
                    &format!(
                        "xsl:attribute: The value '{}' of the attribute 'name' is not a valid QName.\n",
                        name
                    ),
                );
            } else if name == "xmlns" {
                self.style_error(
                    s,
                    Some(inst),
                    "xsl:attribute: The attribute name 'xmlns' is not allowed.\n",
                );
            } else {
                let (prefix, _local) = split_qname(&name);
                if let Some(p) = prefix {
                    if !c.has_ns {
                        let ns = tw(|t| t.search_ns(inst, Some(&p)).map(|n| t.nss[n].href.clone()));
                        if let Some(h) = ns {
                            c.ns = Some(h);
                            c.has_ns = true;
                        } else {
                            self.style_error(
                                s,
                                Some(inst),
                                &format!(
                                    "xsl:attribute: The prefixed QName '{}' has no namespace binding in scope in the stylesheet; this is an error, since the namespace was not specified by the instruction itself.\n",
                                    name
                                ),
                            );
                        }
                    }
                }
            }
        }
        c
    }

    fn select_comp(&mut self, s: usize, inst: NodeId, kind: Kind, what: &str) -> PreComp {
        let mut c = PreComp {
            kind,
            inst,
            ..Default::default()
        };
        c.select = self.cns_prop(inst, "select");
        match c.select.clone() {
            None => {
                self.style_error(s, Some(inst), &format!("{} : select is missing\n", what));
            }
            Some(sel) => {
                c.comp = self.xpath_compile(&sel);
                if c.comp.is_none() {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!("{} : could not compile select expression '{}'\n", what, sel),
                    );
                }
            }
        }
        c
    }

    fn value_of_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut noescape = false;
        if let Some(p) = self.cns_prop(inst, "disable-output-escaping") {
            if p == "yes" {
                noescape = true;
            } else if p != "no" {
                self.style_warning(
                    s,
                    Some(inst),
                    "xsl:value-of : disable-output-escaping allows only yes or no\n",
                );
            }
        }
        let mut c = self.select_comp(s, inst, Kind::ValueOf, "xsl:value-of");
        c.noescape = noescape;
        c
    }

    fn test_comp(&mut self, s: usize, inst: NodeId, kind: Kind, what: &str) -> PreComp {
        let mut c = PreComp {
            kind,
            inst,
            ..Default::default()
        };
        c.test = self.cns_prop(inst, "test");
        match c.test.clone() {
            None => {
                self.style_error(s, Some(inst), &format!("{} : test is not defined\n", what));
            }
            Some(test) => {
                c.comp = self.xpath_compile(&test);
                if c.comp.is_none() {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!("{} : could not compile test expression '{}'\n", what, test),
                    );
                }
            }
        }
        c
    }

    fn for_each_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::ForEach,
            inst,
            ..Default::default()
        };
        c.select = self.cns_prop(inst, "select");
        match c.select.clone() {
            None => {
                self.style_error(s, Some(inst), "xsl:for-each : select is missing\n");
            }
            Some(sel) => {
                c.comp = self.xpath_compile(&sel);
                if c.comp.is_none() {
                    self.style_error(
                        s,
                        Some(inst),
                        &format!(
                            "xsl:for-each : could not compile select expression '{}'\n",
                            sel
                        ),
                    );
                }
            }
        }
        c
    }

    fn apply_templates_comp(&mut self, s: usize, inst: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::ApplyTemplates,
            inst,
            ..Default::default()
        };
        let (_has, ns, local) = self.qname_property(s, inst, "mode", false);
        c.mode = local;
        c.mode_uri = ns;
        c.select = self.cns_prop(inst, "select");
        if let Some(sel) = c.select.clone() {
            c.comp = self.xpath_compile(&sel);
            if c.comp.is_none() {
                self.style_error(
                    s,
                    Some(inst),
                    &format!(
                        "XSLT-apply-templates: could not compile select expression '{}'\n",
                        sel
                    ),
                );
            }
        }
        c
    }

    fn with_param_comp(&mut self, s: usize, inst: NodeId, kind: Kind) -> PreComp {
        let mut c = PreComp {
            kind,
            inst,
            ..Default::default()
        };
        let (has, ns, local) = self.qname_property(s, inst, "name", true);
        c.has_name = has;
        c.has_ns = ns.is_some();
        c.ns = ns;
        c.name = local;
        c.select = self.cns_prop(inst, "select");
        if let Some(sel) = c.select.clone() {
            c.comp = self.xpath_compile(&sel);
            match kind {
                Kind::WithParam => {
                    if c.comp.is_none() {
                        self.style_error(
                            s,
                            Some(inst),
                            &format!(
                                "XSLT-with-param: Failed to compile select expression '{}'\n",
                                sel
                            ),
                        );
                    }
                    if tw(|t| t.nodes[inst].children.is_some()) {
                        self.style_warning(
                            s,
                            Some(inst),
                            "XSLT-with-param: The content should be empty since the attribute select is present.\n",
                        );
                    }
                }
                Kind::Param => {
                    if c.comp.is_none() {
                        self.style_error(
                            s,
                            Some(inst),
                            &format!(
                                "XSLT-param: could not compile select expression '{}'.\n",
                                sel
                            ),
                        );
                    }
                    if tw(|t| t.nodes[inst].children.is_some()) {
                        self.style_warning(
                            s,
                            Some(inst),
                            "XSLT-param: The content should be empty since the attribute 'select' is present.\n",
                        );
                    }
                }
                _ => {
                    if c.comp.is_none() {
                        self.style_error(
                            s,
                            Some(inst),
                            &format!(
                                "XSLT-variable: Failed to compile the XPath expression '{}'.\n",
                                sel
                            ),
                        );
                    }
                    let kids = tw(|t| t.children_of(inst));
                    for k in kids {
                        let bad = tw(|t| {
                            let nd = &t.nodes[k];
                            nd.t != NType::Comment
                                && (nd.t != NType::Text || !is_blank(&nd.content))
                        });
                        if bad {
                            self.style_error(
                                s,
                                Some(inst),
                                "XSLT-variable: There must be no child nodes, since the attribute 'select' was specified.\n",
                            );
                        }
                    }
                }
            }
        }
        c
    }

    fn number_comp(&mut self, s: usize, cur: NodeId) -> PreComp {
        let mut c = PreComp {
            kind: Kind::Number,
            inst: cur,
            ..Default::default()
        };
        c.numdata.value = self.cns_prop(cur, "value");
        let (fmt, has) = self.static_avt(cur, "format", Some(XSLT_NAMESPACE));
        c.numdata.has_format = has;
        c.numdata.format = if !has { Some(String::new()) } else { fmt };
        c.numdata.count = self.cns_prop(cur, "count");
        c.numdata.from = self.cns_prop(cur, "from");
        let doc = tw(|t| t.nodes[cur].doc.unwrap_or(cur));
        if let Some(p) = c.numdata.count.clone() {
            c.numdata.count_pat = self.compile_pattern(&p, doc, cur, Some(s), false);
        }
        if let Some(p) = c.numdata.from.clone() {
            c.numdata.from_pat = self.compile_pattern(&p, doc, cur, Some(s), false);
        }
        if let Some(p) = self.cns_prop(cur, "level") {
            if p == "single" || p == "multiple" || p == "any" {
                c.numdata.level = Some(p);
            } else {
                self.style_warning(
                    s,
                    Some(cur),
                    &format!("xsl:number : invalid value {} for level\n", p),
                );
            }
        }
        if self.cns_prop(cur, "lang").is_some() {
            self.err(
                Some(s),
                Some(cur),
                "xsl:number : lang attribute not implemented\n",
            );
        }
        if let Some(p) = self.cns_prop(cur, "letter-value") {
            if p == "alphabetic" {
                self.style_warning(
                    s,
                    Some(cur),
                    "xsl:number : letter-value 'alphabetic' not implemented\n",
                );
            } else if p == "traditional" {
                self.style_warning(
                    s,
                    Some(cur),
                    "xsl:number : letter-value 'traditional' not implemented\n",
                );
            } else {
                self.style_warning(
                    s,
                    Some(cur),
                    &format!("xsl:number : invalid value {} for letter-value\n", p),
                );
            }
        }
        if let Some(p) = self.cns_prop(cur, "grouping-separator") {
            match p.chars().next() {
                Some(ch) => {
                    c.numdata.grouping_character = ch as u32;
                    c.numdata.grouping_character_len = ch.len_utf8();
                }
                None => {
                    c.numdata.grouping_character = 0;
                }
            }
        }
        if let Some(p) = self.cns_prop(cur, "grouping-size") {
            c.numdata.digits_per_group = scanf_int(&p).unwrap_or(c.numdata.digits_per_group);
        } else {
            c.numdata.grouping_character = 0;
        }
        if c.numdata.value.is_none() && c.numdata.level.is_none() {
            c.numdata.level = Some("single".into());
        }
        c
    }
}

/// sscanf("%d")
pub fn scanf_int(s: &str) -> Option<i32> {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let st = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == st {
        return None;
    }
    t[..i].parse::<i64>().ok().map(|v| v as i32)
}

/// keys.c skipPredicate: position after the matching ']'
fn skip_predicate(b: &[u8], mut end: usize) -> Option<usize> {
    if b[end] != b'[' {
        return Some(end);
    }
    end += 1;
    let mut level = 0;
    while end < b.len() {
        if b[end] == b'\'' || b[end] == b'"' {
            let lim = b[end];
            end += 1;
            while end < b.len() && b[end] != lim {
                end += 1;
            }
            if end >= b.len() {
                return None;
            }
            end += 1;
            continue;
        } else if b[end] == b'[' {
            level += 1;
        } else if b[end] == b']' {
            if level == 0 {
                return Some(end + 1);
            }
            level -= 1;
        }
        end += 1;
    }
    None
}

pub fn tree_dummy(_t: &Tree) {}
