//! Compiles a stylesheet document (XSLT 1.0 sections 2, 7-16) into
//! [`Stylesheet`].

use std::collections::HashMap;
use std::rc::Rc;

use super::{Host, XSL_NS};
use crate::tree::{Arena, NodeData, NodeId, QName, Str, XML_NS, XMLNS_NS};
use crate::xpath::pattern::{self, Pattern, PatternKey};
use crate::xpath::{self, BoundExpr, NsMap};

/// A compiled XPath expression with the stylesheet module it was written in
/// (the base URI of `document()`).
#[derive(Debug)]
pub struct XExpr {
    pub(crate) b: BoundExpr,
    pub(crate) module: u32,
}

/// An attribute value template (section 7.6.2).
#[derive(Debug)]
pub struct Avt(pub(crate) Vec<AvtPart>);

#[derive(Debug)]
pub(crate) enum AvtPart {
    Lit(String),
    Expr(XExpr),
}

/// An expanded name of a variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarName {
    pub(crate) ns: Str,
    pub(crate) local: Str,
}

/// Where a variable or parameter gets its value (section 11.2).
#[derive(Debug)]
pub(crate) enum ValueSrc {
    Select(XExpr),
    Body(Vec<Instr>),
    Empty,
}

#[derive(Debug)]
pub(crate) struct WithParam {
    pub name: VarName,
    pub value: ValueSrc,
}

#[derive(Debug)]
pub(crate) struct SortKey {
    pub select: XExpr,
    pub order: Option<Avt>,
    pub data_type: Option<Avt>,
    pub case_order: Option<Avt>,
}

#[derive(Debug)]
pub(crate) struct Lre {
    pub name: QName,
    pub decls: Vec<(Option<Str>, Str)>,
    pub attrs: Vec<(QName, Avt)>,
    pub sets: Vec<Str>,
    pub body: Vec<Instr>,
}

#[derive(Debug)]
pub(crate) struct NumberSpec {
    pub value: Option<XExpr>,
    pub level: u8,
    pub count: Option<Vec<Pattern>>,
    pub from: Option<Vec<Pattern>>,
    pub format: Avt,
    pub group_sep: Option<Avt>,
    pub group_size: Option<Avt>,
}

#[derive(Debug)]
pub(crate) struct DocumentSpec {
    pub href: Avt,
    pub output: OutputSpec,
    pub body: Vec<Instr>,
}

/// One instruction with its place in the stylesheet.
#[derive(Debug)]
pub struct Instr {
    pub(crate) kind: Kind,
    pub(crate) line: u32,
    pub(crate) module: u32,
    pub(crate) elem: Str,
}

#[derive(Debug)]
pub(crate) enum Kind {
    Text {
        text: String,
        doe: bool,
    },
    ValueOf {
        select: XExpr,
        doe: bool,
    },
    Lre(Box<Lre>),
    ApplyTemplates {
        select: Option<XExpr>,
        mode: u32,
        sorts: Vec<SortKey>,
        params: Vec<WithParam>,
    },
    CallTemplate {
        name: Str,
        params: Vec<WithParam>,
    },
    ApplyImports,
    ForEach {
        select: XExpr,
        sorts: Vec<SortKey>,
        body: Vec<Instr>,
    },
    If {
        test: XExpr,
        body: Vec<Instr>,
    },
    Choose {
        whens: Vec<(XExpr, Vec<Instr>)>,
        otherwise: Vec<Instr>,
    },
    Var {
        name: VarName,
        value: ValueSrc,
        is_param: bool,
    },
    Attribute {
        name: Avt,
        ns: Option<Avt>,
        body: Vec<Instr>,
        scope: NsMap,
    },
    Element {
        name: Avt,
        ns: Option<Avt>,
        sets: Vec<Str>,
        body: Vec<Instr>,
        scope: NsMap,
    },
    Comment {
        body: Vec<Instr>,
    },
    Pi {
        name: Avt,
        body: Vec<Instr>,
    },
    Copy {
        sets: Vec<Str>,
        body: Vec<Instr>,
    },
    CopyOf {
        select: XExpr,
    },
    Number(Box<NumberSpec>),
    Message {
        body: Vec<Instr>,
        terminate: Option<Avt>,
    },
    Document(Box<DocumentSpec>),
    FuncResult {
        value: ValueSrc,
    },
    /// An extension element nobody implements: its `xsl:fallback`s.
    Unknown {
        name: String,
        fallback: Option<Vec<Instr>>,
    },
}

/// A template rule or named template (section 5).
#[derive(Debug)]
pub struct Template {
    /// The name shown in recursion reports: `name`, or `match` as written.
    pub(crate) label: String,
    pub(crate) body: Vec<Instr>,
    /// Import precedence.
    pub(crate) prec: u32,
    /// The lowest precedence among what its module imports
    /// (`xsl:apply-imports`).
    pub(crate) import_min: u32,
    pub(crate) mode: u32,
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub prec: u32,
    pub priority: f64,
    pub pos: u32,
    pub tpl: u32,
    pub pat: Pattern,
}

/// The template rules of one mode, indexed by what they can match, each list
/// best first.
#[derive(Debug, Default)]
pub(crate) struct ModeTable {
    pub elements: HashMap<Str, Vec<Entry>>,
    pub any_element: Vec<Entry>,
    pub attributes: HashMap<Str, Vec<Entry>>,
    pub any_attribute: Vec<Entry>,
    pub text: Vec<Entry>,
    pub comment: Vec<Entry>,
    pub pi: Vec<Entry>,
    pub root: Vec<Entry>,
    pub any: Vec<Entry>,
}

#[derive(Debug)]
pub(crate) struct GlobalVar {
    pub name: VarName,
    pub value: ValueSrc,
    pub is_param: bool,
    pub prec: u32,
}

#[derive(Debug)]
pub(crate) struct AttrSetDef {
    pub uses: Vec<Str>,
    pub attrs: Vec<Instr>,
}

#[derive(Debug)]
pub(crate) struct KeyDef {
    pub pats: Vec<Pattern>,
    pub use_expr: XExpr,
}

/// `xsl:decimal-format` (section 12.3).
#[derive(Clone, Debug)]
pub struct DecimalFormat {
    pub decimal_separator: char,
    pub grouping_separator: char,
    pub infinity: String,
    pub minus_sign: char,
    pub nan: String,
    pub percent: char,
    pub per_mille: char,
    pub zero_digit: char,
    pub digit: char,
    pub pattern_separator: char,
}

impl Default for DecimalFormat {
    fn default() -> DecimalFormat {
        DecimalFormat {
            decimal_separator: '.',
            grouping_separator: ',',
            infinity: "Infinity".into(),
            minus_sign: '-',
            nan: "NaN".into(),
            percent: '%',
            per_mille: '\u{2030}',
            zero_digit: '0',
            digit: '#',
            pattern_separator: ';',
        }
    }
}

/// The output methods (section 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutMethod {
    Xml,
    Html,
    Text,
    Xhtml,
}

/// `xsl:output` (section 16).
#[derive(Clone, Debug, Default)]
pub struct OutputSpec {
    pub method: Option<OutMethod>,
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub omit_xml_declaration: Option<bool>,
    pub standalone: Option<bool>,
    pub doctype_public: Option<String>,
    pub doctype_system: Option<String>,
    pub indent: Option<bool>,
    pub media_type: Option<String>,
    /// Expanded names (`{uri}local` or `local`).
    pub cdata_elements: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum SpaceTest {
    Any,
    Ns(Str),
    Name(Str, Str),
}

#[derive(Debug)]
pub(crate) struct SpaceRule {
    pub test: SpaceTest,
    pub prec: u32,
    pub strip: bool,
}

#[derive(Debug)]
pub(crate) struct FuncDef {
    pub body: Vec<Instr>,
}

/// A stylesheet module.
#[derive(Debug)]
pub struct Module {
    /// The document's URL.
    pub url: Option<String>,
    /// The document in the arena; the embedder keeps it alive.
    pub doc: NodeId,
}

/// A compiled stylesheet.
#[derive(Debug, Default)]
pub struct Stylesheet {
    pub modules: Vec<Module>,
    pub(crate) templates: Vec<Template>,
    pub(crate) named: HashMap<Str, u32>,
    pub(crate) modes: Vec<ModeTable>,
    pub(crate) mode_ids: HashMap<Option<Str>, u32>,
    pub(crate) globals: Vec<GlobalVar>,
    pub(crate) attr_sets: HashMap<Str, Vec<AttrSetDef>>,
    pub(crate) keys: HashMap<Str, Vec<KeyDef>>,
    pub(crate) decimal_formats: HashMap<Option<Str>, DecimalFormat>,
    pub(crate) space_rules: Vec<SpaceRule>,
    pub(crate) funcs: HashMap<(String, String), FuncDef>,
    /// `xsl:output`, merged.
    pub output: OutputSpec,
}

/// Namespaces of the extension elements implemented here.
pub(crate) const EXSLT_COMMON: &str = "http://exslt.org/common";
pub(crate) const EXSLT_FUNCTIONS: &str = "http://exslt.org/functions";
const SAXON_NS: &str = "http://icl.com/saxon";
const SAXON6_NS: &str = "http://saxon.sf.net/";
const XALAN_REDIRECT_NS: &str = "org.apache.xalan.xslt.extensions.Redirect";
const XALAN_REDIRECT_NS2: &str = "http://xml.apache.org/xalan/redirect";

/// The extension elements that write another document.
pub(crate) fn is_document_element(ns: &str, local: &str) -> bool {
    match ns {
        EXSLT_COMMON => local == "document",
        SAXON_NS | SAXON6_NS => local == "output",
        XALAN_REDIRECT_NS | XALAN_REDIRECT_NS2 => local == "write",
        XSL_NS => local == "document",
        _ => false,
    }
}

/// `{uri}local`, or `local` without namespace.
pub(crate) fn clark(ns: &str, local: &str) -> String {
    if ns.is_empty() {
        local.to_string()
    } else {
        format!("{{{ns}}}{local}")
    }
}

/// What is in scope at a stylesheet element.
#[derive(Clone)]
struct Scope {
    /// Prefix bindings ("" = default namespace).
    ns: NsMap,
    /// The same, in declaration order.
    decls: Rc<Vec<(String, String)>>,
    /// Excluded namespace URIs (`exclude-result-prefixes`).
    exclude: Rc<Vec<String>>,
    /// Extension namespace URIs (`extension-element-prefixes`).
    ext: Rc<Vec<String>>,
    /// `xml:space="preserve"` applies.
    preserve: bool,
}

struct TopItem {
    el: NodeId,
    module: u32,
    scope: Scope,
}

struct Compiler<'c> {
    arena: &'c mut Arena,
    host: &'c mut dyn Host,
    ss: Stylesheet,
    errors: u32,
    /// URLs of the modules being read (recursion check).
    stack: Vec<String>,
    /// Module of the code being compiled.
    module: u32,
    next_prec: u32,
    next_pos: u32,
    aliases: HashMap<String, (Option<String>, String)>,
    /// Prefixes of the whole document, for `Dom\` stylesheets.
    extra_ns: Option<Vec<(String, String)>>,
    rules: Vec<(u32, Vec<Pattern>, f64, u32)>,
    version_10: bool,
}

/// Compiles the stylesheet document `doc`. Problems are reported to the
/// host; `None` when there were errors.
pub fn compile(arena: &mut Arena, doc: NodeId, host: &mut dyn Host) -> Option<Stylesheet> {
    let mut c = Compiler {
        arena,
        host,
        ss: Stylesheet::default(),
        errors: 0,
        stack: Vec::new(),
        module: 0,
        next_prec: 1,
        next_pos: 0,
        aliases: HashMap::new(),
        extra_ns: None,
        rules: Vec::new(),
        version_10: true,
    };
    c.ss.mode_ids.insert(None, 0);
    c.ss.modes.push(ModeTable::default());
    c.ss.decimal_formats.insert(None, DecimalFormat::default());
    c.run(doc);
    c.finish_rules();
    if c.errors > 0 {
        return None;
    }
    Some(c.ss)
}

fn is_ws(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

/// Splits on whitespace (attribute lists of names).
fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split([' ', '\t', '\n', '\r']).filter(|t| !t.is_empty())
}

impl Compiler<'_> {
    // ----- diagnostics ------------------------------------------------------

    fn location(&self, el: NodeId) -> String {
        let file = self
            .ss
            .modules
            .get(self.module as usize)
            .and_then(|m| m.url.clone())
            .unwrap_or_else(|| "(null)".into());
        let name = self
            .arena
            .qname(el)
            .map_or(String::new(), |q| q.local.to_string());
        format!(
            "compilation error: file {file} line {} element {name}",
            self.arena.line(el)
        )
    }

    fn error(&mut self, el: NodeId, msg: &str) {
        let loc = self.location(el);
        self.host.report(self.arena, &loc);
        self.host.report(self.arena, msg);
        self.errors += 1;
    }

    fn warning(&mut self, el: NodeId, msg: &str) {
        let loc = self.location(el);
        self.host.report(self.arena, &loc);
        self.host.report(self.arena, msg);
    }

    // ----- modules ----------------------------------------------------------

    fn run(&mut self, doc: NodeId) {
        let Some(root) = self.arena.document_element(doc) else {
            self.host
                .report(self.arena, "xsltParseStylesheetProcess : empty stylesheet");
            self.errors += 1;
            return;
        };
        if self.arena.is_modern(doc) {
            // Prefixes used anywhere count as declared on the stylesheet
            // element (test auto_registration_namespaces_new_dom.phpt).
            let mut all = Vec::new();
            for n in self.arena.descendants(doc) {
                let q = match self.arena.data(n) {
                    NodeData::Element(e) => Some(&e.name),
                    _ => None,
                };
                if let Some(q) = q
                    && let (Some(p), Some(u)) = (&q.prefix, &q.ns)
                    && !all.iter().any(|(x, _): &(String, String)| x == &**p)
                {
                    all.push((p.to_string(), u.to_string()));
                }
            }
            self.extra_ns = Some(all);
        }
        let _ = root;
        self.module_tree(doc);
    }

    /// Compiles a module and what it imports; returns nothing, precedence
    /// numbers grow in import order (section 2.6.2).
    fn module_tree(&mut self, doc: NodeId) {
        let url = self.arena.doc(doc).url.clone();
        let module = self.ss.modules.len() as u32;
        self.ss.modules.push(Module {
            url: url.clone(),
            doc,
        });
        self.module = module;
        let Some(root) = self.arena.document_element(doc) else {
            return;
        };
        if let Some(u) = &url {
            self.stack.push(u.clone());
        }
        let import_min = self.next_prec;
        let mut items = Vec::new();
        let mut imports = Vec::new();
        let simple = !self.is_xsl(root, "stylesheet") && !self.is_xsl(root, "transform");
        if simple {
            self.simplified(root, doc, import_min);
        } else {
            let scope = self.root_scope(root);
            self.flatten(root, module, scope, &mut items, &mut imports);
            for (el, m, href) in imports {
                self.module = m;
                let base = self.ss.modules[m as usize].url.clone();
                let target = self.host.resolve(&href, base.as_deref());
                if self.stack.contains(&target) {
                    self.error(
                        el,
                        &format!("xsl:import : recursion detected on imported URL {target}"),
                    );
                    continue;
                }
                match self.host.load(self.arena, &target, true) {
                    Some(d) => {
                        self.module_tree(d);
                    }
                    None => {
                        self.error(el, &format!("xsl:import : unable to load {href}"));
                    }
                }
            }
            let prec = self.next_prec;
            self.next_prec += 1;
            self.top_items(items, prec, import_min);
        }
        self.module = module;
        if url.is_some() {
            self.stack.pop();
        }
    }

    fn is_xsl(&self, n: NodeId, local: &str) -> bool {
        self.arena
            .qname(n)
            .is_some_and(|q| q.ns_str() == XSL_NS && &*q.local == local)
    }

    fn attr(&self, el: NodeId, name: &str) -> Option<String> {
        let a = self.arena.get_attr_node_ns(el, None, name)?;
        Some(self.arena.attr_value(a))
    }

    fn xsl_attr(&self, el: NodeId, name: &str) -> Option<String> {
        let a = self.arena.get_attr_node_ns(el, Some(XSL_NS), name)?;
        Some(self.arena.attr_value(a))
    }

    fn root_scope(&mut self, root: NodeId) -> Scope {
        let mut scope = Scope {
            ns: Rc::new(HashMap::new()),
            decls: Rc::new(Vec::new()),
            exclude: Rc::new(Vec::new()),
            ext: Rc::new(Vec::new()),
            preserve: false,
        };
        // Declarations of the ancestors (none for a document element) then
        // of the element.
        if let Some(extra) = self.extra_ns.clone() {
            let mut map = HashMap::new();
            let mut decls = Vec::new();
            for (p, u) in extra {
                map.insert(p.clone(), u.clone());
                decls.push((p, u));
            }
            scope.ns = Rc::new(map);
            scope.decls = Rc::new(decls);
        }
        self.enter(root, &scope, true)
    }

    /// The scope inside element `el`.
    fn enter(&mut self, el: NodeId, outer: &Scope, is_xsl_el: bool) -> Scope {
        let mut own: Vec<(String, String)> = Vec::new();
        if let Some(e) = self.arena.element(el) {
            for d in &e.ns_decls {
                own.push((
                    d.prefix.as_deref().unwrap_or("").to_string(),
                    d.uri.to_string(),
                ));
            }
        }
        for a in self.arena.attrs(el) {
            let ad = self.arena.attr(a).unwrap();
            if ad.name.ns.as_deref() == Some(XMLNS_NS) {
                let p = if ad.name.prefix.is_some() {
                    ad.name.local.to_string()
                } else {
                    String::new()
                };
                own.push((p, self.arena.attr_value(a)));
            }
        }
        let mut scope = outer.clone();
        if !own.is_empty() {
            let mut map: HashMap<String, String> = (*outer.ns).clone();
            let mut decls: Vec<(String, String)> = (*outer.decls).clone();
            for (p, u) in own {
                decls.retain(|(x, _)| *x != p);
                if u.is_empty() {
                    map.remove(&p);
                } else {
                    map.insert(p.clone(), u.clone());
                    decls.push((p, u));
                }
            }
            scope.ns = Rc::new(map);
            scope.decls = Rc::new(decls);
        }
        let (ex, ext) = if is_xsl_el {
            (
                self.attr(el, "exclude-result-prefixes"),
                self.attr(el, "extension-element-prefixes"),
            )
        } else {
            (
                self.xsl_attr(el, "exclude-result-prefixes"),
                self.xsl_attr(el, "extension-element-prefixes"),
            )
        };
        // On the stylesheet element the attributes may also carry the XSLT
        // prefix (test file xslt011.xsl).
        let (ex, ext) = if is_xsl_el {
            (
                ex.or_else(|| self.xsl_attr(el, "exclude-result-prefixes")),
                ext.or_else(|| self.xsl_attr(el, "extension-element-prefixes")),
            )
        } else {
            (ex, ext)
        };
        if let Some(list) = ex {
            let uris = self.prefix_list(el, &list, &scope, "exclude-result-prefixes");
            if !uris.is_empty() {
                let mut v = (*scope.exclude).clone();
                v.extend(uris);
                scope.exclude = Rc::new(v);
            }
        }
        if let Some(list) = ext {
            let uris = self.prefix_list(el, &list, &scope, "extension-element-prefixes");
            if !uris.is_empty() {
                let mut v = (*scope.ext).clone();
                v.extend(uris);
                scope.ext = Rc::new(v);
            }
        }
        match self.arena.get_attr_node_ns(el, Some(XML_NS), "space") {
            Some(a) => match self.arena.attr_value(a).as_str() {
                "preserve" => scope.preserve = true,
                "default" => scope.preserve = false,
                _ => {}
            },
            None => {}
        }
        scope
    }

    fn prefix_list(&mut self, el: NodeId, list: &str, scope: &Scope, what: &str) -> Vec<String> {
        let mut out = Vec::new();
        for p in tokens(list) {
            let key = if p == "#default" { "" } else { p };
            match scope.ns.get(key) {
                Some(u) => out.push(u.clone()),
                None => {
                    if p != "#default" {
                        self.warning(el, &format!("xsl:{what} : undefined namespace {p}"));
                    }
                }
            }
        }
        out
    }

    /// Top-level elements with `xsl:include`s replaced by their contents,
    /// and the `xsl:import`s (section 2.6.1).
    fn flatten(
        &mut self,
        root: NodeId,
        module: u32,
        scope: Scope,
        items: &mut Vec<TopItem>,
        imports: &mut Vec<(NodeId, u32, String)>,
    ) {
        if let Some(v) = self.attr(root, "version") {
            if v == "1.0" || v == "1.1" {
                // Known versions.
            } else {
                self.version_10 = false;
            }
        }
        let kids: Vec<NodeId> = self.arena.children(root).collect();
        for k in kids {
            match self.arena.data(k) {
                NodeData::Element(_) => {}
                NodeData::Text(t) | NodeData::CData(t) if !is_ws(t) => {
                    let msg = "xsltParseStylesheetProcess : text not allowed at the top level";
                    self.module = module;
                    self.warning(root, msg);
                    continue;
                }
                _ => continue,
            }
            if self.is_xsl(k, "import") {
                match self.attr(k, "href") {
                    Some(h) => imports.push((k, module, h)),
                    None => {
                        self.module = module;
                        self.error(k, "xsl:import : invalid URI reference");
                    }
                }
                continue;
            }
            if self.is_xsl(k, "include") {
                let Some(h) = self.attr(k, "href") else {
                    self.module = module;
                    self.error(k, "xsl:include : invalid URI reference");
                    continue;
                };
                let base = self.ss.modules[module as usize].url.clone();
                let target = self.host.resolve(&h, base.as_deref());
                self.module = module;
                if self.stack.contains(&target) {
                    self.error(
                        k,
                        &format!("xsl:include : recursion detected on included URL {target}"),
                    );
                    continue;
                }
                let Some(d) = self.host.load(self.arena, &target, true) else {
                    self.error(k, &format!("xsl:include : unable to load {h}"));
                    continue;
                };
                let m = self.ss.modules.len() as u32;
                self.ss.modules.push(Module {
                    url: self.arena.doc(d).url.clone(),
                    doc: d,
                });
                self.stack.push(target);
                if let Some(r) = self.arena.document_element(d) {
                    let s = self.root_scope(r);
                    self.flatten(r, m, s, items, imports);
                }
                self.stack.pop();
                continue;
            }
            let s = self.enter(k, &scope, true);
            items.push(TopItem {
                el: k,
                module,
                scope: s,
            });
        }
    }

    fn simplified(&mut self, root: NodeId, doc: NodeId, import_min: u32) {
        // Section 2.3: a literal result element as the stylesheet.
        if self.xsl_attr(root, "version").is_none() {
            self.host.report(
                self.arena,
                "xsltParseStylesheetProcess : document is not a stylesheet",
            );
            self.errors += 1;
            return;
        }
        let empty = Scope {
            ns: Rc::new(HashMap::new()),
            decls: Rc::new(Vec::new()),
            exclude: Rc::new(Vec::new()),
            ext: Rc::new(Vec::new()),
            preserve: false,
        };
        let prec = self.next_prec;
        self.next_prec += 1;
        let scope = self.enter(doc, &empty, false);
        let body = self.body_of(&[root], &scope, true);
        let pats = match pattern::compile("/", &scope.ns) {
            Ok(p) => p,
            Err(_) => return,
        };
        self.add_template(
            Template {
                label: "/".into(),
                body,
                prec,
                import_min,
                mode: 0,
            },
            Some(pats),
            None,
            None,
        );
    }

    fn top_items(&mut self, items: Vec<TopItem>, prec: u32, import_min: u32) {
        // Aliases first: templates anywhere use them.
        for it in &items {
            if self.is_xsl(it.el, "namespace-alias") {
                self.module = it.module;
                self.namespace_alias(it.el, &it.scope);
            }
        }
        for it in items {
            self.module = it.module;
            self.top_item(it, prec, import_min);
        }
    }

    fn namespace_alias(&mut self, el: NodeId, scope: &Scope) {
        let (Some(from), Some(to)) = (
            self.attr(el, "stylesheet-prefix"),
            self.attr(el, "result-prefix"),
        ) else {
            self.error(el, "namespace-alias: missing prefix attribute");
            return;
        };
        let from_key = if from == "#default" {
            ""
        } else {
            from.as_str()
        };
        let to_key = if to == "#default" { "" } else { to.as_str() };
        let from_uri = scope.ns.get(from_key).cloned().unwrap_or_default();
        let to_uri = scope.ns.get(to_key).cloned().unwrap_or_default();
        let to_prefix = if to_key.is_empty() {
            None
        } else {
            Some(to.clone())
        };
        self.aliases.insert(from_uri, (to_prefix, to_uri));
    }

    fn top_item(&mut self, it: TopItem, prec: u32, import_min: u32) {
        let el = it.el;
        let scope = &it.scope;
        let Some(q) = self.arena.qname(el).cloned() else {
            return;
        };
        if q.ns_str() == super::compile::EXSLT_FUNCTIONS && &*q.local == "function" {
            self.func_def(el, scope);
            return;
        }
        if q.ns_str() != XSL_NS {
            // User-defined top-level data (section 2.2).
            return;
        }
        match &*q.local {
            "template" => self.template(el, scope, prec, import_min),
            "variable" | "param" => {
                let is_param = &*q.local == "param";
                let Some(name) = self.var_name_attr(el, scope) else {
                    return;
                };
                let value = self.value_src(el, scope);
                // The highest precedence wins; the same precedence twice is
                // an error (section 11.4).
                if let Some(g) = self.ss.globals.iter_mut().find(|g| g.name == name) {
                    if g.prec == prec {
                        let msg = format!("Global parameter {} already defined", name.local);
                        self.warning(el, &msg);
                        return;
                    }
                    g.value = value;
                    g.is_param = is_param;
                    g.prec = prec;
                } else {
                    self.ss.globals.push(GlobalVar {
                        name,
                        value,
                        is_param,
                        prec,
                    });
                }
            }
            "attribute-set" => self.attribute_set(el, scope),
            "key" => self.key(el, scope),
            "decimal-format" => self.decimal_format(el, scope),
            "output" => self.output(el, scope),
            "strip-space" | "preserve-space" => {
                let strip = &*q.local == "strip-space";
                let Some(list) = self.attr(el, "elements") else {
                    self.error(el, "xsl:strip-space : missing elements attribute");
                    return;
                };
                for t in tokens(&list) {
                    let test = if t == "*" {
                        SpaceTest::Any
                    } else if let Some(p) = t.strip_suffix(":*") {
                        SpaceTest::Ns(scope.ns.get(p).map_or("", String::as_str).into())
                    } else if let Some((p, l)) = t.split_once(':') {
                        SpaceTest::Name(scope.ns.get(p).map_or("", String::as_str).into(), l.into())
                    } else {
                        SpaceTest::Name("".into(), t.into())
                    };
                    self.ss.space_rules.push(SpaceRule { test, prec, strip });
                }
            }
            "namespace-alias" => {}
            other => {
                if self.version_10 {
                    let msg = format!("xsltParseStylesheetTop: unknown {other} element");
                    self.warning(el, &msg);
                }
            }
        }
    }

    // ----- names ------------------------------------------------------------

    /// Expands a QName written in an attribute (no default namespace,
    /// section 2.4).
    fn expand(&mut self, el: NodeId, name: &str, scope: &Scope) -> Option<(String, String)> {
        if let Some((p, l)) = name.split_once(':') {
            let uri = if p == "xml" {
                XML_NS.to_string()
            } else if let Some(u) = scope.ns.get(p) {
                u.clone()
            } else {
                self.error(el, &format!("Undefined namespace prefix: {p}"));
                return None;
            };
            Some((uri, l.to_string()))
        } else {
            Some((String::new(), name.to_string()))
        }
    }

    fn clark_of(&mut self, el: NodeId, name: &str, scope: &Scope) -> Option<Str> {
        let (u, l) = self.expand(el, name.trim(), scope)?;
        Some(clark(&u, &l).into())
    }

    fn var_name_attr(&mut self, el: NodeId, scope: &Scope) -> Option<VarName> {
        let Some(n) = self.attr(el, "name") else {
            let local = self
                .arena
                .qname(el)
                .map_or(String::new(), |q| q.local.to_string());
            self.error(el, &format!("xsl:{local} : missing name attribute"));
            return None;
        };
        let (u, l) = self.expand(el, n.trim(), scope)?;
        Some(VarName {
            ns: u.as_str().into(),
            local: l.as_str().into(),
        })
    }

    fn mode_id(&mut self, name: Option<Str>) -> u32 {
        if let Some(&m) = self.ss.mode_ids.get(&name) {
            return m;
        }
        let m = self.ss.modes.len() as u32;
        self.ss.modes.push(ModeTable::default());
        self.ss.mode_ids.insert(name, m);
        m
    }

    fn sets_attr(&mut self, el: NodeId, value: Option<String>, scope: &Scope) -> Vec<Str> {
        let mut out = Vec::new();
        if let Some(v) = value {
            for t in tokens(&v) {
                if let Some(c) = self.clark_of(el, t, scope) {
                    out.push(c);
                }
            }
        }
        out
    }

    // ----- expressions ------------------------------------------------------

    fn expr(&mut self, el: NodeId, text: &str, scope: &Scope) -> Option<XExpr> {
        match xpath::compile_bound(text, scope.ns.clone()) {
            Ok(b) => Some(XExpr {
                b,
                module: self.module,
            }),
            Err(e) => {
                self.xpath_error(el, text, &e);
                None
            }
        }
    }

    fn xpath_error(&mut self, el: NodeId, text: &str, e: &xpath::XPathError) {
        self.host.report(self.arena, &e.message);
        let msg = format!("xsltCompileXPath: could not compile '{text}'");
        self.error(el, &msg);
    }

    fn req_expr(&mut self, el: NodeId, attr: &str, scope: &Scope) -> Option<XExpr> {
        let Some(t) = self.attr(el, attr) else {
            let local = self
                .arena
                .qname(el)
                .map_or(String::new(), |q| q.local.to_string());
            self.error(el, &format!("xsl:{local} : missing {attr} attribute"));
            return None;
        };
        self.expr(el, &t, scope)
    }

    fn pattern_attr(&mut self, el: NodeId, text: &str, scope: &Scope) -> Option<Vec<Pattern>> {
        match pattern::compile(text, &scope.ns) {
            Ok(p) => Some(p),
            Err(_) => {
                self.error(
                    el,
                    &format!("xsltCompilePattern : failed to compile '{text}'"),
                );
                None
            }
        }
    }

    fn avt(&mut self, el: NodeId, text: &str, scope: &Scope) -> Option<Avt> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '{' {
                if chars.get(i + 1) == Some(&'{') {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                // The expression runs to the matching brace, literals
                // skipped (section 7.6.2).
                let mut j = i + 1;
                let mut quote = None;
                while j < chars.len() {
                    let d = chars[j];
                    match quote {
                        Some(q) if d == q => quote = None,
                        Some(_) => {}
                        None if d == '"' || d == '\'' => quote = Some(d),
                        None if d == '}' => break,
                        None => {}
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    self.error(
                        el,
                        &format!("Attribute template: unmatched '{{' in '{text}'"),
                    );
                    return None;
                }
                let src: String = chars[i + 1..j].iter().collect();
                let e = self.expr(el, &src, scope)?;
                if !lit.is_empty() {
                    parts.push(AvtPart::Lit(std::mem::take(&mut lit)));
                }
                parts.push(AvtPart::Expr(e));
                i = j + 1;
                continue;
            }
            if c == '}' {
                if chars.get(i + 1) == Some(&'}') {
                    lit.push('}');
                    i += 2;
                    continue;
                }
                self.error(
                    el,
                    &format!("Attribute template: unmatched '}}' in '{text}'"),
                );
                return None;
            }
            lit.push(c);
            i += 1;
        }
        if !lit.is_empty() {
            parts.push(AvtPart::Lit(lit));
        }
        Some(Avt(parts))
    }

    fn opt_avt(&mut self, el: NodeId, attr: &str, scope: &Scope) -> Option<Avt> {
        let t = self.attr(el, attr)?;
        self.avt(el, &t, scope)
    }

    fn value_src(&mut self, el: NodeId, scope: &Scope) -> ValueSrc {
        if let Some(s) = self.attr(el, "select") {
            return match self.expr(el, &s, scope) {
                Some(e) => ValueSrc::Select(e),
                None => ValueSrc::Empty,
            };
        }
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        let body = self.body_of(&kids, scope, false);
        if body.is_empty() {
            ValueSrc::Empty
        } else {
            ValueSrc::Body(body)
        }
    }

    // ----- templates --------------------------------------------------------

    fn template(&mut self, el: NodeId, scope: &Scope, prec: u32, import_min: u32) {
        let m_text = self.attr(el, "match");
        let n_text = self.attr(el, "name");
        if m_text.is_none() && n_text.is_none() {
            self.error(el, "xsl:template : error invalid name or match");
            return;
        }
        let priority = self
            .attr(el, "priority")
            .and_then(|p| p.trim().parse::<f64>().ok());
        let mode_name = match self.attr(el, "mode") {
            Some(m) => self.clark_of(el, &m, scope),
            None => None,
        };
        let mode = self.mode_id(mode_name);
        let pats = match &m_text {
            Some(t) => match self.pattern_attr(el, t, scope) {
                Some(p) => Some(p),
                None => return,
            },
            None => None,
        };
        let name = match &n_text {
            Some(n) => self.clark_of(el, n, scope),
            None => None,
        };
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        let body = self.body_of(&kids, scope, true);
        let label = n_text.or(m_text).unwrap_or_default();
        self.add_template(
            Template {
                label,
                body,
                prec,
                import_min,
                mode,
            },
            pats,
            name,
            priority,
        );
    }

    fn add_template(
        &mut self,
        t: Template,
        pats: Option<Vec<Pattern>>,
        name: Option<Str>,
        priority: Option<f64>,
    ) {
        let prec = t.prec;
        let mode = t.mode;
        let id = self.ss.templates.len() as u32;
        self.ss.templates.push(t);
        if let Some(n) = name {
            // The highest import precedence wins (section 6).
            let replace = match self.ss.named.get(&n) {
                Some(&old) => self.ss.templates[old as usize].prec <= prec,
                None => true,
            };
            if replace {
                self.ss.named.insert(n, id);
            }
        }
        if let Some(pats) = pats {
            for pat in pats {
                let pos = self.next_pos;
                self.next_pos += 1;
                let pr = priority.unwrap_or(pat.priority);
                let entry = Entry {
                    prec,
                    priority: pr,
                    pos,
                    tpl: id,
                    pat,
                };
                let table = &mut self.ss.modes[mode as usize];
                match entry.pat.key() {
                    PatternKey::Element(n) => table.elements.entry(n).or_default().push(entry),
                    PatternKey::AnyElement => table.any_element.push(entry),
                    PatternKey::Attribute(n) => table.attributes.entry(n).or_default().push(entry),
                    PatternKey::AnyAttribute => table.any_attribute.push(entry),
                    PatternKey::Text => table.text.push(entry),
                    PatternKey::Comment => table.comment.push(entry),
                    PatternKey::Pi => table.pi.push(entry),
                    PatternKey::Root => table.root.push(entry),
                    PatternKey::Any => table.any.push(entry),
                }
            }
        }
    }

    fn finish_rules(&mut self) {
        let _ = &self.rules;
        // Best first: precedence, priority, then the last in the stylesheet
        // (section 5.5).
        fn order(v: &mut [Entry]) {
            v.sort_by(|a, b| {
                b.prec
                    .cmp(&a.prec)
                    .then(
                        b.priority
                            .partial_cmp(&a.priority)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(b.pos.cmp(&a.pos))
            });
        }
        for t in &mut self.ss.modes {
            for v in t.elements.values_mut() {
                order(v);
            }
            for v in t.attributes.values_mut() {
                order(v);
            }
            order(&mut t.any_element);
            order(&mut t.any_attribute);
            order(&mut t.text);
            order(&mut t.comment);
            order(&mut t.pi);
            order(&mut t.root);
            order(&mut t.any);
        }
    }

    fn func_def(&mut self, el: NodeId, scope: &Scope) {
        let Some(n) = self.attr(el, "name") else {
            self.error(el, "func:function : missing name attribute");
            return;
        };
        let Some((u, l)) = self.expand(el, n.trim(), scope) else {
            return;
        };
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        let body = self.body_of(&kids, scope, false);
        self.ss.funcs.insert((u, l), FuncDef { body });
    }

    fn attribute_set(&mut self, el: NodeId, scope: &Scope) {
        let Some(n) = self.attr(el, "name") else {
            self.error(el, "xsl:attribute-set : missing name attribute");
            return;
        };
        let Some(name) = self.clark_of(el, &n, scope) else {
            return;
        };
        let u = self.attr(el, "use-attribute-sets");
        let uses = self.sets_attr(el, u, scope);
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        let attrs = self.body_of(&kids, scope, false);
        self.ss
            .attr_sets
            .entry(name)
            .or_default()
            .push(AttrSetDef { uses, attrs });
    }

    fn key(&mut self, el: NodeId, scope: &Scope) {
        let (Some(n), Some(m), Some(u)) = (
            self.attr(el, "name"),
            self.attr(el, "match"),
            self.attr(el, "use"),
        ) else {
            self.error(el, "xsl:key : missing attribute");
            return;
        };
        let Some(name) = self.clark_of(el, &n, scope) else {
            return;
        };
        let Some(pats) = self.pattern_attr(el, &m, scope) else {
            return;
        };
        let Some(use_expr) = self.expr(el, &u, scope) else {
            return;
        };
        self.ss
            .keys
            .entry(name)
            .or_default()
            .push(KeyDef { pats, use_expr });
    }

    fn decimal_format(&mut self, el: NodeId, scope: &Scope) {
        let name = match self.attr(el, "name") {
            Some(n) => self.clark_of(el, &n, scope),
            None => None,
        };
        let mut f = DecimalFormat::default();
        let ch =
            |c: &Self, a: &str, d: char| c.attr(el, a).and_then(|v| v.chars().next()).unwrap_or(d);
        f.decimal_separator = ch(self, "decimal-separator", f.decimal_separator);
        f.grouping_separator = ch(self, "grouping-separator", f.grouping_separator);
        f.minus_sign = ch(self, "minus-sign", f.minus_sign);
        f.percent = ch(self, "percent", f.percent);
        f.per_mille = ch(self, "per-mille", f.per_mille);
        f.zero_digit = ch(self, "zero-digit", f.zero_digit);
        f.digit = ch(self, "digit", f.digit);
        f.pattern_separator = ch(self, "pattern-separator", f.pattern_separator);
        if let Some(v) = self.attr(el, "infinity") {
            f.infinity = v;
        }
        if let Some(v) = self.attr(el, "NaN") {
            f.nan = v;
        }
        self.ss.decimal_formats.insert(name, f);
    }

    fn output(&mut self, el: NodeId, scope: &Scope) {
        let mut spec = std::mem::take(&mut self.ss.output);
        self.output_attrs(el, scope, &mut spec);
        self.ss.output = spec;
    }

    fn output_attrs(&mut self, el: NodeId, scope: &Scope, spec: &mut OutputSpec) {
        if let Some(m) = self.attr(el, "method") {
            spec.method = match m.trim() {
                "xml" => Some(OutMethod::Xml),
                "html" => Some(OutMethod::Html),
                "text" => Some(OutMethod::Text),
                "xhtml" => Some(OutMethod::Xhtml),
                other => {
                    if !other.contains(':') {
                        let msg = format!("invalid value for method: {other}");
                        self.warning(el, &msg);
                    }
                    spec.method
                }
            };
        }
        let yes = |c: &Self, a: &str| c.attr(el, a).map(|v| v.trim() == "yes");
        if let Some(v) = self.attr(el, "version") {
            spec.version = Some(v);
        }
        if let Some(v) = self.attr(el, "encoding") {
            spec.encoding = Some(v);
        }
        if let Some(v) = yes(self, "omit-xml-declaration") {
            spec.omit_xml_declaration = Some(v);
        }
        if let Some(v) = yes(self, "standalone") {
            spec.standalone = Some(v);
        }
        if let Some(v) = yes(self, "indent") {
            spec.indent = Some(v);
        }
        if let Some(v) = self.attr(el, "doctype-public") {
            spec.doctype_public = Some(v);
        }
        if let Some(v) = self.attr(el, "doctype-system") {
            spec.doctype_system = Some(v);
        }
        if let Some(v) = self.attr(el, "media-type") {
            spec.media_type = Some(v);
        }
        if let Some(v) = self.attr(el, "cdata-section-elements") {
            for t in tokens(&v) {
                let name = if t.contains(':') {
                    self.clark_of(el, t, scope).map(|s| s.to_string())
                } else {
                    // Unprefixed names use the default namespace here
                    // (section 16.1).
                    let d = scope.ns.get("").map_or("", String::as_str);
                    Some(clark(d, t))
                };
                if let Some(n) = name {
                    spec.cdata_elements.push(n);
                }
            }
        }
    }

    // ----- instructions -----------------------------------------------------

    /// Compiles sibling nodes of a sequence constructor; `top` for the
    /// direct children of xsl:template.
    fn body_of(&mut self, nodes: &[NodeId], scope: &Scope, top: bool) -> Vec<Instr> {
        let mut out = Vec::new();
        let mut text = String::new();
        let mut text_line = 0;
        for &n in nodes {
            match self.arena.data(n) {
                NodeData::Text(t) | NodeData::CData(t) => {
                    if text.is_empty() {
                        text_line = self.arena.line(n);
                    }
                    text.push_str(t);
                }
                NodeData::EntityRef(_) => {
                    text.push_str(&self.arena.text_content(n));
                }
                NodeData::Element(_) => {
                    self.flush_text(&mut text, text_line, scope, &mut out);
                    if let Some(i) = self.instruction(n, scope, top) {
                        out.push(i);
                    }
                }
                _ => {
                    self.flush_text(&mut text, text_line, scope, &mut out);
                }
            }
        }
        self.flush_text(&mut text, text_line, scope, &mut out);
        out
    }

    fn flush_text(&mut self, text: &mut String, line: u32, scope: &Scope, out: &mut Vec<Instr>) {
        if text.is_empty() {
            return;
        }
        // Section 3.4: whitespace-only text nodes are stripped.
        if !scope.preserve && is_ws(text) {
            text.clear();
            return;
        }
        out.push(Instr {
            kind: Kind::Text {
                text: std::mem::take(text),
                doe: false,
            },
            line,
            module: self.module,
            elem: "text".into(),
        });
    }

    fn children_body(&mut self, el: NodeId, scope: &Scope) -> Vec<Instr> {
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        self.body_of(&kids, scope, false)
    }

    fn instruction(&mut self, el: NodeId, outer: &Scope, top: bool) -> Option<Instr> {
        let q = self.arena.qname(el)?.clone();
        let line = self.arena.line(el);
        let module = self.module;
        let is_xsl = q.ns_str() == XSL_NS;
        let scope = self.enter(el, outer, is_xsl);
        let kind = if is_xsl {
            self.xsl_instruction(el, &q.local, &scope)?
        } else if scope.ext.iter().any(|u| &**u == q.ns_str()) {
            self.extension(el, &q, &scope)?
        } else {
            self.literal(el, &q, outer, &scope, top)?
        };
        Some(Instr {
            kind,
            line,
            module,
            elem: q.local.clone(),
        })
    }

    fn with_params(&mut self, el: NodeId, scope: &Scope) -> Vec<WithParam> {
        let mut out = Vec::new();
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        for k in kids {
            if self.is_xsl(k, "with-param") {
                let s = self.enter(k, scope, true);
                if let Some(name) = self.var_name_attr(k, &s) {
                    let value = self.value_src(k, &s);
                    out.push(WithParam { name, value });
                }
            }
        }
        out
    }

    fn sorts(&mut self, el: NodeId, scope: &Scope) -> Vec<SortKey> {
        let mut out = Vec::new();
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        for k in kids {
            if self.is_xsl(k, "sort") {
                let s = self.enter(k, scope, true);
                let text = self.attr(k, "select").unwrap_or_else(|| ".".into());
                let Some(select) = self.expr(k, &text, &s) else {
                    continue;
                };
                let order = self.opt_avt(k, "order", &s);
                let data_type = self.opt_avt(k, "data-type", &s);
                let case_order = self.opt_avt(k, "case-order", &s);
                out.push(SortKey {
                    select,
                    order,
                    data_type,
                    case_order,
                });
            }
        }
        out
    }

    /// The children other than `xsl:sort` (`xsl:for-each`).
    fn body_without_sorts(&mut self, el: NodeId, scope: &Scope) -> Vec<Instr> {
        let kids: Vec<NodeId> = self
            .arena
            .children(el)
            .filter(|&k| !self.is_xsl(k, "sort"))
            .collect();
        self.body_of(&kids, scope, false)
    }

    fn xsl_instruction(&mut self, el: NodeId, local: &str, scope: &Scope) -> Option<Kind> {
        Some(match local {
            "text" => {
                let mut text = String::new();
                let kids: Vec<NodeId> = self.arena.children(el).collect();
                for k in kids {
                    match self.arena.data(k) {
                        NodeData::Text(t) | NodeData::CData(t) => text.push_str(t),
                        NodeData::EntityRef(_) => text.push_str(&self.arena.text_content(k)),
                        NodeData::Comment(_) | NodeData::Pi { .. } => {}
                        _ => {
                            self.error(el, "xsl:text content problem");
                            return None;
                        }
                    }
                }
                let doe = self.attr(el, "disable-output-escaping").as_deref() == Some("yes");
                Kind::Text { text, doe }
            }
            "value-of" => {
                let select = self.req_expr(el, "select", scope)?;
                let doe = self.attr(el, "disable-output-escaping").as_deref() == Some("yes");
                Kind::ValueOf { select, doe }
            }
            "apply-templates" => {
                let select = match self.attr(el, "select") {
                    Some(t) => Some(self.expr(el, &t, scope)?),
                    None => None,
                };
                let mode_name = match self.attr(el, "mode") {
                    Some(m) => self.clark_of(el, &m, scope),
                    None => None,
                };
                let mode = self.mode_id(mode_name);
                let sorts = self.sorts(el, scope);
                let params = self.with_params(el, scope);
                Kind::ApplyTemplates {
                    select,
                    mode,
                    sorts,
                    params,
                }
            }
            "call-template" => {
                let Some(n) = self.attr(el, "name") else {
                    self.error(el, "xsl:call-template : missing name attribute");
                    return None;
                };
                let name = self.clark_of(el, &n, scope)?;
                let params = self.with_params(el, scope);
                Kind::CallTemplate { name, params }
            }
            "apply-imports" => Kind::ApplyImports,
            "for-each" => {
                let select = self.req_expr(el, "select", scope)?;
                let sorts = self.sorts(el, scope);
                let body = self.body_without_sorts(el, scope);
                Kind::ForEach {
                    select,
                    sorts,
                    body,
                }
            }
            "if" => {
                let test = self.req_expr(el, "test", scope)?;
                let body = self.children_body(el, scope);
                Kind::If { test, body }
            }
            "choose" => {
                let mut whens = Vec::new();
                let mut otherwise = Vec::new();
                let kids: Vec<NodeId> = self.arena.children(el).collect();
                for k in kids {
                    if self.is_xsl(k, "when") {
                        let s = self.enter(k, scope, true);
                        let test = self.req_expr(k, "test", &s)?;
                        let body = self.children_body(k, &s);
                        whens.push((test, body));
                    } else if self.is_xsl(k, "otherwise") {
                        let s = self.enter(k, scope, true);
                        otherwise = self.children_body(k, &s);
                    }
                }
                Kind::Choose { whens, otherwise }
            }
            "variable" | "param" => {
                let name = self.var_name_attr(el, scope)?;
                let value = self.value_src(el, scope);
                Kind::Var {
                    name,
                    value,
                    is_param: local == "param",
                }
            }
            "attribute" => {
                let Some(n) = self.attr(el, "name") else {
                    self.error(el, "xsl:attribute : missing name attribute");
                    return None;
                };
                let name = self.avt(el, &n, scope)?;
                let ns = self.opt_avt(el, "namespace", scope);
                let body = self.children_body(el, scope);
                Kind::Attribute {
                    name,
                    ns,
                    body,
                    scope: scope.ns.clone(),
                }
            }
            "element" => {
                let Some(n) = self.attr(el, "name") else {
                    self.error(el, "xsl:element : missing name attribute");
                    return None;
                };
                let name = self.avt(el, &n, scope)?;
                let ns = self.opt_avt(el, "namespace", scope);
                let u = self.attr(el, "use-attribute-sets");
                let sets = self.sets_attr(el, u, scope);
                let body = self.children_body(el, scope);
                Kind::Element {
                    name,
                    ns,
                    sets,
                    body,
                    scope: scope.ns.clone(),
                }
            }
            "comment" => Kind::Comment {
                body: self.children_body(el, scope),
            },
            "processing-instruction" => {
                let Some(n) = self.attr(el, "name") else {
                    self.error(el, "xsl:processing-instruction : missing name attribute");
                    return None;
                };
                let name = self.avt(el, &n, scope)?;
                let body = self.children_body(el, scope);
                Kind::Pi { name, body }
            }
            "copy" => {
                let u = self.attr(el, "use-attribute-sets");
                let sets = self.sets_attr(el, u, scope);
                let body = self.children_body(el, scope);
                Kind::Copy { sets, body }
            }
            "copy-of" => Kind::CopyOf {
                select: self.req_expr(el, "select", scope)?,
            },
            "number" => self.number(el, scope)?,
            "message" => {
                let terminate = self.opt_avt(el, "terminate", scope);
                let body = self.children_body(el, scope);
                Kind::Message { body, terminate }
            }
            "document" => self.document_element(el, scope)?,
            "fallback" => {
                // Section 15: only for instructions that could not run.
                return None;
            }
            "sort" | "with-param" | "when" | "otherwise" => {
                let msg = format!("xsl:{local} : misplaced element");
                self.error(el, &msg);
                return None;
            }
            other => {
                if self.version_10 {
                    let msg = format!("xsltStylePreCompute: unknown xsl:{other}");
                    self.error(el, &msg);
                    return None;
                }
                // Forwards-compatible processing (section 2.5).
                let fallback = self.fallback_of(el, scope);
                Kind::Unknown {
                    name: format!("xsl:{other}"),
                    fallback,
                }
            }
        })
    }

    fn fallback_of(&mut self, el: NodeId, scope: &Scope) -> Option<Vec<Instr>> {
        let mut found = None;
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        for k in kids {
            if self.is_xsl(k, "fallback") {
                let s = self.enter(k, scope, true);
                let b = self.children_body(k, &s);
                found.get_or_insert_with(Vec::new).extend(b);
            }
        }
        found
    }

    fn number(&mut self, el: NodeId, scope: &Scope) -> Option<Kind> {
        let value = match self.attr(el, "value") {
            Some(t) => Some(self.expr(el, &t, scope)?),
            None => None,
        };
        let level = match self.attr(el, "level").as_deref() {
            None | Some("single") => 0,
            Some("multiple") => 1,
            Some("any") => 2,
            Some(other) => {
                self.error(el, &format!("xsl:number : invalid value {other} for level"));
                return None;
            }
        };
        let count = match self.attr(el, "count") {
            Some(t) => Some(self.pattern_attr(el, &t, scope)?),
            None => None,
        };
        let from = match self.attr(el, "from") {
            Some(t) => Some(self.pattern_attr(el, &t, scope)?),
            None => None,
        };
        let format = match self.opt_avt(el, "format", scope) {
            Some(a) => a,
            None => Avt(vec![AvtPart::Lit("1".into())]),
        };
        let group_sep = self.opt_avt(el, "grouping-separator", scope);
        let group_size = self.opt_avt(el, "grouping-size", scope);
        Some(Kind::Number(Box::new(NumberSpec {
            value,
            level,
            count,
            from,
            format,
            group_sep,
            group_size,
        })))
    }

    fn document_element(&mut self, el: NodeId, scope: &Scope) -> Option<Kind> {
        let t = match self.attr(el, "href").or_else(|| self.attr(el, "file")) {
            Some(t) => t,
            None => {
                self.error(el, "xsltDocumentComp: href attribute is missing");
                return None;
            }
        };
        let href = self.avt(el, &t, scope)?;
        let mut output = OutputSpec::default();
        self.output_attrs(el, scope, &mut output);
        let body = self.children_body(el, scope);
        Some(Kind::Document(Box::new(DocumentSpec {
            href,
            output,
            body,
        })))
    }

    fn extension(&mut self, el: NodeId, q: &QName, scope: &Scope) -> Option<Kind> {
        if is_document_element(q.ns_str(), &q.local) {
            return self.document_element(el, scope);
        }
        if q.ns_str() == EXSLT_FUNCTIONS && &*q.local == "result" {
            let value = self.value_src(el, scope);
            return Some(Kind::FuncResult { value });
        }
        let fallback = self.fallback_of(el, scope);
        Some(Kind::Unknown {
            name: q.qualified(),
            fallback,
        })
    }

    fn literal(
        &mut self,
        el: NodeId,
        q: &QName,
        outer: &Scope,
        scope: &Scope,
        top: bool,
    ) -> Option<Kind> {
        let excluded = |u: &str| {
            u == XSL_NS
                || u == XML_NS
                || scope.exclude.iter().any(|x| x == u)
                || scope.ext.iter().any(|x| x == u)
        };
        // The namespace nodes to copy (section 7.1.1): all in scope but the
        // excluded ones for a direct child of xsl:template; elsewhere only
        // what this element declares (tests bug69168.phpt, bug70078.phpt).
        let mut decls: Vec<(Option<Str>, Str)> = Vec::new();
        for (p, u) in scope.decls.iter() {
            if excluded(u) {
                continue;
            }
            if !top && outer.ns.get(p) == Some(u) {
                continue;
            }
            let (p2, u2) = match self.aliases.get(u) {
                Some((np, nu)) => (np.clone().unwrap_or_default(), nu.clone()),
                None => (p.clone(), u.clone()),
            };
            let prefix = if p2.is_empty() {
                None
            } else {
                Some(p2.as_str().into())
            };
            decls.push((prefix, u2.as_str().into()));
        }
        let mut name = q.clone();
        if let Some(ns) = &q.ns
            && let Some((np, nu)) = self.aliases.get(&**ns)
        {
            name.prefix = np.as_deref().map(Into::into);
            name.ns = if nu.is_empty() {
                None
            } else {
                Some(nu.as_str().into())
            };
        }
        let mut attrs = Vec::new();
        let mut sets = Vec::new();
        let list: Vec<NodeId> = self.arena.attrs(el).collect();
        for a in list {
            let ad = self.arena.attr(a).unwrap();
            let an = ad.name.clone();
            let ns = an.ns.as_deref().unwrap_or("");
            if ns == XMLNS_NS {
                continue;
            }
            if ns == XSL_NS {
                if &*an.local == "use-attribute-sets" {
                    let v = self.arena.attr_value(a);
                    sets = self.sets_attr(el, Some(v), scope);
                }
                continue;
            }
            let v = self.arena.attr_value(a);
            let avt = self.avt(el, &v, scope)?;
            let mut out_name = an.clone();
            if let Some(n) = &an.ns
                && let Some((np, nu)) = self.aliases.get(&**n)
            {
                out_name.prefix = np.as_deref().map(Into::into);
                out_name.ns = Some(nu.as_str().into());
            }
            attrs.push((out_name, avt));
        }
        let kids: Vec<NodeId> = self.arena.children(el).collect();
        let body = self.body_of(&kids, scope, false);
        Some(Kind::Lre(Box::new(Lre {
            name,
            decls,
            attrs,
            sets,
            body,
        })))
    }
}
