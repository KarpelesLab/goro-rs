//! xsl:document / exsl:document & co. (xsltDocumentElem)

use crate::output::OutProps;
use crate::style::{PreComp, XSLT_SAXON_NAMESPACE, XSLT_XALAN_NAMESPACE};
use crate::transform::{OutType, Transform};
use crate::util::*;
use goro_ext_xml::tree::{NType, NodeId};
use std::rc::Rc;

impl<'v> Transform<'v> {
    pub fn document_elem(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let name = tw(|t| t.nodes[inst].name.clone());
        let url = match &comp.filename {
            Some(f) => Some(f.clone()),
            None => {
                if name == "output" {
                    self.eval_attr_value_template(inst, "file", Some(XSLT_SAXON_NAMESPACE))
                        .or_else(|| {
                            self.eval_attr_value_template(inst, "href", Some(XSLT_SAXON_NAMESPACE))
                        })
                } else if name == "write" {
                    let mut u =
                        self.eval_attr_value_template(inst, "select", Some(XSLT_XALAN_NAMESPACE));
                    if let Some(sel) = u.take() {
                        if let Ok(c) = crate::xpath::compile(&sel) {
                            let ns = Rc::new(Vec::new());
                            u = self.eval_xpath_string_ns(&c, &ns);
                        }
                    }
                    u.or_else(|| {
                        self.eval_attr_value_template(inst, "file", Some(XSLT_XALAN_NAMESPACE))
                    })
                    .or_else(|| {
                        self.eval_attr_value_template(inst, "href", Some(XSLT_XALAN_NAMESPACE))
                    })
                } else if name == "document" {
                    self.eval_attr_value_template(inst, "href", None)
                } else {
                    None
                }
            }
        };
        let url = match url {
            Some(u) => u,
            None => {
                self.error(
                    Some(inst),
                    "xsltDocumentElem: href/URI-Reference not found\n",
                );
                return;
            }
        };
        let filename = match build_uri(&url, None) {
            Some(f) => f,
            None => {
                self.error(
                    Some(inst),
                    &format!("xsltDocumentElem: URL computation failed for {}\n", url),
                );
                return;
            }
        };
        if self.sec_prefs != 0 {
            let r = crate::security::check_write(self, &filename);
            if r <= 0 {
                if r == 0 {
                    self.error(
                        Some(inst),
                        &format!("xsltDocumentElem: write rights for {} denied\n", filename),
                    );
                }
                return;
            }
        }
        let old_output = self.output;
        let old_insert = self.insert;
        let old_type = self.out_type;
        let mut props = OutProps {
            indent: -1,
            omit_xml_declaration: -1,
            standalone: -1,
            ..Default::default()
        };
        let mut version: Option<String> = None;
        let mut doctype_system: Option<String> = None;
        let mut doctype_public: Option<String> = None;
        if let Some(p) = self.eval_attr_value_template(inst, "version", None) {
            version = Some(p);
        }
        if let Some(p) = self.eval_attr_value_template(inst, "encoding", None) {
            props.encoding = Some(p);
        }
        if let Some(p) = self.eval_attr_value_template(inst, "method", None) {
            match get_qname_uri(self.vm, inst, &p) {
                Err(()) => {}
                Ok((None, local)) => {
                    if local == "xml" || local == "html" || local == "text" {
                        props.method = Some(local);
                    } else {
                        self.error(
                            Some(inst),
                            &format!("invalid value for method: {}\n", local),
                        );
                    }
                }
                Ok((Some(u), local)) => {
                    props.method = Some(local);
                    props.method_uri = Some(u);
                }
            }
        }
        if let Some(p) = self.eval_attr_value_template(inst, "doctype-system", None) {
            doctype_system = Some(p);
        }
        if let Some(p) = self.eval_attr_value_template(inst, "doctype-public", None) {
            doctype_public = Some(p);
        }
        for (attr, which) in [
            ("standalone", 0),
            ("indent", 1),
            ("omit-xml-declaration", 2),
        ] {
            if let Some(p) = self.eval_attr_value_template(inst, attr, None) {
                let v = match p.as_str() {
                    "yes" => 1,
                    "no" => 0,
                    _ => {
                        self.error(Some(inst), &format!("invalid value for {}: {}\n", attr, p));
                        continue;
                    }
                };
                match which {
                    0 => props.standalone = v,
                    1 => props.indent = v,
                    _ => props.omit_xml_declaration = v,
                }
            }
        }
        let _ = self.eval_attr_value_template(inst, "cdata-section-elements", None);
        let method = props.method.clone();
        let res: NodeId;
        match method.as_deref() {
            Some(m) if m != "xml" => {
                if m == "html" {
                    self.out_type = OutType::Html;
                    let (mut dp, mut ds) = (doctype_public.clone(), doctype_system.clone());
                    if dp.is_some() || ds.is_some() {
                        res = html_new_doc_dtd(ds.as_deref(), dp.as_deref(), true);
                    } else {
                        if let Some(v) = &version {
                            if let Some((p, s)) = crate::apply::html_ids_pub(v) {
                                dp = p.map(|x| x.to_string());
                                ds = s.map(|x| x.to_string());
                            }
                        }
                        res = html_new_doc_dtd(ds.as_deref(), dp.as_deref(), false);
                    }
                } else if m == "xhtml" {
                    self.error(Some(inst), "xsltDocumentElem: unsupported method xhtml\n");
                    self.out_type = OutType::Html;
                    res = html_new_doc_dtd(
                        doctype_system.as_deref(),
                        doctype_public.as_deref(),
                        false,
                    );
                } else if m == "text" {
                    self.out_type = OutType::Text;
                    res = tw(|t| t.new_doc(version.as_deref()));
                } else {
                    self.error(
                        Some(inst),
                        &format!("xsltDocumentElem: unsupported method ({})\n", m),
                    );
                    self.output = old_output;
                    self.insert = old_insert;
                    self.out_type = old_type;
                    return;
                }
            }
            _ => {
                self.out_type = OutType::Xml;
                res = tw(|t| t.new_doc(version.as_deref()));
            }
        }
        if let Some(e) = &props.encoding {
            tw(|t| {
                if let Some(i) = t.doc_info_mut(res) {
                    i.encoding = Some(e.clone());
                }
            });
        }
        self.output = res;
        self.insert = Some(res);
        let ch = tw(|t| t.nodes[inst].children);
        self.apply_sequence_constructor(node, ch, None);
        if let Some(root) = tw(|t| t.root_element(res)) {
            let doctype = tw(|t| match t.ns_prefix(t.nodes[root].ns) {
                Some(p) if t.nodes[root].ns.is_some() => format!("{}:{}", p, t.nodes[root].name),
                _ => t.nodes[root].name.clone(),
            });
            if self.out_type == OutType::Xml
                && (doctype_public.is_some() || doctype_system.is_some())
            {
                tw(|t| {
                    t.create_int_subset(
                        Some(res),
                        &doctype,
                        doctype_public.as_deref(),
                        doctype_system.as_deref(),
                    )
                });
            }
        }
        let mut append = false;
        if let Some(p) = self.eval_attr_value_template(inst, "append", None) {
            if p == "true" || p == "yes" {
                props.omit_xml_declaration = 1;
                append = true;
            } else {
                props.omit_xml_declaration = 0;
            }
        }
        let ret = match crate::output::save_result_props(self.vm, &props, res) {
            Ok(bytes) => {
                let path = goro_ext_xml::io::strip_file_scheme(&filename);
                if append {
                    use std::io::Write;
                    match std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(&path)
                    {
                        Ok(mut f) => f.write_all(&bytes).map(|_| 0).unwrap_or(-1),
                        Err(_) => -1,
                    }
                } else if goro_ext_xml::io::write_resource(self.vm, &path, &bytes) {
                    0
                } else {
                    -1
                }
            }
            Err(()) => -1,
        };
        if ret < 0 {
            self.error(
                Some(inst),
                &format!("xsltDocumentElem: unable to save to {}\n", filename),
            );
        }
        let _ = NType::Element;
        self.output = old_output;
        self.insert = old_insert;
        self.out_type = old_type;
    }
}

/// htmlNewDoc (dtd_default) / htmlNewDocNoDtD
fn html_new_doc_dtd(system: Option<&str>, public: Option<&str>, dtd_default: bool) -> NodeId {
    tw(|t| {
        let d = t.new_html_doc();
        let (sys, publ) = if dtd_default && system.is_none() && public.is_none() {
            (
                Some("http://www.w3.org/TR/REC-html40/loose.dtd"),
                Some("-//W3C//DTD HTML 4.0 Transitional//EN"),
            )
        } else {
            (system, public)
        };
        if sys.is_some() || publ.is_some() {
            t.create_int_subset(Some(d), "html", publ, sys);
        }
        d
    })
}
