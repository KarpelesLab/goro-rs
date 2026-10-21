//! W3C XML Schema (XSD 1.0) structures: schema parsing/construction and
//! instance validation, ported from libxml2 2.13 xmlschemas.c.
//!
//! API:
//! - `Schema::parse_file(vm, url)` / `Schema::parse_memory(vm, data)`
//!   compile a schema (`(Option<Rc<Schema>>, Vec<XmlError>)`; errors are to
//!   be reported by the caller, a schema is only returned when there were no
//!   errors);
//! - `ValidCtxt::new(schema)`, `set_options(XML_SCHEMA_VAL_VC_I_CREATE)`,
//!   then `validate_doc(doc)` / `validate_one_element(elem)` (DOM walk) or the
//!   streaming interface `start_element` / `characters` / `end_element`;
//!   errors are collected in `ValidCtxt::errors`.

use crate::parser::{ERR_ERROR, ERR_WARNING, XmlError};
use crate::tree::{self, NType, NodeId};
use crate::xmlerrors as E;
use crate::xmlregexp::{Automata, ExecCtxt, Regexp, StateId};
use crate::xmlschemastypes::{self as T, Facet, Val};
use std::collections::HashMap;
use std::rc::Rc;

pub mod fixup;
pub mod parse;
pub mod parse_comp;
pub mod parse_doc;
pub mod parse_types;
pub mod simple;
pub mod valid;

pub use valid::ValidCtxt;

pub const XS_NS: &str = "http://www.w3.org/2001/XMLSchema";
pub const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

pub const UNBOUNDED: i32 = 1 << 30;

/// xmlSchemaValidOption XML_SCHEMA_VAL_VC_I_CREATE
pub const VAL_VC_I_CREATE: i32 = 1;

// xmlSchemaTypeType
pub const TYPE_BASIC: i32 = 1;
pub const TYPE_ANY: i32 = 2;
pub const TYPE_FACET: i32 = 3;
pub const TYPE_SIMPLE: i32 = 4;
pub const TYPE_COMPLEX: i32 = 5;
pub const TYPE_SEQUENCE: i32 = 6;
pub const TYPE_CHOICE: i32 = 7;
pub const TYPE_ALL: i32 = 8;
pub const TYPE_SIMPLE_CONTENT: i32 = 9;
pub const TYPE_COMPLEX_CONTENT: i32 = 10;
pub const TYPE_UR: i32 = 11;
pub const TYPE_RESTRICTION: i32 = 12;
pub const TYPE_EXTENSION: i32 = 13;
pub const TYPE_ELEMENT: i32 = 14;
pub const TYPE_ATTRIBUTE: i32 = 15;
pub const TYPE_ATTRIBUTEGROUP: i32 = 16;
pub const TYPE_GROUP: i32 = 17;
pub const TYPE_NOTATION: i32 = 18;
pub const TYPE_LIST: i32 = 19;
pub const TYPE_UNION: i32 = 20;
pub const TYPE_ANY_ATTRIBUTE: i32 = 21;
pub const TYPE_IDC_UNIQUE: i32 = 22;
pub const TYPE_IDC_KEY: i32 = 23;
pub const TYPE_IDC_KEYREF: i32 = 24;
pub const TYPE_PARTICLE: i32 = 25;
pub const TYPE_ATTRIBUTE_USE: i32 = 26;
pub const EXTRA_QNAMEREF: i32 = 2000;
pub const EXTRA_ATTR_USE_PROHIB: i32 = 2001;

// xmlSchemaContentType
pub const CONTENT_UNKNOWN: i32 = 0;
pub const CONTENT_EMPTY: i32 = 1;
pub const CONTENT_ELEMENTS: i32 = 2;
pub const CONTENT_MIXED: i32 = 3;
pub const CONTENT_SIMPLE: i32 = 4;
pub const CONTENT_MIXED_OR_ELEMENTS: i32 = 5;
pub const CONTENT_BASIC: i32 = 6;
pub const CONTENT_ANY: i32 = 7;

pub const ANY_SKIP: i32 = 1;
pub const ANY_LAX: i32 = 2;
pub const ANY_STRICT: i32 = 3;

pub const ATTR_USE_PROHIBITED: i32 = 0;
pub const ATTR_USE_REQUIRED: i32 = 1;
pub const ATTR_USE_OPTIONAL: i32 = 2;

// attribute decl flags
pub const ATTR_GLOBAL: i32 = 1 << 0;
pub const ATTR_NSDEFAULT: i32 = 1 << 7;
pub const ATTR_INTERNAL_RESOLVED: i32 = 1 << 8;
pub const ATTR_FIXED: i32 = 1 << 9;
// attribute use flags
pub const ATTR_USE_FIXED: i32 = 1 << 0;

pub const WILDCARD_COMPLETE: i32 = 1 << 0;

pub const ATTRGROUP_WILDCARD_BUILDED: i32 = 1 << 0;
pub const ATTRGROUP_GLOBAL: i32 = 1 << 1;
pub const ATTRGROUP_MARKED: i32 = 1 << 2;
pub const ATTRGROUP_REDEFINED: i32 = 1 << 3;
pub const ATTRGROUP_HAS_REFS: i32 = 1 << 4;

pub const TYPE_MIXED: i32 = 1 << 0;
pub const TYPE_DERIVATION_METHOD_EXTENSION: i32 = 1 << 1;
pub const TYPE_DERIVATION_METHOD_RESTRICTION: i32 = 1 << 2;
pub const TYPE_GLOBAL: i32 = 1 << 3;
pub const TYPE_OWNED_ATTR_WILDCARD: i32 = 1 << 4;
pub const TYPE_VARIETY_ABSENT: i32 = 1 << 5;
pub const TYPE_VARIETY_LIST: i32 = 1 << 6;
pub const TYPE_VARIETY_UNION: i32 = 1 << 7;
pub const TYPE_VARIETY_ATOMIC: i32 = 1 << 8;
pub const TYPE_FINAL_EXTENSION: i32 = 1 << 9;
pub const TYPE_FINAL_RESTRICTION: i32 = 1 << 10;
pub const TYPE_FINAL_LIST: i32 = 1 << 11;
pub const TYPE_FINAL_UNION: i32 = 1 << 12;
pub const TYPE_FINAL_DEFAULT: i32 = 1 << 13;
pub const TYPE_BUILTIN_PRIMITIVE: i32 = 1 << 14;
pub const TYPE_MARKED: i32 = 1 << 16;
pub const TYPE_BLOCK_DEFAULT: i32 = 1 << 17;
pub const TYPE_BLOCK_EXTENSION: i32 = 1 << 18;
pub const TYPE_BLOCK_RESTRICTION: i32 = 1 << 19;
pub const TYPE_ABSTRACT: i32 = 1 << 20;
pub const TYPE_FACETSNEEDVALUE: i32 = 1 << 21;
pub const TYPE_INTERNAL_RESOLVED: i32 = 1 << 22;
pub const TYPE_INTERNAL_INVALID: i32 = 1 << 23;
pub const TYPE_WHITESPACE_PRESERVE: i32 = 1 << 24;
pub const TYPE_WHITESPACE_REPLACE: i32 = 1 << 25;
pub const TYPE_WHITESPACE_COLLAPSE: i32 = 1 << 26;
pub const TYPE_HAS_FACETS: i32 = 1 << 27;
pub const TYPE_NORMVALUENEEDED: i32 = 1 << 28;
pub const TYPE_FIXUP_1: i32 = 1 << 29;
pub const TYPE_REDEFINED: i32 = 1 << 30;

pub const ELEM_NILLABLE: i32 = 1 << 0;
pub const ELEM_GLOBAL: i32 = 1 << 1;
pub const ELEM_DEFAULT: i32 = 1 << 2;
pub const ELEM_FIXED: i32 = 1 << 3;
pub const ELEM_ABSTRACT: i32 = 1 << 4;
pub const ELEM_TOPLEVEL: i32 = 1 << 5;
pub const ELEM_REF: i32 = 1 << 6;
pub const ELEM_NSDEFAULT: i32 = 1 << 7;
pub const ELEM_INTERNAL_RESOLVED: i32 = 1 << 8;
pub const ELEM_CIRCULAR: i32 = 1 << 9;
pub const ELEM_BLOCK_ABSENT: i32 = 1 << 10;
pub const ELEM_BLOCK_EXTENSION: i32 = 1 << 11;
pub const ELEM_BLOCK_RESTRICTION: i32 = 1 << 12;
pub const ELEM_BLOCK_SUBSTITUTION: i32 = 1 << 13;
pub const ELEM_FINAL_ABSENT: i32 = 1 << 14;
pub const ELEM_FINAL_EXTENSION: i32 = 1 << 15;
pub const ELEM_FINAL_RESTRICTION: i32 = 1 << 16;
pub const ELEM_SUBST_GROUP_HEAD: i32 = 1 << 17;
pub const ELEM_INTERNAL_CHECKED: i32 = 1 << 18;

pub const MODEL_GROUP_DEF_MARKED: i32 = 1 << 0;
pub const MODEL_GROUP_DEF_REDEFINED: i32 = 1 << 1;

pub const QUALIF_ELEM: i32 = 1 << 0;
pub const QUALIF_ATTR: i32 = 1 << 1;
pub const FINAL_DEFAULT_EXTENSION: i32 = 1 << 2;
pub const FINAL_DEFAULT_RESTRICTION: i32 = 1 << 3;
pub const FINAL_DEFAULT_LIST: i32 = 1 << 4;
pub const FINAL_DEFAULT_UNION: i32 = 1 << 5;
pub const BLOCK_DEFAULT_EXTENSION: i32 = 1 << 6;
pub const BLOCK_DEFAULT_RESTRICTION: i32 = 1 << 7;
pub const BLOCK_DEFAULT_SUBSTITUTION: i32 = 1 << 8;
pub const INCLUDING_CONVERT_NS: i32 = 1 << 9;

// schema facet whitespace (facet->whitespace)
pub const FACET_UNKNOWN: i32 = 0;
pub const FACET_PRESERVE: i32 = 1;
pub const FACET_REPLACE: i32 = 2;
pub const FACET_COLLAPSE: i32 = 3;

pub const SCHEMA_MAIN: i32 = 0;
pub const SCHEMA_IMPORT: i32 = 1;
pub const SCHEMA_INCLUDE: i32 = 2;
pub const SCHEMA_REDEFINE: i32 = 3;

pub type ItemId = usize;

/// A schema component (or helper item). Like libxml2 all component kinds
/// share one structure; `typ` tells what the item is.
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub typ: i32,
    pub name: Option<String>,
    pub target_ns: Option<String>,
    pub node: Option<NodeId>,
    pub flags: i32,
    // tree items (particles, model groups, model group defs)
    pub next: Option<ItemId>,
    pub children: Option<ItemId>,
    pub min_occurs: i32,
    pub max_occurs: i32,
    // types / element & attribute declarations
    pub subtypes: Option<ItemId>,
    pub content_type: i32,
    pub base: Option<String>,
    pub base_ns: Option<String>,
    pub base_type: Option<ItemId>,
    pub facets: Vec<SFacet>,
    /// facetSet: (type owning the facet, index in its facets)
    pub facet_set: Vec<(ItemId, usize)>,
    pub attribute_wildcard: Option<ItemId>,
    pub builtin_type: i32,
    pub member_types: Vec<ItemId>,
    pub content_type_def: Option<ItemId>,
    pub cont_model: Option<Rc<Regexp>>,
    /// attribute uses (and, before fixup, prohibitions and attribute group
    /// references)
    pub attr_uses: Vec<ItemId>,
    pub has_attr_uses: bool,
    // element declarations
    pub named_type: Option<String>,
    pub named_type_ns: Option<String>,
    pub subst_group: Option<String>,
    pub subst_group_ns: Option<String>,
    pub value: Option<String>,
    pub ref_decl: Option<ItemId>,
    pub def_val: Option<Val>,
    pub idcs: Vec<ItemId>,
    // attribute declarations / uses
    pub type_name: Option<String>,
    pub type_ns: Option<String>,
    pub def_value: Option<String>,
    pub attr_decl: Option<ItemId>,
    pub occurs: i32,
    // QName references
    pub item: Option<ItemId>,
    pub item_type: i32,
    // prohibitions
    pub is_ref: bool,
    // wildcards
    pub process_contents: i32,
    pub any: bool,
    pub ns_set: Vec<Option<String>>,
    /// negated namespace: Some(None) = not absent ("##other" w/o target ns)
    pub neg_ns_set: Option<Option<String>>,
    // identity constraints
    pub selector: Option<IdcSelect>,
    pub fields: Vec<IdcSelect>,
    pub idc_ref: Option<ItemId>,
}

#[derive(Clone, Debug, Default)]
pub struct IdcSelect {
    pub xpath: String,
    pub index: i32,
}

/// A facet of a simple type (xmlSchemaFacet with its schema node).
#[derive(Clone, Debug)]
pub struct SFacet {
    pub f: Facet,
    pub node: Option<NodeId>,
    /// the computed value as a list (list types)
    pub vals: Option<Vec<Val>>,
}

/// A schema document "bucket".
#[derive(Clone, Debug, Default)]
pub struct Bucket {
    pub typ: i32,
    pub schema_location: Option<String>,
    pub orig_target_ns: Option<String>,
    pub target_ns: Option<String>,
    pub doc: Option<NodeId>,
    pub relations: Vec<Relation>,
    pub located: bool,
    pub parsed: bool,
    pub imported: i32,
    pub globals: Vec<ItemId>,
    pub locals: Vec<ItemId>,
    pub marked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Relation {
    pub typ: i32,
    pub import_ns: Option<String>,
    pub bucket: Option<usize>,
}

/// A compiled schema.
#[derive(Debug, Default)]
pub struct Schema {
    pub items: Vec<Item>,
    pub target_ns: Option<String>,
    pub flags: i32,
    /// global components: (kind, name, namespace) -> item
    pub globals: HashMap<(i32, String, Option<String>), ItemId>,
    pub buckets: Vec<Bucket>,
    /// built-in type value type -> item
    pub builtins: HashMap<i32, ItemId>,
    pub subst_groups: HashMap<(String, Option<String>), Vec<ItemId>>,
    pub has_idcs: bool,
}

pub fn kind_of(typ: i32) -> i32 {
    match typ {
        TYPE_BASIC | TYPE_SIMPLE | TYPE_COMPLEX => TYPE_COMPLEX,
        TYPE_IDC_KEY | TYPE_IDC_UNIQUE | TYPE_IDC_KEYREF => TYPE_IDC_KEY,
        t => t,
    }
}

impl Schema {
    pub fn it(&self, id: ItemId) -> &Item {
        &self.items[id]
    }

    pub fn itm(&mut self, id: ItemId) -> &mut Item {
        &mut self.items[id]
    }

    pub fn add_item(&mut self, it: Item) -> ItemId {
        self.items.push(it);
        self.items.len() - 1
    }

    /// The type item of a built-in type.
    pub fn builtin(&self, vt: i32) -> ItemId {
        self.builtins[&vt]
    }

    /// Create the built-in type definitions (xmlSchemaInitTypes).
    pub fn init_builtins(&mut self) {
        for &(name, vt, _) in T::BUILTINS {
            let mut it = Item {
                typ: TYPE_BASIC,
                name: Some(name.to_string()),
                target_ns: Some(XS_NS.to_string()),
                builtin_type: vt,
                content_type: CONTENT_BASIC,
                min_occurs: 1,
                max_occurs: 1,
                ..Default::default()
            };
            if T::is_primitive(vt) {
                it.flags |= TYPE_BUILTIN_PRIMITIVE;
            }
            match vt {
                T::ANYTYPE | T::ANYSIMPLETYPE => {}
                T::IDREFS | T::NMTOKENS | T::ENTITIES => {
                    it.flags |= TYPE_VARIETY_LIST | TYPE_HAS_FACETS;
                    it.facets.push(SFacet {
                        f: Facet::min_length_one(),
                        node: None,
                        vals: None,
                    });
                }
                _ => it.flags |= TYPE_VARIETY_ATOMIC,
            }
            let id = self.add_item(it);
            self.builtins.insert(vt, id);
        }
        for &(_, vt, base) in T::BUILTINS {
            let id = self.builtins[&vt];
            let b = self.builtins[&base];
            self.items[id].base_type = Some(b);
            if let Some(item) = T::list_item_type(vt) {
                let iid = self.builtins[&item];
                self.items[id].subtypes = Some(iid);
            }
        }
        // xs:anyType: mixed content (sequence of a lax ##any wildcard,
        // minOccurs 0, maxOccurs unbounded) and a lax ##any attribute wildcard
        let any = self.builtins[&T::ANYTYPE];
        self.items[any].content_type = CONTENT_MIXED;
        let wild = self.add_item(Item {
            typ: TYPE_ANY,
            any: true,
            process_contents: ANY_LAX,
            min_occurs: 1,
            max_occurs: 1,
            ..Default::default()
        });
        let p2 = self.add_item(Item {
            typ: TYPE_PARTICLE,
            min_occurs: 0,
            max_occurs: UNBOUNDED,
            children: Some(wild),
            ..Default::default()
        });
        let seq = self.add_item(Item {
            typ: TYPE_SEQUENCE,
            children: Some(p2),
            ..Default::default()
        });
        let p1 = self.add_item(Item {
            typ: TYPE_PARTICLE,
            min_occurs: 1,
            max_occurs: 1,
            children: Some(seq),
            ..Default::default()
        });
        self.items[any].subtypes = Some(p1);
        let awild = self.add_item(Item {
            typ: TYPE_ANY_ATTRIBUTE,
            any: true,
            process_contents: ANY_LAX,
            ..Default::default()
        });
        self.items[any].attribute_wildcard = Some(awild);
    }

    pub fn is_anytype(&self, t: ItemId) -> bool {
        let i = &self.items[t];
        i.typ == TYPE_BASIC && i.builtin_type == T::ANYTYPE
    }

    pub fn is_complex(&self, t: ItemId) -> bool {
        let i = &self.items[t];
        i.typ == TYPE_COMPLEX || i.builtin_type == T::ANYTYPE
    }

    pub fn is_simple(&self, t: ItemId) -> bool {
        let i = &self.items[t];
        i.typ == TYPE_SIMPLE || (i.typ == TYPE_BASIC && i.builtin_type != T::ANYTYPE)
    }

    pub fn is_any_simple_type(&self, t: ItemId) -> bool {
        let i = &self.items[t];
        i.typ == TYPE_BASIC && i.builtin_type == T::ANYSIMPLETYPE
    }

    pub fn is_atomic(&self, t: ItemId) -> bool {
        self.items[t].flags & TYPE_VARIETY_ATOMIC != 0
    }
    pub fn is_list(&self, t: ItemId) -> bool {
        self.items[t].flags & TYPE_VARIETY_LIST != 0
    }
    pub fn is_union(&self, t: ItemId) -> bool {
        self.items[t].flags & TYPE_VARIETY_UNION != 0
    }

    pub fn has_complex_content(&self, t: ItemId) -> bool {
        matches!(
            self.items[t].content_type,
            CONTENT_MIXED | CONTENT_EMPTY | CONTENT_ELEMENTS
        )
    }
    pub fn has_simple_content(&self, t: ItemId) -> bool {
        matches!(self.items[t].content_type, CONTENT_SIMPLE | CONTENT_BASIC)
    }

    /// Global component lookup (WXS_FIND_GLOBAL_ITEM)
    pub fn get_global(&self, kind: i32, name: &str, ns: Option<&str>) -> Option<ItemId> {
        self.globals
            .get(&(kind_of(kind), name.to_string(), ns.map(|s| s.to_string())))
            .copied()
    }

    /// xmlSchemaGetType: built-in types first.
    pub fn get_type(&self, name: &str, ns: Option<&str>) -> Option<ItemId> {
        if ns == Some(XS_NS) {
            if let Some(vt) = T::predefined_type(name, XS_NS) {
                return Some(self.builtins[&vt]);
            }
        }
        self.get_global(TYPE_COMPLEX, name, ns)
    }

    // -----------------------------------------------------------------
    // Component naming for error reports

    /// xmlSchemaItemTypeToStr
    pub fn item_type_to_str(typ: i32) -> &'static str {
        match typ {
            TYPE_BASIC | TYPE_SIMPLE => "simple type definition",
            TYPE_COMPLEX => "complex type definition",
            TYPE_ELEMENT => "element declaration",
            TYPE_ATTRIBUTE_USE => "attribute use",
            TYPE_ATTRIBUTE => "attribute declaration",
            TYPE_GROUP => "model group definition",
            TYPE_ATTRIBUTEGROUP => "attribute group definition",
            TYPE_NOTATION => "notation declaration",
            TYPE_SEQUENCE => "model group (sequence)",
            TYPE_CHOICE => "model group (choice)",
            TYPE_ALL => "model group (all)",
            TYPE_PARTICLE => "particle",
            TYPE_IDC_UNIQUE => "unique identity-constraint",
            TYPE_IDC_KEY => "key identity-constraint",
            TYPE_IDC_KEYREF => "keyref identity-constraint",
            TYPE_ANY => "wildcard (any)",
            EXTRA_QNAMEREF => "[helper component] QName reference",
            EXTRA_ATTR_USE_PROHIB => "[helper component] attribute use prohibition",
            _ => "Not a schema component",
        }
    }

    /// xmlSchemaGetComponentTypeStr
    pub fn component_type_str(&self, item: ItemId) -> &'static str {
        let i = &self.items[item];
        if i.typ == TYPE_BASIC {
            if self.is_complex(item) {
                return "complex type definition";
            }
            return "simple type definition";
        }
        Self::item_type_to_str(i.typ)
    }

    /// xmlSchemaGetComponentNode
    pub fn component_node(&self, item: ItemId) -> Option<NodeId> {
        let i = &self.items[item];
        match i.typ {
            TYPE_ELEMENT | TYPE_ATTRIBUTE | TYPE_COMPLEX | TYPE_SIMPLE | TYPE_ANY
            | TYPE_ANY_ATTRIBUTE | TYPE_PARTICLE | TYPE_SEQUENCE | TYPE_CHOICE | TYPE_ALL
            | TYPE_GROUP | TYPE_ATTRIBUTEGROUP | TYPE_IDC_UNIQUE | TYPE_IDC_KEY
            | TYPE_IDC_KEYREF | EXTRA_QNAMEREF | TYPE_ATTRIBUTE_USE => i.node,
            _ => None,
        }
    }

    /// xmlSchemaGetComponentName
    pub fn component_name(&self, item: ItemId) -> Option<String> {
        let i = &self.items[item];
        match i.typ {
            TYPE_ATTRIBUTE_USE => i.attr_decl.and_then(|d| self.component_name(d)),
            TYPE_ELEMENT | TYPE_ATTRIBUTE | TYPE_ATTRIBUTEGROUP | TYPE_BASIC | TYPE_SIMPLE
            | TYPE_COMPLEX | TYPE_GROUP | TYPE_IDC_KEY | TYPE_IDC_UNIQUE | TYPE_IDC_KEYREF
            | EXTRA_QNAMEREF | TYPE_NOTATION => i.name.clone(),
            _ => None,
        }
    }

    /// xmlSchemaGetComponentTargetNs
    pub fn component_target_ns(&self, item: ItemId) -> Option<String> {
        let i = &self.items[item];
        match i.typ {
            TYPE_BASIC => Some(XS_NS.to_string()),
            TYPE_ATTRIBUTE_USE => i.attr_decl.and_then(|d| self.component_target_ns(d)),
            TYPE_ELEMENT | TYPE_ATTRIBUTE | TYPE_ATTRIBUTEGROUP | TYPE_SIMPLE | TYPE_COMPLEX
            | TYPE_GROUP | TYPE_IDC_KEY | TYPE_IDC_UNIQUE | TYPE_IDC_KEYREF | EXTRA_QNAMEREF
            | TYPE_NOTATION => i.target_ns.clone(),
            _ => None,
        }
    }

    /// xmlSchemaGetComponentQName
    pub fn component_qname(&self, item: ItemId) -> String {
        format_qname(
            self.component_target_ns(item).as_deref(),
            self.component_name(item).as_deref(),
        )
    }

    /// xmlSchemaGetComponentDesignation
    pub fn component_designation(&self, item: ItemId) -> String {
        format!(
            "{} '{}'",
            self.component_type_str(item),
            self.component_qname(item)
        )
    }

    /// xmlSchemaFormatItemForReport
    pub fn format_item_for_report(
        &self,
        item_des: Option<&str>,
        item: Option<ItemId>,
        item_node: Option<NodeId>,
    ) -> String {
        let mut buf = String::new();
        let mut named = true;
        if let Some(d) = item_des {
            buf.push_str(d);
        } else if let Some(id) = item {
            let it = &self.items[id];
            match it.typ {
                TYPE_BASIC => {
                    if self.is_atomic(id) {
                        buf.push_str("atomic type 'xs:");
                    } else if self.is_list(id) {
                        buf.push_str("list type 'xs:");
                    } else if self.is_union(id) {
                        buf.push_str("union type 'xs:");
                    } else {
                        buf.push_str("simple type 'xs:");
                    }
                    buf.push_str(it.name.as_deref().unwrap_or(""));
                    buf.push('\'');
                }
                TYPE_SIMPLE => {
                    let global = it.flags & TYPE_GLOBAL != 0;
                    if !global {
                        buf.push_str("local ");
                    }
                    if self.is_atomic(id) {
                        buf.push_str("atomic type");
                    } else if self.is_list(id) {
                        buf.push_str("list type");
                    } else if self.is_union(id) {
                        buf.push_str("union type");
                    } else {
                        buf.push_str("simple type");
                    }
                    if global {
                        buf.push_str(" '");
                        buf.push_str(it.name.as_deref().unwrap_or(""));
                        buf.push('\'');
                    }
                }
                TYPE_COMPLEX => {
                    let global = it.flags & TYPE_GLOBAL != 0;
                    if !global {
                        buf.push_str("local ");
                    }
                    buf.push_str("complex type");
                    if global {
                        buf.push_str(" '");
                        buf.push_str(it.name.as_deref().unwrap_or(""));
                        buf.push('\'');
                    }
                }
                TYPE_ATTRIBUTE_USE => {
                    buf.push_str("attribute use ");
                    match it.attr_decl {
                        Some(d) => {
                            buf.push('\'');
                            buf.push_str(&self.component_qname(d));
                            buf.push('\'');
                        }
                        None => buf.push_str("(unknown)"),
                    }
                }
                TYPE_ATTRIBUTE => {
                    buf.push_str("attribute decl. '");
                    buf.push_str(&format_qname(it.target_ns.as_deref(), it.name.as_deref()));
                    buf.push('\'');
                }
                TYPE_ATTRIBUTEGROUP => buf.push_str(&self.component_designation(id)),
                TYPE_ELEMENT => {
                    buf.push_str("element decl. '");
                    buf.push_str(&format_qname(it.target_ns.as_deref(), it.name.as_deref()));
                    buf.push('\'');
                }
                TYPE_IDC_UNIQUE | TYPE_IDC_KEY | TYPE_IDC_KEYREF => {
                    buf.push_str(match it.typ {
                        TYPE_IDC_UNIQUE => "unique '",
                        TYPE_IDC_KEY => "key '",
                        _ => "keyRef '",
                    });
                    buf.push_str(it.name.as_deref().unwrap_or(""));
                    buf.push('\'');
                }
                TYPE_ANY | TYPE_ANY_ATTRIBUTE => {
                    buf.push_str(wildcard_pc_to_string(it.process_contents));
                    buf.push_str(" wildcard");
                }
                TYPE_GROUP => {
                    buf.push_str("model group def. '");
                    buf.push_str(&self.component_qname(id));
                    buf.push('\'');
                }
                TYPE_SEQUENCE | TYPE_CHOICE | TYPE_ALL | TYPE_PARTICLE => {
                    buf.push_str(self.component_type_str(id))
                }
                TYPE_NOTATION => {
                    // libxml2 builds the designation and then falls through
                    // to the unnamed case
                    named = false;
                }
                _ => named = false,
            }
        } else {
            named = false;
        }
        if !named {
            if let Some(n) = item_node {
                buf = String::new();
                let (is_attr, elem) = tree::with(|t| {
                    let is_attr = t.nodes[n].t == NType::Attribute;
                    (
                        is_attr,
                        if is_attr {
                            t.nodes[n].parent.unwrap_or(n)
                        } else {
                            n
                        },
                    )
                });
                let _ = is_attr;
                buf.push_str("Element '");
                buf.push_str(&node_qname(elem));
                buf.push('\'');
            }
        }
        if let Some(n) = item_node {
            if node_type(n) == NType::Attribute {
                buf.push_str(", attribute '");
                buf.push_str(&node_qname(n));
                buf.push('\'');
            }
        }
        buf
    }

    /// xmlSchemaGetWhiteSpaceFacetValue
    pub fn whitespace_facet_value(&self, t: ItemId) -> i32 {
        let it = &self.items[t];
        if it.typ == TYPE_BASIC {
            match it.builtin_type {
                T::STRING | T::ANYSIMPLETYPE => return T::WS_PRESERVE,
                T::NORMSTRING => return T::WS_REPLACE,
                _ => return T::WS_COLLAPSE,
            }
        }
        if self.is_list(t) {
            return T::WS_COLLAPSE;
        }
        if self.is_union(t) {
            return T::WS_UNKNOWN;
        }
        if self.is_atomic(t) {
            if it.flags & TYPE_WHITESPACE_PRESERVE != 0 {
                return T::WS_PRESERVE;
            } else if it.flags & TYPE_WHITESPACE_REPLACE != 0 {
                return T::WS_REPLACE;
            }
            return T::WS_COLLAPSE;
        }
        -1
    }

    /// xmlSchemaFormatFacetEnumSet
    pub fn format_facet_enum_set(&self, typ: ItemId) -> String {
        let mut buf = String::new();
        let mut t = Some(typ);
        let mut found = false;
        while let Some(ty) = t {
            if self.items[ty].typ == TYPE_BASIC {
                break;
            }
            let ws = self.items[ty]
                .base_type
                .map(|b| self.whitespace_facet_value(b))
                .unwrap_or(T::WS_UNKNOWN);
            for f in &self.items[ty].facets {
                if f.f.typ != T::FACET_ENUMERATION {
                    continue;
                }
                found = true;
                let v = match &f.f.val {
                    Some(v) => canon_value_whtsp_ext(v, ws),
                    None => String::new(),
                };
                if buf.is_empty() {
                    buf.push('\'');
                } else {
                    buf.push_str(", '");
                }
                buf.push_str(&v);
                buf.push('\'');
            }
            if found {
                break;
            }
            t = self.items[ty].base_type;
        }
        buf
    }

    /// xmlSchemaIsGlobalItem
    pub fn is_global_item(&self, item: ItemId) -> bool {
        let i = &self.items[item];
        match i.typ {
            TYPE_COMPLEX | TYPE_SIMPLE => i.flags & TYPE_GLOBAL != 0,
            TYPE_GROUP => true,
            TYPE_ELEMENT => i.flags & ELEM_GLOBAL != 0,
            TYPE_ATTRIBUTE => i.flags & ATTR_GLOBAL != 0,
            _ => true,
        }
    }

    /// The "the [local ]atomic type 'xs:int'" part of simple type errors.
    pub fn simple_type_designation(&self, t: ItemId) -> String {
        let mut msg = String::new();
        let global = self.is_global_item(t);
        msg.push_str(if global { "the " } else { "the local " });
        if self.is_atomic(t) {
            msg.push_str("atomic type");
        } else if self.is_list(t) {
            msg.push_str("list type");
        } else if self.is_union(t) {
            msg.push_str("union type");
        }
        if global {
            let it = &self.items[t];
            msg.push_str(" '");
            if it.builtin_type != 0 {
                msg.push_str("xs:");
                msg.push_str(it.name.as_deref().unwrap_or(""));
            } else {
                msg.push_str(&format_qname(it.target_ns.as_deref(), it.name.as_deref()));
            }
            msg.push('\'');
        }
        msg
    }
}

/// xmlSchemaGetCanonValueWhtspExt
pub fn canon_value_whtsp_ext(val: &Val, ws: i32) -> String {
    match val.typ {
        T::STRING | T::NORMSTRING | T::ANYSIMPLETYPE => {
            let s = val.str_value().unwrap_or("");
            let r = if ws == T::WS_COLLAPSE {
                T::collapse_string(s)
            } else if ws == T::WS_REPLACE {
                T::whitespace_replace(s)
            } else {
                None
            };
            r.unwrap_or_else(|| s.to_string())
        }
        _ => T::get_canon_value(val).1,
    }
}

pub fn wildcard_pc_to_string(pc: i32) -> &'static str {
    match pc {
        ANY_SKIP => "skip",
        ANY_LAX => "lax",
        ANY_STRICT => "strict",
        _ => "invalid process contents",
    }
}

/// xmlSchemaFacetTypeToString
pub fn facet_type_to_string(t: i32) -> &'static str {
    match t {
        T::FACET_PATTERN => "pattern",
        T::FACET_MAXEXCLUSIVE => "maxExclusive",
        T::FACET_MAXINCLUSIVE => "maxInclusive",
        T::FACET_MINEXCLUSIVE => "minExclusive",
        T::FACET_MININCLUSIVE => "minInclusive",
        T::FACET_WHITESPACE => "whiteSpace",
        T::FACET_ENUMERATION => "enumeration",
        T::FACET_LENGTH => "length",
        T::FACET_MAXLENGTH => "maxLength",
        T::FACET_MINLENGTH => "minLength",
        T::FACET_TOTALDIGITS => "totalDigits",
        T::FACET_FRACTIONDIGITS => "fractionDigits",
        _ => "Internal Error",
    }
}

/// xmlSchemaFormatQName
pub fn format_qname(ns: Option<&str>, local: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(n) = ns {
        s.push('{');
        s.push_str(n);
        s.push('}');
    }
    match local {
        Some(l) => s.push_str(l),
        None => s.push_str("(NULL)"),
    }
    s
}

// ---------------------------------------------------------------------------
// Tree helpers (schema documents and instances live in the global tree)

pub fn node_type(n: NodeId) -> NType {
    tree::with(|t| t.nodes[n].t)
}

pub fn node_name(n: NodeId) -> String {
    tree::with(|t| t.nodes[n].name.clone())
}

pub fn node_ns(n: NodeId) -> Option<String> {
    tree::with(|t| t.ns_href(t.nodes[n].ns).map(|s| s.to_string()))
}

pub fn node_qname(n: NodeId) -> String {
    tree::with(|t| format_qname(t.ns_href(t.nodes[n].ns), Some(&t.nodes[n].name)))
}

pub fn node_parent(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].parent)
}

pub fn node_children(n: NodeId) -> Vec<NodeId> {
    tree::with(|t| t.children_of(n))
}

pub fn node_content(n: NodeId) -> String {
    tree::with(|t| t.node_get_content(n))
}

/// Is `n` an element `xs:<name>`? (IS_SCHEMA)
pub fn is_schema(n: Option<NodeId>, name: &str) -> bool {
    match n {
        None => false,
        Some(n) => tree::with(|t| {
            let nd = &t.nodes[n];
            nd.t == NType::Element && nd.name == name && t.ns_href(nd.ns) == Some(XS_NS)
        }),
    }
}

/// xmlSchemaGetPropNode: attribute in no namespace
pub fn get_prop_node(n: NodeId, name: &str) -> Option<NodeId> {
    tree::with(|t| {
        t.attrs_of(n)
            .into_iter()
            .find(|&a| t.nodes[a].ns.is_none() && t.nodes[a].name == name)
    })
}

/// xmlSchemaGetPropNodeNs
pub fn get_prop_node_ns(n: NodeId, uri: &str, name: &str) -> Option<NodeId> {
    tree::with(|t| {
        t.attrs_of(n).into_iter().find(|&a| {
            t.nodes[a].ns.is_some()
                && t.nodes[a].name == name
                && t.ns_href(t.nodes[a].ns) == Some(uri)
        })
    })
}

/// xmlSchemaGetProp (xmlGetNoNsProp)
pub fn get_prop(n: NodeId, name: &str) -> Option<String> {
    let a = get_prop_node(n, name)?;
    Some(node_content(a))
}

pub fn attrs_of(n: NodeId) -> Vec<NodeId> {
    tree::with(|t| t.attrs_of(n))
}

/// xmlSearchNs from the context of `n` (the element for attributes)
pub fn search_ns(n: NodeId, prefix: Option<&str>) -> Option<String> {
    if prefix == Some("xml") {
        return Some(crate::xmlschema::parse::XML_XML_NAMESPACE.to_string());
    }
    tree::with(|t| {
        let ns = t.search_ns_ro(n, prefix)?;
        Some(t.nss[ns].href.clone())
    })
}

/// File and line libxml2 attaches to an error raised on `node`.
pub fn node_location(n: NodeId) -> (Option<String>, i32) {
    tree::with(|t| {
        let mut cur = Some(n);
        let mut file = None;
        if let Some(d) = t.nodes[n].doc {
            file = t.doc_info(d).and_then(|i| i.url.clone());
        }
        let mut i = 0;
        while let Some(c) = cur {
            if t.nodes[c].t == NType::Element || i >= 10 {
                break;
            }
            cur = t.nodes[c].parent;
            i += 1;
        }
        let line = match cur {
            Some(c) if t.nodes[c].t == NType::Element => t.nodes[c].line as i32,
            _ => 0,
        };
        (file, line)
    })
}

/// Build an XmlError like xmlVRaiseError does for the schema modules.
pub fn mk_error(
    domain: i32,
    code: i32,
    level: i32,
    msg: String,
    node: Option<NodeId>,
    file: Option<String>,
    line: i32,
) -> XmlError {
    let (f, l) = match node {
        Some(n) => {
            let (nf, nl) = node_location(n);
            (file.or(nf), if line != 0 { line } else { nl })
        }
        None => (file, line),
    };
    let mut message = msg;
    if !message.ends_with('\n') {
        message.push('\n');
    }
    let formatted = vec![message.trim_end_matches('\n').to_string()];
    XmlError {
        domain,
        code,
        level,
        message,
        file: f,
        line: l,
        col: 0,
        formatted,
    }
}

pub fn err_level_error() -> i32 {
    ERR_ERROR
}

pub fn err_level_warning() -> i32 {
    ERR_WARNING
}

/// Keep some imports used by the submodules.
#[allow(dead_code)]
fn _uses(_: Option<ExecCtxt>, _: Option<Automata>, _: Option<StateId>) -> i32 {
    E::SCHEMAP_INTERNAL
}
