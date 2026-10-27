//! Validation of identity constraints (xs:unique / xs:key / xs:keyref),
//! following xmlschemas.c: IDC matchers registered on the scope element,
//! XPath state objects fed with the element/attribute stream, key
//! sequences collected per target node, node tables bubbled up to the
//! keyrefs that need them.

use std::collections::HashMap;

use super::super::simple::{VList, values_equal};
use super::super::*;
use super::{Active, NODE_INFO_VALUE_NEEDED, ValidCtxt};
use crate::pattern::{ATTRIBUTE_NODE, ELEMENT_NODE, StreamCtxt};
use crate::xmlschemastypes as T;

const STO_SELECTOR: i32 = 1;
const STO_FIELD: i32 = 2;

/// xmlSchemaPSVIIDCKey
struct Key {
    typ: ItemId,
    val: VList,
}

/// xmlSchemaPSVIIDCNode
struct IdcNode {
    keys: Vec<usize>,
    line: i32,
    local: String,
    ns: Option<String>,
}

/// xmlSchemaPSVIIDCBinding
pub(super) struct Binding {
    def: ItemId,
    nodes: Vec<usize>,
    dupls: Vec<usize>,
}

/// xmlSchemaIDCAug
struct Aidc {
    def: ItemId,
    keyref_depth: i32,
}

/// xmlSchemaIDCMatcher
#[derive(Default)]
struct Matcher {
    depth: i32,
    aidc: usize,
    idc_type: i32,
    key_seqs: Vec<Option<Vec<Option<usize>>>>,
    targets: Vec<usize>,
    htab: HashMap<String, Vec<usize>>,
}

/// xmlSchemaIDCStateObj
struct Sto {
    typ: i32,
    depth: i32,
    matcher: usize,
    /// the IDC and the field index (None: the selector)
    idc: ItemId,
    field: Option<usize>,
    ctxt: StreamCtxt,
    history: Vec<i32>,
}

/// The IDC part of the validation context.
#[derive(Default)]
pub(super) struct IdcState {
    aidcs: Vec<Aidc>,
    has_keyrefs: bool,
    keys: Vec<Key>,
    nodes: Vec<IdcNode>,
    matchers: Vec<Matcher>,
    free_matchers: Vec<usize>,
    stos: Vec<Option<Sto>>,
    /// active state objects, the head (most recently added) first
    active: Vec<usize>,
}

impl IdcState {
    pub(super) fn has_states(&self) -> bool {
        !self.active.is_empty()
    }
}

/// The current info item (vctxt->inode): an element or an attribute.
#[derive(Clone, Copy)]
enum Cur {
    Elem(usize),
    Attr(usize),
}

impl ValidCtxt {
    fn idc_cur(&self) -> Cur {
        match self.active {
            Active::Attr(i) => Cur::Attr(i),
            Active::Elem(i) => Cur::Elem(i),
            Active::None => Cur::Elem(self.depth.max(0) as usize),
        }
    }

    /// xmlSchemaPreRun: augment the IDC definitions.
    pub(super) fn idc_pre_run(&mut self) {
        self.idc = IdcState::default();
        if !self.schema.has_idcs {
            return;
        }
        for (id, it) in self.schema.items.iter().enumerate() {
            if matches!(it.typ, TYPE_IDC_KEY | TYPE_IDC_UNIQUE | TYPE_IDC_KEYREF) {
                self.idc.aidcs.push(Aidc {
                    def: id,
                    keyref_depth: -1,
                });
                if it.typ == TYPE_IDC_KEYREF {
                    self.idc.has_keyrefs = true;
                }
            }
        }
    }

    fn aidc_of(&self, def: ItemId) -> Option<usize> {
        self.idc.aidcs.iter().position(|a| a.def == def)
    }

    /// xmlSchemaIDCAddStateObject
    fn idc_add_state_object(&mut self, matcher: usize, idc: ItemId, field: Option<usize>) -> i32 {
        let s = self.schema.clone();
        let sel = match field {
            None => s.it(idc).selector.as_ref(),
            Some(f) => s.it(idc).fields.get(f),
        };
        let ctxt = match sel
            .and_then(|sel| sel.comp.as_ref())
            .and_then(|c| c.stream_ctxt())
        {
            Some(c) => c,
            None => {
                self.verror_int(
                    "xmlSchemaIDCAddStateObject",
                    "failed to create an XPath validation context",
                );
                return -1;
            }
        };
        let sto = Sto {
            typ: if field.is_some() {
                STO_FIELD
            } else {
                STO_SELECTOR
            },
            depth: self.depth,
            matcher,
            idc,
            field,
            ctxt,
            history: Vec::new(),
        };
        let id = match self.idc.stos.iter().position(|s| s.is_none()) {
            Some(i) => {
                self.idc.stos[i] = Some(sto);
                i
            }
            None => {
                self.idc.stos.push(Some(sto));
                self.idc.stos.len() - 1
            }
        };
        self.idc.active.insert(0, id);
        0
    }

    /// xmlSchemaIDCRegisterMatchers
    #[inline(never)]
    pub(super) fn idc_register_matchers(&mut self, decl: ItemId) -> i32 {
        let s = self.schema.clone();
        let idcs = s.it(decl).idcs.clone();
        if idcs.is_empty() {
            return 0;
        }
        let d = self.depth as usize;
        if !self.elems[d].idc_matchers.is_empty() {
            self.verror_int(
                "xmlSchemaIDCRegisterMatchers",
                "The chain of IDC matchers is expected to be empty",
            );
            return -1;
        }
        for idc in idcs {
            if s.it(idc).typ == TYPE_IDC_KEYREF {
                let ref_idc = s.it(idc).idc_ref.and_then(|r| s.it(r).item);
                if let Some(r) = ref_idc {
                    self.elems[d].has_keyrefs = true;
                    let a = match self.aidc_of(r) {
                        Some(a) => a,
                        None => {
                            self.verror_int(
                                "xmlSchemaIDCRegisterMatchers",
                                "Could not find an augmented IDC item for an IDC definition",
                            );
                            return -1;
                        }
                    };
                    let kd = self.idc.aidcs[a].keyref_depth;
                    if kd == -1 || self.depth < kd {
                        self.idc.aidcs[a].keyref_depth = self.depth;
                    }
                }
            }
            let a = match self.aidc_of(idc) {
                Some(a) => a,
                None => {
                    self.verror_int(
                        "xmlSchemaIDCRegisterMatchers",
                        "Could not find an augmented IDC item for an IDC definition",
                    );
                    return -1;
                }
            };
            let m = Matcher {
                depth: self.depth,
                aidc: a,
                idc_type: s.it(idc).typ,
                ..Default::default()
            };
            let mid = match self.idc.free_matchers.pop() {
                Some(i) => {
                    self.idc.matchers[i] = m;
                    i
                }
                None => {
                    self.idc.matchers.push(m);
                    self.idc.matchers.len() - 1
                }
            };
            self.elems[d].idc_matchers.push(mid);
            if self.idc_add_state_object(mid, idc, None) == -1 {
                return -1;
            }
        }
        0
    }

    fn idc_eval_sto(&mut self, id: usize, node_type: i32, depth: i32, resolved: &mut i32) -> i32 {
        let cur = self.idc_cur();
        let (local, ns) = match cur {
            Cur::Elem(i) => (self.elems[i].local.clone(), self.elems[i].ns.clone()),
            Cur::Attr(i) => (self.attrs[i].local.clone(), self.attrs[i].ns.clone()),
        };
        let sto = self.idc.stos[id].as_mut().unwrap();
        let res = if node_type == ELEMENT_NODE {
            sto.ctxt.push(Some(&local), ns.as_deref())
        } else {
            sto.ctxt.push_attr(Some(&local), ns.as_deref())
        };
        if res == -1 {
            self.verror_int("xmlSchemaXPathEvaluate", "calling xmlStreamPush()");
            return -1;
        }
        if res == 0 {
            return 0;
        }
        sto.history.push(depth);
        if sto.typ == STO_SELECTOR {
            let (matcher, idc) = (sto.matcher, sto.idc);
            let nb = self.schema.it(idc).fields.len();
            for f in 0..nb {
                if self.idc_add_state_object(matcher, idc, Some(f)) == -1 {
                    return -1;
                }
            }
        } else if sto.typ == STO_FIELD {
            if *resolved == 0 {
                match cur {
                    Cur::Elem(i) => self.elems[i].flags |= NODE_INFO_VALUE_NEEDED,
                    Cur::Attr(i) => self.attrs[i].flags |= NODE_INFO_VALUE_NEEDED,
                }
            }
            *resolved += 1;
        }
        0
    }

    /// xmlSchemaXPathEvaluate: the number of fields resolving to the
    /// current node, -1 on internal errors.
    #[inline(never)]
    pub(super) fn idc_xpath_evaluate(&mut self, node_type: i32) -> i32 {
        if self.idc.active.is_empty() {
            return 0;
        }
        let mut depth = self.depth;
        if node_type == ATTRIBUTE_NODE {
            depth += 1;
        }
        let mut resolved = 0;
        let first: Vec<usize> = self.idc.active.clone();
        for id in first.iter().copied() {
            if self.idc_eval_sto(id, node_type, depth, &mut resolved) == -1 {
                return -1;
            }
        }
        // the field state objects created on this node are evaluated too
        let added = self.idc.active.len() - first.len();
        let newer: Vec<usize> = self.idc.active[..added].to_vec();
        for id in newer {
            if self.idc_eval_sto(id, node_type, depth, &mut resolved) == -1 {
                return -1;
            }
        }
        resolved
    }

    /// xmlSchemaXPathPop
    pub(super) fn idc_xpath_pop(&mut self) {
        for &id in &self.idc.active {
            if let Some(s) = self.idc.stos[id].as_mut() {
                s.ctxt.pop();
            }
        }
    }

    fn canon_key_seq(&self, keys: &[usize], for_hash: bool) -> String {
        let s = &self.schema;
        let mut buf = String::from("[");
        for (i, &k) in keys.iter().enumerate() {
            buf.push('\'');
            let key = &self.idc.keys[k];
            let ws = if for_hash {
                T::WS_COLLAPSE
            } else {
                s.whitespace_facet_value(key.typ)
            };
            let mut out: Option<String> = None;
            let list = key.val.len() > 1;
            for v in &key.val {
                let value: Option<String> = match v.typ {
                    T::STRING | T::NORMSTRING | T::ANYSIMPLETYPE => {
                        Some(canon_value_whtsp_ext(v, ws))
                    }
                    _ => {
                        let (r, mut c) = T::get_canon_value(v);
                        if r == -1 {
                            c = "???".to_string();
                        }
                        if for_hash && v.typ == T::DECIMAL && c.len() > 2 && c.ends_with(".0") {
                            c.truncate(c.len() - 2);
                        }
                        Some(c)
                    }
                };
                match (&mut out, value) {
                    (None, None) => {
                        if !list {
                            out = Some(String::new());
                        }
                    }
                    (None, Some(v)) => out = Some(v),
                    (Some(o), Some(v)) => {
                        o.push(' ');
                        o.push_str(&v);
                    }
                    (Some(_), None) => {}
                }
            }
            buf.push_str(&out.unwrap_or_default());
            if i + 1 < keys.len() {
                buf.push_str("', ");
            } else {
                buf.push('\'');
            }
        }
        buf.push(']');
        buf
    }

    fn keys_equal(&self, a: &[usize], b: &[usize]) -> i32 {
        let s = &self.schema;
        let mut res = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            res = values_equal(s, &self.idc.keys[*x].val, &self.idc.keys[*y].val);
            if res != 1 {
                return res;
            }
        }
        res
    }

    fn idc_err(&mut self, msg: &str) {
        let s = self.schema.clone();
        self.ec.custom_err(&s, E::SCHEMAV_CVC_IDC, None, None, msg);
    }

    /// xmlSchemaXPathProcessHistory
    #[inline(never)]
    pub(super) fn idc_process_history(&mut self, depth: i32) -> i32 {
        if self.idc.active.is_empty() {
            return 0;
        }
        let s = self.schema.clone();
        let cur = self.idc_cur();
        let typ = match cur {
            Cur::Elem(i) => self.elems[i].type_def,
            Cur::Attr(i) => self.attrs[i].type_def,
        };
        let mut key: Option<usize> = None;
        let mut pos_in_list = 0;
        while pos_in_list < self.idc.active.len() {
            let id = self.idc.active[pos_in_list];
            let (sto_typ, sto_depth, matcher, idc, field, last) = {
                let sto = self.idc.stos[id].as_mut().unwrap();
                sto.ctxt.pop();
                (
                    sto.typ,
                    sto.depth,
                    sto.matcher,
                    sto.idc,
                    sto.field,
                    sto.history.last().copied(),
                )
            };
            // only matches at the current depth are of interest
            if last.is_some_and(|d| d != depth) {
                pos_in_list += 1;
                continue;
            }
            'deregister: {
                if last.is_none() {
                    break 'deregister;
                }
                if sto_typ == STO_FIELD {
                    let simple_type = match typ {
                        Some(t) if s.is_complex(t) => {
                            if s.has_simple_content(t) {
                                match s.it(t).content_type_def {
                                    Some(c) => Some(c),
                                    None => {
                                        self.verror_int(
                                            "xmlSchemaXPathProcessHistory",
                                            "field resolves to a CT with simple content but the CT is missing the ST definition",
                                        );
                                        return -1;
                                    }
                                }
                            } else {
                                None
                            }
                        }
                        t => t,
                    };
                    let xpath = field
                        .and_then(|f| s.it(idc).fields.get(f))
                        .map(|f| f.xpath.clone())
                        .unwrap_or_default();
                    let st = match simple_type {
                        None => {
                            let m = format!(
                                "The XPath '{}' of a field of {} does evaluate to a node of non-simple type",
                                xpath,
                                s.component_designation(idc)
                            );
                            self.idc_err(&m);
                            self.idc.stos[id].as_mut().unwrap().history.pop();
                            break 'deregister;
                        }
                        Some(t) => t,
                    };
                    let has_val = match cur {
                        Cur::Elem(i) => self.elems[i].val.is_some(),
                        Cur::Attr(i) => self.attrs[i].val.is_some(),
                    };
                    if key.is_none() && !has_val {
                        self.idc_err("Warning: No precomputed value available, the value was either invalid or something strange happened");
                        self.idc.stos[id].as_mut().unwrap().history.pop();
                        break 'deregister;
                    }
                    let mdepth = self.idc.matchers[matcher].depth;
                    let pos = (sto_depth - mdepth) as usize;
                    let idx = s
                        .it(idc)
                        .fields
                        .get(field.unwrap_or(0))
                        .map(|f| f.index)
                        .unwrap_or(0) as usize;
                    let nb_fields = s.it(idc).fields.len();
                    {
                        let m = &mut self.idc.matchers[matcher];
                        if pos >= m.key_seqs.len() {
                            m.key_seqs.resize_with(pos + 1, || None);
                        }
                    }
                    let exists = matches!(
                        &self.idc.matchers[matcher].key_seqs[pos],
                        Some(seq) if seq.get(idx).is_some_and(|k| k.is_some())
                    );
                    if exists {
                        let m = format!(
                            "The XPath '{}' of a field of {} evaluates to a node-set with more than one member",
                            xpath,
                            s.component_designation(idc)
                        );
                        self.idc_err(&m);
                        self.idc.stos[id].as_mut().unwrap().history.pop();
                        break 'deregister;
                    }
                    if self.idc.matchers[matcher].key_seqs[pos].is_none() {
                        self.idc.matchers[matcher].key_seqs[pos] = Some(vec![None; nb_fields]);
                    }
                    let k = match key {
                        Some(k) => k,
                        None => {
                            let val = match cur {
                                Cur::Elem(i) => self.elems[i].val.take(),
                                Cur::Attr(i) => self.attrs[i].val.take(),
                            }
                            .unwrap_or_default();
                            self.idc.keys.push(Key { typ: st, val });
                            let k = self.idc.keys.len() - 1;
                            key = Some(k);
                            k
                        }
                    };
                    if let Some(seq) = self.idc.matchers[matcher].key_seqs[pos].as_mut() {
                        if idx < seq.len() {
                            seq[idx] = Some(k);
                        }
                    }
                } else if sto_typ == STO_SELECTOR {
                    let def = self.idc.aidcs[self.idc.matchers[matcher].aidc].def;
                    let idc_type = s.it(def).typ;
                    let nb_keys = s.it(def).fields.len();
                    let pos = (depth - self.idc.matchers[matcher].depth) as usize;
                    let seq = self.idc.matchers[matcher]
                        .key_seqs
                        .get_mut(pos)
                        .and_then(|s| s.take());
                    'leave: {
                        let complete: Option<Vec<usize>> = seq
                            .as_ref()
                            .and_then(|seq| seq.iter().copied().collect::<Option<Vec<usize>>>());
                        let keys = match complete {
                            Some(k) if k.len() == nb_keys => k,
                            _ => {
                                if idc_type == TYPE_IDC_KEY {
                                    let m = format!(
                                        "Not all fields of {} evaluate to a node",
                                        s.component_designation(def)
                                    );
                                    self.idc_err(&m);
                                }
                                break 'leave;
                            }
                        };
                        if idc_type != TYPE_IDC_KEYREF
                            && !self.idc.matchers[matcher].targets.is_empty()
                        {
                            let h = self.canon_key_seq(&keys, true);
                            let cands: Vec<usize> = self.idc.matchers[matcher]
                                .htab
                                .get(&h)
                                .cloned()
                                .unwrap_or_default();
                            let mut dup = false;
                            for ti in cands {
                                let nid = self.idc.matchers[matcher].targets[ti];
                                let bkeys = self.idc.nodes[nid].keys.clone();
                                let r = self.keys_equal(&keys, &bkeys);
                                if r == -1 {
                                    return -1;
                                }
                                if r == 1 {
                                    dup = true;
                                    break;
                                }
                            }
                            if dup {
                                let m = format!(
                                    "Duplicate key-sequence {} in {}",
                                    self.canon_key_seq(&keys, false),
                                    s.component_designation(def)
                                );
                                self.idc_err(&m);
                                break 'leave;
                            }
                        }
                        let (line, local, ns) = match cur {
                            Cur::Elem(i) => (
                                self.elems[i].line,
                                self.elems[i].local.clone(),
                                self.elems[i].ns.clone(),
                            ),
                            Cur::Attr(i) => (
                                self.attrs[i].line,
                                self.attrs[i].local.clone(),
                                self.attrs[i].ns.clone(),
                            ),
                        };
                        let hash = if idc_type != TYPE_IDC_KEYREF {
                            Some(self.canon_key_seq(&keys, true))
                        } else {
                            None
                        };
                        self.idc.nodes.push(IdcNode {
                            keys,
                            line,
                            local,
                            ns,
                        });
                        let nid = self.idc.nodes.len() - 1;
                        let m = &mut self.idc.matchers[matcher];
                        m.targets.push(nid);
                        if let Some(h) = hash {
                            let ti = m.targets.len() - 1;
                            m.htab.entry(h).or_default().push(ti);
                        }
                    }
                }
                self.idc.stos[id].as_mut().unwrap().history.pop();
            }
            // deregister_check:
            let (nh, sd) = {
                let sto = self.idc.stos[id].as_ref().unwrap();
                (sto.history.len(), sto.depth)
            };
            if nh == 0 && sd == depth {
                if pos_in_list != 0 {
                    self.verror_int(
                        "xmlSchemaXPathProcessHistory",
                        "The state object to be removed is not the first in the list",
                    );
                }
                self.idc.active.remove(pos_in_list);
                self.idc.stos[id] = None;
            } else {
                pos_in_list += 1;
            }
        }
        0
    }

    /// xmlSchemaIDCFillNodeTables
    #[inline(never)]
    pub(super) fn idc_fill_node_tables(&mut self, d: usize) -> i32 {
        let s = self.schema.clone();
        let matchers = self.elems[d].idc_matchers.clone();
        for mid in matchers {
            let aidc = self.idc.matchers[mid].aidc;
            let def = self.idc.aidcs[aidc].def;
            if s.it(def).typ == TYPE_IDC_KEYREF || self.idc.matchers[mid].targets.is_empty() {
                continue;
            }
            let kd = self.idc.aidcs[aidc].keyref_depth;
            if kd == -1 || kd > self.depth {
                continue;
            }
            let bi = match self.elems[d].idc_table.iter().position(|b| b.def == def) {
                Some(i) => i,
                None => {
                    self.elems[d].idc_table.push(Binding {
                        def,
                        nodes: Vec::new(),
                        dupls: Vec::new(),
                    });
                    self.elems[d].idc_table.len() - 1
                }
            };
            let targets = std::mem::take(&mut self.idc.matchers[mid].targets);
            self.idc.matchers[mid].htab.clear();
            if self.elems[d].idc_table[bi].nodes.is_empty()
                && self.elems[d].idc_table[bi].dupls.is_empty()
            {
                self.elems[d].idc_table[bi].nodes = targets;
                continue;
            }
            'targets: for t in targets {
                let keys = self.idc.nodes[t].keys.clone();
                let dupls = self.elems[d].idc_table[bi].dupls.clone();
                for dn in dupls {
                    let r = self.keys_equal(&keys, &self.idc.nodes[dn].keys.clone());
                    if r == -1 {
                        return -1;
                    }
                    if r == 1 {
                        continue 'targets;
                    }
                }
                let mut j = 0;
                while j < self.elems[d].idc_table[bi].nodes.len() {
                    let nj = self.elems[d].idc_table[bi].nodes[j];
                    let r = self.keys_equal(&keys, &self.idc.nodes[nj].keys.clone());
                    if r == -1 {
                        return -1;
                    }
                    if r == 1 {
                        let b = &mut self.elems[d].idc_table[bi];
                        b.dupls.push(nj);
                        b.nodes.swap_remove(j);
                        continue 'targets;
                    }
                    j += 1;
                }
                self.elems[d].idc_table[bi].nodes.push(t);
            }
        }
        0
    }

    /// xmlSchemaBubbleIDCNodeTables
    #[inline(never)]
    pub(super) fn idc_bubble_node_tables(&mut self, d: usize) -> i32 {
        let table = std::mem::take(&mut self.elems[d].idc_table);
        for mut bind in table {
            if bind.nodes.is_empty() && bind.dupls.is_empty() {
                continue;
            }
            if let Some(a) = self.aidc_of(bind.def) {
                let kd = self.idc.aidcs[a].keyref_depth;
                if kd == -1 || kd >= self.depth {
                    continue;
                }
            }
            let p = d - 1;
            match self.elems[p]
                .idc_table
                .iter()
                .position(|b| b.def == bind.def)
            {
                Some(pi) => {
                    let mut old_num = self.elems[p].idc_table[pi].nodes.len();
                    let old_dupls = self.elems[p].idc_table[pi].dupls.len();
                    'nodes: for node in bind.nodes {
                        let keys = self.idc.nodes[node].keys.clone();
                        for j in 0..old_dupls {
                            let dn = self.elems[p].idc_table[pi].dupls[j];
                            let r = self.keys_equal(&keys, &self.idc.nodes[dn].keys.clone());
                            if r == -1 {
                                return -1;
                            }
                            if r == 1 {
                                continue 'nodes;
                            }
                        }
                        let mut found = None;
                        for j in 0..old_num {
                            let pn = self.elems[p].idc_table[pi].nodes[j];
                            let r = self.keys_equal(&keys, &self.idc.nodes[pn].keys.clone());
                            if r == -1 {
                                return -1;
                            }
                            if r == 1 {
                                found = Some(j);
                                break;
                            }
                        }
                        let pb = &mut self.elems[p].idc_table[pi];
                        match found {
                            Some(j) => {
                                let par_node = pb.nodes[j];
                                old_num -= 1;
                                let nb = pb.nodes.len() - 1;
                                pb.nodes[j] = pb.nodes[old_num];
                                if nb != old_num {
                                    pb.nodes[old_num] = pb.nodes[nb];
                                }
                                pb.nodes.truncate(nb);
                                pb.dupls.push(par_node);
                            }
                            None => pb.nodes.push(node),
                        }
                    }
                }
                None => {
                    let nb = Binding {
                        def: bind.def,
                        nodes: std::mem::take(&mut bind.nodes),
                        dupls: std::mem::take(&mut bind.dupls),
                    };
                    self.elems[p].idc_table.insert(0, nb);
                }
            }
        }
        0
    }

    /// xmlSchemaCheckCVCIDCKeyRef
    #[inline(never)]
    pub(super) fn idc_check_keyrefs(&mut self, d: usize) -> i32 {
        let s = self.schema.clone();
        let matchers = self.elems[d].idc_matchers.clone();
        for mid in matchers {
            if self.idc.matchers[mid].idc_type != TYPE_IDC_KEYREF
                || self.idc.matchers[mid].targets.is_empty()
            {
                continue;
            }
            let def = self.idc.aidcs[self.idc.matchers[mid].aidc].def;
            let refd = s.it(def).idc_ref.and_then(|r| s.it(r).item);
            let bind = self.elems[d]
                .idc_table
                .iter()
                .find(|b| Some(b.def) == refd)
                .map(|b| (b.nodes.clone(), b.dupls.clone()));
            let mut table: HashMap<String, Vec<usize>> = HashMap::new();
            if let Some((nodes, _)) = &bind {
                for &n in nodes {
                    let h = self.canon_key_seq(&self.idc.nodes[n].keys.clone(), true);
                    table.entry(h).or_default().push(n);
                }
            }
            let targets = self.idc.matchers[mid].targets.clone();
            for rn in targets {
                let ref_keys = self.idc.nodes[rn].keys.clone();
                let mut res = 0;
                if let Some((_, dupls)) = &bind {
                    let h = self.canon_key_seq(&ref_keys, true);
                    for &n in table.get(&h).map(|v| v.as_slice()).unwrap_or(&[]) {
                        res = self.keys_equal(&self.idc.nodes[n].keys.clone(), &ref_keys);
                        if res == -1 {
                            return -1;
                        }
                        if res == 1 {
                            break;
                        }
                    }
                    if res == 0 && !dupls.is_empty() {
                        for &dn in dupls {
                            res = self.keys_equal(&self.idc.nodes[dn].keys.clone(), &ref_keys);
                            if res == -1 {
                                return -1;
                            }
                            if res == 1 {
                                let m = format!(
                                    "More than one match found for key-sequence {} of keyref '{}'",
                                    self.canon_key_seq(&ref_keys, false),
                                    s.component_qname(def)
                                );
                                self.keyref_err(rn, &m);
                                break;
                            }
                        }
                    }
                }
                if res == 0 {
                    let m = format!(
                        "No match found for key-sequence {} of keyref '{}'",
                        self.canon_key_seq(&ref_keys, false),
                        s.component_qname(def)
                    );
                    self.keyref_err(rn, &m);
                }
            }
        }
        0
    }

    /// xmlSchemaKeyrefErr
    fn keyref_err(&mut self, node: usize, message: &str) {
        let n = &self.idc.nodes[node];
        let msg = format!(
            "Element '{}': {}.\n",
            format_qname(n.ns.as_deref(), Some(&n.local)),
            message
        );
        let line = n.line;
        self.ec.err4line(
            crate::parser::ERR_ERROR,
            E::SCHEMAV_CVC_IDC,
            None,
            line,
            msg,
        );
    }

    /// The end of an element (xmlSchemaValidatorPopElem, end_elem).
    #[inline(never)]
    pub(super) fn idc_end_elem(&mut self) -> i32 {
        let d = self.depth as usize;
        if self.elems[d].applied_xpath && self.idc_process_history(self.depth) == -1 {
            return -1;
        }
        if !self.elems[d].idc_matchers.is_empty()
            && self.idc.has_keyrefs
            && self.idc_fill_node_tables(d) == -1
        {
            return -1;
        }
        if self.elems[d].has_keyrefs && self.idc_check_keyrefs(d) == -1 {
            return -1;
        }
        if !self.elems[d].idc_table.is_empty()
            && self.depth > 0
            && self.idc.has_keyrefs
            && self.idc_bubble_node_tables(d) == -1
        {
            return -1;
        }
        // xmlSchemaClearElemInfo: release the matchers
        let ms = std::mem::take(&mut self.elems[d].idc_matchers);
        for m in ms {
            self.idc.matchers[m] = Matcher::default();
            self.idc.free_matchers.push(m);
        }
        0
    }
}
