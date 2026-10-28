//! Document type declarations: the internal subset, the external subset
//! (through the entity loader), parameter entities and conditional
//! sections (XML 1.0 sections 2.8, 3.2-3.4, 4.2-4.7).

use super::Sink;
use super::parser::{Halt, Parser, R, collapse_spaces};
use crate::chars::{is_pubid_char, is_space};
use crate::error::Domain;
use crate::tree::{
    AttrDecl, AttrDefault, AttrType, ContentModel, DtdData, DtdItem, ElementDecl, EntityDecl,
    EntityKind, NotationDecl, Occur, Particle, Str,
};

impl Parser<'_> {
    /// `<!DOCTYPE ...>` at the position.
    pub(crate) fn parse_doctype(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.pos += 9;
        if !self.skip_spaces() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            self.fatal(65, "Space required after 'DOCTYPE'");
        }
        let Some(nr) = self.scan_name() else {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(68, "xmlParseDocTypeDecl : no DOCTYPE name !"));
        };
        let name = self.slice(nr).to_string();
        let name: Str = self.interned_or(&name);
        self.skip_spaces();
        let (public_id, system_id) = self.parse_external_id(false)?;
        self.skip_spaces();
        self.dtd = Some(DtdData {
            name,
            public_id,
            system_id,
            ..Default::default()
        });
        self.ext = None;
        self.has_pe_refs = false;
        if self.cur() == b'[' {
            self.pos += 1;
            let start = self.pos;
            self.parse_subset(sink, false)?;
            let raw = self.slice((start, self.pos)).to_string();
            if self.cur() != b']' {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                return Err(self.fatal(61, "DOCTYPE improperly terminated"));
            }
            self.dtd.as_mut().unwrap().raw_internal = Some(raw);
            self.pos += 1;
            self.skip_spaces();
        }
        if self.cur() != b'>' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(61, "DOCTYPE improperly terminated"));
        }
        self.pos += 1;
        if self.opts.load_dtd() {
            self.load_external_subset(sink)?;
        }
        if !self.stopped {
            self.finish_doctype(sink);
        }
        Ok(())
    }

    fn load_external_subset(&mut self, sink: &mut dyn Sink) -> R<()> {
        let (public, system) = {
            let d = self.dtd.as_ref().unwrap();
            (d.public_id.clone(), d.system_id.clone())
        };
        let Some(system) = system else {
            return Ok(());
        };
        let base = self.opts.url.clone();
        let res = match self.loader.as_mut() {
            Some(l) => l.load(public.as_deref(), &system, base.as_deref()),
            None => Err(super::LoadError::NotFound),
        };
        let bytes = match res {
            Ok(b) => b,
            Err(super::LoadError::NotFound) => {
                let url = super::parser::resolve_system_id(&system, base.as_deref());
                self.warn(
                    Domain::Io,
                    1549,
                    format!("failed to load external entity \"{url}\""),
                );
                return Ok(());
            }
            Err(super::LoadError::Refused) => return Ok(()),
        };
        let internal = self.dtd.clone().unwrap();
        let text = self.decode_external(&bytes)?;
        // Internal declarations take precedence (XML 1.0 2.8): the external
        // subset is parsed into the same maps, where first declarations win.
        let r = self.with_buffer(text, |p| p.parse_subset(sink, true));
        let combined = self.dtd.take().unwrap();
        let mut ext = DtdData {
            name: internal.name.clone(),
            public_id: public,
            system_id: Some(system),
            ..Default::default()
        };
        for (k, v) in &combined.entities {
            if !internal.entities.contains_key(k) {
                ext.entities.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &combined.pentities {
            if !internal.pentities.contains_key(k) {
                ext.pentities.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &combined.elements {
            if !internal.elements.contains_key(k) {
                ext.elements.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &combined.attlists {
            let int = internal.attlists.get(k);
            let extra: Vec<AttrDecl> = v
                .iter()
                .filter(|a| !int.is_some_and(|l| l.iter().any(|b| b.name == a.name)))
                .cloned()
                .collect();
            if !extra.is_empty() {
                ext.attlists.insert(k.clone(), extra);
            }
        }
        for (k, v) in &combined.notations {
            if !internal.notations.contains_key(k) {
                ext.notations.insert(k.clone(), v.clone());
            }
        }
        ext.items = combined.items[internal.items.len().min(combined.items.len())..].to_vec();
        self.dtd = Some(internal);
        self.ext = Some(ext);
        r
    }

    /// Runs `f` with the parser reading `text` instead of the document.
    pub(crate) fn with_buffer<T>(
        &mut self,
        text: Vec<u8>,
        f: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let lc = self.line_col(self.pos);
        let saved_err = self.err_pos;
        if saved_err.is_none() {
            self.err_pos = Some(lc);
        }
        let saved_buf = std::mem::replace(&mut self.buf, text);
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let r = f(self);
        self.buf = saved_buf;
        self.pos = saved_pos;
        self.err_pos = saved_err;
        r
    }

    /// `intSubset` up to `]` (internal) or `extSubsetDecl` to the end
    /// (external).
    fn parse_subset(&mut self, sink: &mut dyn Sink, external: bool) -> R<()> {
        loop {
            self.skip_spaces();
            if self.eof() {
                if external {
                    return Ok(());
                }
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                return Err(self.fatal(61, "DOCTYPE improperly terminated"));
            }
            let b = self.cur();
            if b == b']' && !external {
                return Ok(());
            }
            if b == b'%' {
                self.parse_pe_decl_sep(sink, external)?;
                continue;
            }
            if b == b'<' {
                if self.starts_with(b"<!--") {
                    let text = self.scan_dtd_comment()?;
                    self.dtd
                        .as_mut()
                        .unwrap()
                        .items
                        .push(DtdItem::Comment(text));
                    continue;
                }
                if self.starts_with(b"<?") {
                    let (t, d) = self.scan_dtd_pi()?;
                    self.dtd.as_mut().unwrap().items.push(DtdItem::Pi(t, d));
                    continue;
                }
                if external && self.starts_with(b"<![") {
                    self.parse_conditional(sink)?;
                    continue;
                }
                if self.starts_with(b"<!ELEMENT")
                    || self.starts_with(b"<!ATTLIST")
                    || self.starts_with(b"<!ENTITY")
                    || self.starts_with(b"<!NOTATION")
                {
                    if external {
                        // Expand parameter-entity references inside the
                        // declaration (allowed in the external subset).
                        let decl = self.take_decl_text()?;
                        self.with_buffer(decl.into_bytes(), |p| p.parse_markup_decl(true))?;
                    } else if let Err(e) = self.parse_markup_decl(false) {
                        if e == Halt::Fatal && !self.no_subset_error {
                            self.fatal(118, "Content error in the internal subset");
                        }
                        self.no_subset_error = false;
                        return Err(e);
                    }
                    continue;
                }
            }
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            if external && !self.pe_from_internal {
                return Err(self.fatal(61, "Content error in the external subset"));
            }
            return Err(self.fatal(118, "Content error in the internal subset"));
        }
    }

    fn parse_pe_decl_sep(&mut self, sink: &mut dyn Sink, external: bool) -> R<()> {
        self.pos += 1;
        let Some(r) = self.scan_name() else {
            return Err(self.fatal(24, "PEReference: no name"));
        };
        let name = self.slice(r).to_string();
        if self.cur() != b';' {
            return Err(self.fatal(25, "PEReference: expecting ';'"));
        }
        self.pos += 1;
        self.has_pe_refs = true;
        let Some(e) = self.find_entity(&name, true) else {
            if self.standalone == 1 {
                return Err(self.fatal(26, format!("PEReference: %{name}; not found")));
            }
            self.warn(
                Domain::Parser,
                27,
                format!("PEReference: %{name}; not found"),
            );
            return Ok(());
        };
        let text = match e.kind {
            EntityKind::InternalParameter => e.value.clone().unwrap_or_default(),
            _ => {
                if !self.opts.load_dtd() {
                    return Ok(());
                }
                let sys = e.system_id.clone().unwrap_or_default();
                match self.load_text(e.public_id.as_deref(), &sys)? {
                    Some(t) => t,
                    None => return Ok(()),
                }
            }
        };
        let saved = self.pe_from_internal;
        if !external {
            self.pe_from_internal = true;
            // Errors in the replacement text point just before the end of
            // the reference (observed on PHP 8.5).
            let lc = self.line_col(self.pos - 1);
            self.err_pos.get_or_insert(lc);
        }
        let saved_err = self.err_pos;
        let r = self.with_buffer(text.into_bytes(), |p| {
            p.parse_subset(sink, true)?;
            Ok(())
        });
        self.err_pos = if external { saved_err } else { None };
        self.pe_from_internal = saved;
        if r.is_err() && !external && self.stopped {
            // Report as the internal subset's content error.
            return r;
        }
        r
    }

    fn load_text(&mut self, public: Option<&str>, system: &str) -> R<Option<String>> {
        let base = self.opts.url.clone();
        let res = match self.loader.as_mut() {
            Some(l) => l.load(public, system, base.as_deref()),
            None => Err(super::LoadError::NotFound),
        };
        match res {
            Ok(b) => {
                let text = self.decode_external(&b)?;
                Ok(Some(String::from_utf8(text).unwrap_or_default()))
            }
            Err(super::LoadError::NotFound) => {
                let url = super::parser::resolve_system_id(system, base.as_deref());
                self.warn(
                    Domain::Io,
                    1549,
                    format!("failed to load external entity \"{url}\""),
                );
                Ok(None)
            }
            Err(super::LoadError::Refused) => Ok(None),
        }
    }

    /// Decodes an external entity and checks and removes its text
    /// declaration (XML 1.0 4.3.1: `encoding` is required, `standalone`
    /// is not allowed).
    pub(crate) fn decode_external(&mut self, bytes: &[u8]) -> R<Vec<u8>> {
        let enc = crate::encoding::declared_encoding(bytes)
            .and_then(|(n, _)| crate::encoding::lookup(&n));
        let d = crate::encoding::decode(bytes, enc.or(crate::encoding::sniff(bytes).0));
        let mut text = d.text;
        if text.starts_with(b"<?xml") && text.len() > 5 && is_space(text[5]) {
            let Some(end) = memchr::memmem::find(&text, b"?>") else {
                return Err(self.fatal(57, "parsing XML declaration: '?>' expected"));
            };
            let decl = String::from_utf8_lossy(&text[5..end]).into_owned();
            let mut rest = decl.trim_start();
            let mut has_encoding = false;
            if let Some(r) = rest.strip_prefix("version") {
                let r = r.trim_start();
                let r = r.strip_prefix('=').map(str::trim_start).unwrap_or(r);
                let q = r.chars().next().unwrap_or(' ');
                let close = r[1..].find(q).map(|i| i + 2).unwrap_or(r.len());
                rest = r[close.min(r.len())..].trim_start();
            }
            if let Some(r) = rest.strip_prefix("encoding") {
                has_encoding = true;
                let r = r.trim_start();
                let r = r.strip_prefix('=').map(str::trim_start).unwrap_or(r);
                let q = r.chars().next().unwrap_or(' ');
                let close = r[1..].find(q).map(|i| i + 2).unwrap_or(r.len());
                rest = r[close.min(r.len())..].trim_start();
            }
            if rest.starts_with("standalone") {
                return Err(self.fatal(57, "parsing XML declaration: '?>' expected"));
            }
            if !rest.is_empty() || !has_encoding {
                return Err(self.fatal(32, "Missing encoding in text declaration"));
            }
            text.drain(..end + 2);
        }
        Ok(text)
    }

    /// Replaces the parameter-entity reference at the position with its
    /// replacement text padded with spaces (XML 1.0 4.4.8), in the buffer.
    fn splice_pe(&mut self) -> R<()> {
        let start = self.pos;
        self.pos += 1;
        let Some(r) = self.scan_name() else {
            return Err(self.fatal(24, "PEReference: no name"));
        };
        let name = self.slice(r).to_string();
        if self.cur() != b';' {
            return Err(self.fatal(25, "PEReference: expecting ';'"));
        }
        let end = self.pos + 1;
        self.splices += 1;
        if self.splices > 10_000 {
            return Err(self.halt(89, "Detected an entity reference loop"));
        }
        let val = match self.find_entity(&name, true) {
            Some(e) if e.kind == EntityKind::InternalParameter => e.value.unwrap_or_default(),
            Some(e) => {
                let sys = e.system_id.unwrap_or_default();
                self.load_text(e.public_id.as_deref(), &sys)?
                    .unwrap_or_default()
            }
            None => {
                self.warn(
                    Domain::Parser,
                    27,
                    format!("PEReference: %{name}; not found"),
                );
                String::new()
            }
        };
        let rep = format!(" {val} ");
        self.buf.splice(start..end, rep.into_bytes());
        self.pos = start;
        Ok(())
    }

    /// The text of the markup declaration at the position, up to and
    /// including its `>` (quotes respected), with parameter-entity
    /// references expanded.
    fn take_decl_text(&mut self) -> R<String> {
        let start = self.pos;
        let entity_decl = self.starts_with(b"<!ENTITY");
        let mut quote = 0u8;
        while !self.eof() {
            let b = self.cur();
            if quote != 0 {
                if b == quote {
                    quote = 0;
                }
            } else if b == b'"' || b == b'\'' {
                quote = b;
            } else if b == b'>' {
                self.pos += 1;
                return Ok(self.slice((start, self.pos)).to_string());
            } else if b == b'%'
                && self
                    .char_at(self.pos + 1)
                    .is_some_and(crate::chars::is_name_start)
            {
                let is_marker = entity_decl
                    && self.buf[start..self.pos]
                        .iter()
                        .filter(|c| !is_space(**c))
                        .count()
                        == 8;
                if !is_marker {
                    self.splice_pe()?;
                    continue;
                }
            }
            self.pos += 1;
        }
        Err(self.fatal(73, "expected '>'"))
    }

    fn parse_conditional(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.pos += 3;
        self.skip_spaces();
        while self.cur() == b'%' {
            self.splice_pe()?;
            self.skip_spaces();
        }
        let mut kw = String::new();
        if let Some(r) = self.scan_name() {
            kw = self.slice(r).to_string();
        }
        self.skip_spaces();
        while self.cur() == b'%' {
            self.splice_pe()?;
            self.skip_spaces();
        }
        if self.cur() != b'[' {
            return Err(self.fatal(49, "Invalid conditional section"));
        }
        self.pos += 1;
        let Some(end) = self.find_section_end() else {
            return Err(self.fatal(49, "Conditional section not closed"));
        };
        match kw.as_str() {
            "INCLUDE" => {
                let inner = self.buf[self.pos..end].to_vec();
                self.pos = end + 3;
                self.with_buffer(inner, |p| p.parse_subset(sink, true))
            }
            "IGNORE" => {
                self.pos = end + 3;
                Ok(())
            }
            _ => Err(self.fatal(49, "Invalid conditional section")),
        }
    }

    /// Position of the `]]>` closing the conditional section that starts at
    /// the position (nested sections counted).
    fn find_section_end(&self) -> Option<usize> {
        let mut depth = 0;
        let mut i = self.pos;
        let mut quote = 0u8;
        while i + 2 < self.buf.len() {
            let b = self.buf[i];
            if quote != 0 {
                if b == quote {
                    quote = 0;
                }
                i += 1;
                continue;
            }
            if self.buf[i..].starts_with(b"<!--") {
                match memchr::memmem::find(&self.buf[i + 4..], b"-->") {
                    Some(o) => i += 4 + o + 3,
                    None => return None,
                }
                continue;
            }
            if self.buf[i..].starts_with(b"<![") {
                depth += 1;
                i += 3;
                continue;
            }
            if self.buf[i..].starts_with(b"]]>") {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                i += 3;
                continue;
            }
            i += 1;
        }
        None
    }

    fn scan_dtd_comment(&mut self) -> R<String> {
        self.pos += 4;
        let start = self.pos;
        match memchr::memmem::find(&self.buf[self.pos..], b"--") {
            Some(off) => {
                let i = self.pos + off;
                if self.at(i + 2) != b'>' {
                    self.pos = i;
                    let so_far: String = self.slice((start, i)).chars().take(50).collect();
                    return Err(
                        self.fatal(80, format!("Double hyphen within comment: <!--{so_far}"))
                    );
                }
                self.check_chars(start, i)?;
                let t = self.slice((start, i)).to_string();
                self.pos = i + 3;
                Ok(t)
            }
            None => {
                self.pos = self.buf.len();
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                Err(self.fatal(45, "Comment not terminated"))
            }
        }
    }

    fn scan_dtd_pi(&mut self) -> R<(String, String)> {
        self.pos += 2;
        let Some(r) = self.scan_name() else {
            return Err(self.fatal(46, "xmlParsePI : no target name"));
        };
        let target = self.slice(r).to_string();
        if target.eq_ignore_ascii_case("xml") {
            return Err(self.fatal(
                64,
                "XML declaration allowed only at the start of the document",
            ));
        }
        if !self.starts_with(b"?>") && !self.skip_spaces() {
            return Err(self.fatal(65, format!("ParsePI: PI {target} space expected")));
        }
        match memchr::memmem::find(&self.buf[self.pos..], b"?>") {
            Some(off) => {
                self.check_chars(self.pos, self.pos + off)?;
                let d = self.slice((self.pos, self.pos + off)).to_string();
                self.pos += off + 2;
                Ok((target, d))
            }
            None => {
                self.pos = self.buf.len();
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                Err(self.fatal(47, format!("ParsePI: PI {target} never end ...")))
            }
        }
    }

    /// `ExternalID` (or `PublicID` alone when `notation`). Returns
    /// (public, system).
    fn parse_external_id(&mut self, notation: bool) -> R<(Option<String>, Option<String>)> {
        if self.starts_with(b"SYSTEM") {
            self.pos += 6;
            if !self.skip_spaces() {
                return Err(self.fatal(65, "Space required after 'SYSTEM'"));
            }
            let s = self.parse_system_literal()?;
            return Ok((None, Some(s)));
        }
        if self.starts_with(b"PUBLIC") {
            self.pos += 6;
            if !self.skip_spaces() {
                return Err(self.fatal(65, "Space required after 'PUBLIC'"));
            }
            let p = self.parse_pubid_literal()?;
            let had_space = self.skip_spaces();
            let q = self.cur();
            if q == b'"' || q == b'\'' {
                if !had_space {
                    return Err(self.fatal(65, "Space required after the Public Identifier"));
                }
                let s = self.parse_system_literal()?;
                return Ok((Some(p), Some(s)));
            }
            if !notation {
                self.fatal(43, "SystemLiteral \" or ' expected");
                self.no_subset_error = true;
                return Err(self.fatal(70, "SYSTEM or PUBLIC, the URI is missing"));
            }
            return Ok((Some(p), None));
        }
        if self.incomplete() {
            return Err(Halt::Incomplete);
        }
        Ok((None, None))
    }

    fn parse_system_literal(&mut self) -> R<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(70, "SystemLiteral \" or ' expected"));
        }
        self.pos += 1;
        let start = self.pos;
        while !self.eof() && self.cur() != q {
            self.pos += 1;
        }
        if self.eof() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(71, "Unfinished System or Public ID \" or ' expected"));
        }
        let s = self.slice((start, self.pos)).to_string();
        self.pos += 1;
        Ok(s)
    }

    fn parse_pubid_literal(&mut self) -> R<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(69, "PubidLiteral \" or ' expected"));
        }
        self.pos += 1;
        let start = self.pos;
        while !self.eof() && self.cur() != q {
            let c = self.cur() as char;
            if !is_pubid_char(c) {
                return Err(self.fatal(69, "Unfinished PubidLiteral"));
            }
            self.pos += 1;
        }
        if self.eof() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(71, "Unfinished System or Public ID \" or ' expected"));
        }
        let s = self.slice((start, self.pos)).to_string();
        self.pos += 1;
        Ok(s)
    }

    fn require_space(&mut self, after: &str) -> R<()> {
        if !self.skip_spaces() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(65, format!("Space required after '{after}'")));
        }
        Ok(())
    }

    fn name_or(&mut self, code: u32, msg: &str) -> R<Str> {
        match self.scan_name() {
            Some(r) => {
                let s = self.slice(r).to_string();
                Ok(self.interned_or(&s))
            }
            None => {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                Err(self.fatal(code, msg.to_string()))
            }
        }
    }

    fn end_decl(&mut self) -> R<()> {
        self.skip_spaces();
        if self.cur() != b'>' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(73, "expected '>'"));
        }
        self.pos += 1;
        Ok(())
    }

    fn parse_markup_decl(&mut self, external: bool) -> R<()> {
        if self.starts_with(b"<!ELEMENT") {
            self.pos += 9;
            self.require_space("ELEMENT")?;
            let name = self.name_or(68, "xmlParseElementDecl: no name for Element")?;
            self.require_space("the element name")?;
            let content = if self.starts_with(b"EMPTY") {
                self.pos += 5;
                ContentModel::Empty
            } else if self.starts_with(b"ANY") {
                self.pos += 3;
                ContentModel::Any
            } else if self.cur() == b'(' {
                self.pos += 1;
                self.skip_spaces();
                if self.starts_with(b"#PCDATA") {
                    self.pos += 7;
                    self.parse_mixed()?
                } else {
                    ContentModel::Children(self.parse_children_group()?)
                }
            } else {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                return Err(self.fatal(54, "xmlParseElementDecl: 'EMPTY', 'ANY' or '(' expected"));
            };
            if let Err(e) = self.end_decl() {
                self.no_subset_error = true;
                return Err(e);
            }
            let d = self.dtd.as_mut().unwrap();
            if d.elements.contains_key(&name) {
                // Reported when validating while parsing (observed on PHP 8.5).
                if self.opts.has(super::opt::DTDVALID) {
                    self.error(
                        Domain::Valid,
                        509,
                        format!("Redefinition of element {name}"),
                    );
                }
                return Ok(());
            }
            d.elements.insert(
                name.clone(),
                ElementDecl {
                    name: name.clone(),
                    content,
                },
            );
            d.items.push(DtdItem::Element(name));
            return Ok(());
        }
        if self.starts_with(b"<!ATTLIST") {
            self.pos += 9;
            self.require_space("ATTLIST")?;
            let el = self.name_or(68, "ATTLIST: no name for Element")?;
            loop {
                let had_space = self.skip_spaces();
                if self.cur() == b'>' {
                    self.pos += 1;
                    break;
                }
                if !had_space {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    return Err(self.fatal(65, "Space required after the attribute name"));
                }
                let an = self.name_or(68, "ATTLIST: no name for Attribute")?;
                self.require_space("the attribute name")?;
                let (atype, values) = self.parse_att_type()?;
                self.require_space("the attribute type")?;
                let (default, value) = if self.starts_with(b"#REQUIRED") {
                    self.pos += 9;
                    (AttrDefault::Required, None)
                } else if self.starts_with(b"#IMPLIED") {
                    self.pos += 8;
                    (AttrDefault::Implied, None)
                } else {
                    let mut kind = AttrDefault::None;
                    if self.starts_with(b"#FIXED") {
                        self.pos += 6;
                        self.require_space("'#FIXED'")?;
                        kind = AttrDefault::Fixed;
                    }
                    let v = match self.parse_default_value() {
                        Ok(v) => v,
                        Err(Halt::Fatal) if !self.halted => {
                            self.fatal(39, "Attribute default value declaration error");
                            return Err(
                                self.fatal(65, "Space required after the attribute default value")
                            );
                        }
                        Err(e) => return Err(e),
                    };
                    let v = if atype != AttrType::Cdata {
                        collapse_spaces(&v)
                    } else {
                        v
                    };
                    (kind, Some(v))
                };
                let d = self.dtd.as_mut().unwrap();
                let list = d.attlists.entry(el.clone()).or_default();
                if list.iter().any(|a| a.name == an) {
                    continue;
                }
                list.push(AttrDecl {
                    element: el.clone(),
                    name: an.clone(),
                    atype,
                    values,
                    default,
                    value,
                });
                d.items.push(DtdItem::Attr(el.clone(), an));
            }
            return Ok(());
        }
        if self.starts_with(b"<!ENTITY") {
            self.pos += 8;
            self.require_space("'<!ENTITY'")?;
            let mut parameter = false;
            if self.cur() == b'%' {
                self.pos += 1;
                self.require_space("'%'")?;
                parameter = true;
            }
            let name = self.name_or(68, "xmlParseEntityDecl: no name")?;
            if name.contains(':') && self.opts.namespaces {
                self.error(
                    Domain::Namespace,
                    205,
                    format!("colons are forbidden from entities names '{name}'"),
                );
            }
            self.require_space("the entity name")?;
            let q = self.cur();
            let mut ent = EntityDecl {
                name: name.clone(),
                kind: if parameter {
                    EntityKind::InternalParameter
                } else {
                    EntityKind::InternalGeneral
                },
                value: None,
                orig: None,
                public_id: None,
                system_id: None,
                notation: None,
                checked: false,
            };
            if q == b'"' || q == b'\'' {
                let (orig, value) = self.parse_entity_value(external)?;
                ent.orig = Some(orig);
                ent.value = Some(value);
            } else {
                let (p, s) = self.parse_external_id(false)?;
                if s.is_none() {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    return Err(self.fatal(84, "Entity value required"));
                }
                ent.public_id = p;
                ent.system_id = s;
                ent.kind = if parameter {
                    EntityKind::ExternalParameter
                } else {
                    EntityKind::ExternalGeneralParsed
                };
                let had_space = self.skip_spaces();
                if self.starts_with(b"NDATA") {
                    if parameter {
                        return Err(self.fatal(75, "NDATA not allowed for parameter entities"));
                    }
                    if !had_space {
                        return Err(self.fatal(65, "Space required before 'NDATA'"));
                    }
                    self.pos += 5;
                    self.require_space("'NDATA'")?;
                    let n = self.name_or(68, "NDATA: no notation name")?;
                    ent.notation = Some(n.to_string());
                    ent.kind = EntityKind::ExternalGeneralUnparsed;
                }
            }
            self.end_decl()?;
            if !parameter && crate::tree::predefined_entity(&name).is_some() {
                // Redeclaring a predefined entity: accepted when it maps to
                // the same character (XML 1.0 4.6).
                return Ok(());
            }
            let d = self.dtd.as_mut().unwrap();
            let map = if parameter {
                &mut d.pentities
            } else {
                &mut d.entities
            };
            if map.contains_key(&name) {
                return Ok(());
            }
            map.insert(name.clone(), ent);
            d.items.push(DtdItem::Entity(name, parameter));
            return Ok(());
        }
        if self.starts_with(b"<!NOTATION") {
            self.pos += 10;
            self.require_space("'<!NOTATION'")?;
            let name = self.name_or(68, "Notation: Name expected here")?;
            if !self.skip_spaces() {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                return Err(self.fatal(65, "Space required after the NOTATION name'"));
            }
            let (p, s) = self.parse_external_id(true)?;
            if p.is_none() && s.is_none() {
                return Err(self.fatal(41, "Notation: Name expected here"));
            }
            self.end_decl()?;
            let d = self.dtd.as_mut().unwrap();
            if !d.notations.contains_key(&name) {
                d.notations.insert(
                    name.clone(),
                    NotationDecl {
                        name: name.clone(),
                        public_id: p,
                        system_id: s,
                    },
                );
                d.items.push(DtdItem::Notation(name));
            }
            return Ok(());
        }
        Err(self.fatal(118, "Content error in the internal subset"))
    }

    fn parse_default_value(&mut self) -> R<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(39, "AttValue: \" or ' expected"));
        }
        self.pos += 1;
        let mut out = String::new();
        while !self.eof() && self.cur() != q {
            let b = self.cur();
            match b {
                b'<' => {
                    return Err(self.fatal(38, "Unescaped '<' not allowed in attributes values"));
                }
                b'\t' | b'\n' => {
                    out.push(' ');
                    self.pos += 1;
                }
                b'&' => {
                    let rest_start = self.pos + 1;
                    let end = self.buf[rest_start..]
                        .iter()
                        .position(|&c| c == b';' || c == q);
                    match end {
                        Some(e) if self.buf[rest_start + e] == b';' => {
                            let r = self.slice((rest_start, rest_start + e)).to_string();
                            self.pos = rest_start + e + 1;
                            if let Some(num) = r.strip_prefix('#') {
                                let v = if let Some(h) = num.strip_prefix('x') {
                                    u32::from_str_radix(h, 16).ok()
                                } else {
                                    num.parse().ok()
                                };
                                match v.and_then(char::from_u32) {
                                    Some(c) => out.push(c),
                                    None => {
                                        return Err(self
                                            .halt(9, "xmlParseCharRef: invalid xmlChar value 0"));
                                    }
                                }
                            } else if let Some(c) = crate::tree::predefined_entity(&r) {
                                out.push(c);
                            } else {
                                // WFCs No External Entity References and
                                // Entity Declared apply to defaults too.
                                match self.find_entity(&r, false) {
                                    Some(e) if e.kind == EntityKind::InternalGeneral => {
                                        if !self.expand_in_attr(&e, &mut out, 0)? {
                                            return Err(Halt::Fatal);
                                        }
                                    }
                                    Some(_) => {
                                        return Err(self.fatal(
                                            29,
                                            format!("Attribute references external entity '{r}'"),
                                        ));
                                    }
                                    None => {
                                        if self.has_pe_refs
                                            || self.ext.is_some() && self.standalone != 1
                                        {
                                            out.push('&');
                                            out.push_str(&r);
                                            out.push(';');
                                        } else {
                                            return Err(
                                                self.fatal(26, format!("Entity '{r}' not defined"))
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        _ => return Err(self.fatal(23, "EntityRef: expecting ';'")),
                    }
                }
                _ => {
                    let c = self.char_at(self.pos).unwrap_or('\u{FFFD}');
                    out.push(c);
                    self.pos += c.len_utf8().max(1);
                }
            }
        }
        if self.eof() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(40, "AttValue: ' expected"));
        }
        self.pos += 1;
        Ok(out)
    }

    /// `EntityValue`: returns the literal as written and the replacement
    /// text (character references and parameter entities expanded, general
    /// entity references bypassed, XML 1.0 4.5).
    fn parse_entity_value(&mut self, external: bool) -> R<(String, String)> {
        let q = self.cur();
        self.pos += 1;
        let start = self.pos;
        while !self.eof() && self.cur() != q {
            self.pos += 1;
        }
        if self.eof() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(37, "EntityValue: \" or ' expected"));
        }
        let orig = self.slice((start, self.pos)).to_string();
        self.pos += 1;
        let mut value = String::with_capacity(orig.len());
        let mut chars = orig.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '&' => {
                    let rest = &orig[i + 1..];
                    let Some(end) = rest.find(';') else {
                        return Err(self.fatal(23, "EntityRef: expecting ';'"));
                    };
                    let r = &rest[..end];
                    if let Some(num) = r.strip_prefix('#') {
                        let v = if let Some(h) = num.strip_prefix('x') {
                            u32::from_str_radix(h, 16).ok()
                        } else {
                            num.parse().ok()
                        };
                        match v
                            .filter(|&v| crate::chars::is_xml_char(v))
                            .and_then(char::from_u32)
                        {
                            Some(ch) => value.push(ch),
                            None => {
                                return Err(self.halt(
                                    9,
                                    format!(
                                        "xmlParseCharRef: invalid xmlChar value {}",
                                        v.unwrap_or(0)
                                    ),
                                ));
                            }
                        }
                    } else {
                        if !crate::chars::is_name(r) {
                            return Err(self.fatal(68, "xmlParseEntityRef: no name"));
                        }
                        value.push('&');
                        value.push_str(r);
                        value.push(';');
                    }
                    for _ in 0..=end {
                        chars.next();
                    }
                }
                '%' => {
                    let rest = &orig[i + 1..];
                    let Some(end) = rest.find(';') else {
                        return Err(self.fatal(25, "PEReference: expecting ';'"));
                    };
                    let r = rest[..end].to_string();
                    if !external {
                        let semi = start + i + 1 + end;
                        if self.find_entity(&r, true).is_none() {
                            let lc = self.line_col(semi);
                            let saved = self.err_pos;
                            self.err_pos.get_or_insert(lc);
                            self.warn(Domain::Parser, 27, format!("Entity '{r}' not defined"));
                            self.err_pos = saved;
                            value.push('%');
                            continue;
                        }
                        return Err(self.fatal(88, "PEReferences forbidden in internal subset"));
                    }
                    for _ in 0..=end {
                        chars.next();
                    }
                    match self.find_entity(&r, true) {
                        Some(e) if e.kind == EntityKind::InternalParameter => {
                            value.push_str(&e.value.unwrap_or_default());
                        }
                        Some(e) => {
                            let sys = e.system_id.unwrap_or_default();
                            if let Some(t) = self.load_text(e.public_id.as_deref(), &sys)? {
                                value.push_str(&t);
                            }
                        }
                        None => {
                            self.warn(Domain::Parser, 27, format!("PEReference: %{r}; not found"))
                        }
                    }
                }
                c => value.push(c),
            }
        }
        Ok((orig, value))
    }

    fn parse_att_type(&mut self) -> R<(AttrType, Vec<String>)> {
        let kws: [(&[u8], AttrType); 8] = [
            (b"CDATA", AttrType::Cdata),
            (b"IDREFS", AttrType::IdRefs),
            (b"IDREF", AttrType::IdRef),
            (b"ID", AttrType::Id),
            (b"ENTITY", AttrType::Entity),
            (b"ENTITIES", AttrType::Entities),
            (b"NMTOKENS", AttrType::NmTokens),
            (b"NMTOKEN", AttrType::NmToken),
        ];
        // ENTITIES before ENTITY.
        if self.starts_with(b"ENTITIES") {
            self.pos += 8;
            return Ok((AttrType::Entities, Vec::new()));
        }
        for (kw, t) in kws {
            if self.starts_with(kw) {
                self.pos += kw.len();
                return Ok((t, Vec::new()));
            }
        }
        if self.starts_with(b"NOTATION") {
            self.pos += 8;
            self.require_space("'NOTATION'")?;
            if self.cur() != b'(' {
                return Err(self.fatal(51, "'(' required to start 'NOTATION'"));
            }
            let v = self.parse_enumeration(true)?;
            return Ok((AttrType::Notation, v));
        }
        if self.cur() == b'(' {
            let v = self.parse_enumeration(false)?;
            return Ok((AttrType::Enumeration, v));
        }
        if self.incomplete() {
            return Err(Halt::Incomplete);
        }
        Err(self.fatal(52, "attribute type expected"))
    }

    fn parse_enumeration(&mut self, names: bool) -> R<Vec<String>> {
        self.pos += 1;
        let mut v = Vec::new();
        loop {
            self.skip_spaces();
            let start = self.pos;
            if names {
                if self.scan_name().is_none() {
                    return Err(self.fatal(68, "Name expected in NOTATION declaration"));
                }
            } else {
                while let Some(c) = self.char_at(self.pos) {
                    if !crate::chars::is_name_char(c) {
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                if self.pos == start {
                    return Err(self.fatal(68, "NmToken expected in ATTLIST enumeration"));
                }
            }
            v.push(self.slice((start, self.pos)).to_string());
            self.skip_spaces();
            match self.cur() {
                b'|' => self.pos += 1,
                b')' => {
                    self.pos += 1;
                    return Ok(v);
                }
                _ => {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    return Err(self.fatal(51, "')' required to finish ATTLIST enumeration"));
                }
            }
        }
    }

    fn parse_mixed(&mut self) -> R<ContentModel> {
        let mut names = Vec::new();
        loop {
            self.skip_spaces();
            match self.cur() {
                b')' => {
                    self.pos += 1;
                    if self.cur() == b'*' {
                        self.pos += 1;
                    } else if !names.is_empty() {
                        return Err(
                            self.fatal(51, "xmlParseElementMixedContentDecl : '|' or ')' expected")
                        );
                    }
                    return Ok(ContentModel::Mixed(names));
                }
                b'|' => {
                    self.pos += 1;
                    self.skip_spaces();
                    let n = self.name_or(68, "xmlParseElementMixedContentDecl : Name expected")?;
                    names.push(n);
                }
                _ => {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    return Err(
                        self.fatal(51, "xmlParseElementMixedContentDecl : '|' or ')' expected")
                    );
                }
            }
        }
    }

    fn parse_occur(&mut self) -> Occur {
        let o = match self.cur() {
            b'?' => Occur::Opt,
            b'*' => Occur::Star,
            b'+' => Occur::Plus,
            _ => return Occur::One,
        };
        self.pos += 1;
        o
    }

    /// After `(`: a choice or sequence group.
    fn parse_children_group(&mut self) -> R<Particle> {
        let mut items = Vec::new();
        let mut sep = 0u8;
        loop {
            self.skip_spaces();
            let p = if self.cur() == b'(' {
                self.pos += 1;
                self.parse_children_group()?
            } else {
                let n = self.name_or(
                    68,
                    "xmlParseElementChildrenContentDecl : Name or '(' expected",
                )?;
                let o = self.parse_occur();
                Particle::Name(n, o)
            };
            items.push(p);
            self.skip_spaces();
            match self.cur() {
                b')' => {
                    self.pos += 1;
                    let o = self.parse_occur();
                    return Ok(if sep == b'|' {
                        Particle::Choice(items, o)
                    } else {
                        Particle::Seq(items, o)
                    });
                }
                c @ (b',' | b'|') => {
                    if sep != 0 && sep != c {
                        let want = if sep == b',' { "','" } else { "'|'" };
                        return Err(self.fatal(
                            66,
                            format!("xmlParseElementChildrenContentDecl : {want} expected"),
                        ));
                    }
                    sep = c;
                    self.pos += 1;
                }
                _ => {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    return Err(self.fatal(
                        50,
                        "xmlParseElementChildrenContentDecl : ',' '|' or ')' expected",
                    ));
                }
            }
        }
    }
}
