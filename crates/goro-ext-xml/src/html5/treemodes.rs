//! The insertion modes: a port of lexbor's html/tree/insertion_mode/*.c.
//! Every mode returns `true` when the token was consumed and `false` when
//! it must be reprocessed (in the then-current mode).

use super::dom::LKind;
use super::tree::*;
use super::*;

static DOCTYPE_PUBLIC_IS: [&str; 3] = [
    "-//W3O//DTD W3 HTML Strict 3.0//EN//",
    "-/W3C/DTD HTML 4.0 Transitional/EN",
    "HTML",
];
static DOCTYPE_SYSTEM_IS: [&str; 1] =
    ["http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd"];
static DOCTYPE_PUBLIC_START: [&str; 55] = [
    "+//Silmaril//dtd html Pro v0r11 19970101//",
    "-//AS//DTD HTML 3.0 asWedit + extensions//",
    "-//AdvaSoft Ltd//DTD HTML 3.0 asWedit + extensions//",
    "-//IETF//DTD HTML 2.0 Level 1//",
    "-//IETF//DTD HTML 2.0 Level 2//",
    "-//IETF//DTD HTML 2.0 Strict Level 1//",
    "-//IETF//DTD HTML 2.0 Strict Level 2//",
    "-//IETF//DTD HTML 2.0 Strict//",
    "-//IETF//DTD HTML 2.0//",
    "-//IETF//DTD HTML 2.1E//",
    "-//IETF//DTD HTML 3.0//",
    "-//IETF//DTD HTML 3.2 Final//",
    "-//IETF//DTD HTML 3.2//",
    "-//IETF//DTD HTML 3//",
    "-//IETF//DTD HTML Level 0//",
    "-//IETF//DTD HTML Level 1//",
    "-//IETF//DTD HTML Level 2//",
    "-//IETF//DTD HTML Level 3//",
    "-//IETF//DTD HTML Strict Level 0//",
    "-//IETF//DTD HTML Strict Level 1//",
    "-//IETF//DTD HTML Strict Level 2//",
    "-//IETF//DTD HTML Strict Level 3//",
    "-//IETF//DTD HTML Strict//",
    "-//IETF//DTD HTML//",
    "-//Metrius//DTD Metrius Presentational//",
    "-//Microsoft//DTD Internet Explorer 2.0 HTML Strict//",
    "-//Microsoft//DTD Internet Explorer 2.0 HTML//",
    "-//Microsoft//DTD Internet Explorer 2.0 Tables//",
    "-//Microsoft//DTD Internet Explorer 3.0 HTML Strict//",
    "-//Microsoft//DTD Internet Explorer 3.0 HTML//",
    "-//Microsoft//DTD Internet Explorer 3.0 Tables//",
    "-//Netscape Comm. Corp.//DTD HTML//",
    "-//Netscape Comm. Corp.//DTD Strict HTML//",
    "-//O'Reilly and Associates//DTD HTML 2.0//",
    "-//O'Reilly and Associates//DTD HTML Extended 1.0//",
    "-//O'Reilly and Associates//DTD HTML Extended Relaxed 1.0//",
    "-//SQ//DTD HTML 2.0 HoTMetaL + extensions//",
    "-//SoftQuad Software//DTD HoTMetaL PRO 6.0::19990601::extensions to HTML 4.0//",
    "-//SoftQuad//DTD HoTMetaL PRO 4.0::19971010::extensions to HTML 4.0//",
    "-//Spyglass//DTD HTML 2.0 Extended//",
    "-//Sun Microsystems Corp.//DTD HotJava HTML//",
    "-//Sun Microsystems Corp.//DTD HotJava Strict HTML//",
    "-//W3C//DTD HTML 3 1995-03-24//",
    "-//W3C//DTD HTML 3.2 Draft//",
    "-//W3C//DTD HTML 3.2 Final//",
    "-//W3C//DTD HTML 3.2//",
    "-//W3C//DTD HTML 3.2S Draft//",
    "-//W3C//DTD HTML 4.0 Frameset//",
    "-//W3C//DTD HTML 4.0 Transitional//",
    "-//W3C//DTD HTML Experimental 19960712//",
    "-//W3C//DTD HTML Experimental 970421//",
    "-//W3C//DTD W3 HTML//",
    "-//W3O//DTD W3 HTML 3.0//",
    "-//WebTechs//DTD Mozilla HTML 2.0//",
    "-//WebTechs//DTD Mozilla HTML//",
];
static DOCTYPE_SYS_PUB_START: [&str; 2] = [
    "-//W3C//DTD HTML 4.01 Frameset//",
    "-//W3C//DTD HTML 4.01 Transitional//",
];
static DOCTYPE_LIM_PUB_START: [&str; 2] = [
    "-//W3C//DTD XHTML 1.0 Frameset//",
    "-//W3C//DTD XHTML 1.0 Transitional//",
];

fn starts_ci(s: &[u8], prefix: &str) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

impl Parser {
    pub(crate) fn mode_call(&mut self, m: Mode, tok: &mut Token) -> bool {
        match m {
            Mode::Initial => self.im_initial(tok),
            Mode::BeforeHtml => self.im_before_html(tok),
            Mode::BeforeHead => self.im_before_head(tok),
            Mode::InHead => self.im_in_head(tok),
            Mode::InHeadNoscript => self.im_in_head_noscript(tok),
            Mode::AfterHead => self.im_after_head(tok),
            Mode::InBody => self.im_in_body(tok),
            Mode::InBodySkipNewLine | Mode::InBodySkipNewLineTextarea => {
                self.im_in_body_skip_new_line(tok)
            }
            Mode::Text => self.im_text(tok),
            Mode::InTable => self.im_in_table(tok),
            Mode::InTableText => self.im_in_table_text(tok),
            Mode::InCaption => self.im_in_caption(tok),
            Mode::InColumnGroup => self.im_in_column_group(tok),
            Mode::InTableBody => self.im_in_table_body(tok),
            Mode::InRow => self.im_in_row(tok),
            Mode::InCell => self.im_in_cell(tok),
            Mode::InSelect => self.im_in_select(tok),
            Mode::InSelectInTable => self.im_in_select_in_table(tok),
            Mode::InTemplate => self.im_in_template(tok),
            Mode::AfterBody => self.im_after_body(tok),
            Mode::InFrameset => self.im_in_frameset(tok),
            Mode::AfterFrameset => self.im_after_frameset(tok),
            Mode::AfterAfterBody => self.im_after_after_body(tok),
            Mode::AfterAfterFrameset => self.im_after_after_frameset(tok),
        }
    }

    fn closed(tok: &Token) -> bool {
        tok.typ & TOKEN_CLOSE != 0
    }

    // ---------------------------------------------------------------------
    // initial.c

    fn im_initial(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _EM_COMMENT => {
                let d = self.document;
                self.insert_comment(tok, Some(d));
                true
            }
            _EM_DOCTYPE => {
                self.mode = Mode::BeforeHtml;
                self.initial_doctype(tok)
            }
            t => {
                if t == _TEXT {
                    self.tok_skip_ws_begin(tok);
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.parse_error(tok, UNTOININMO);
                self.compat_mode = CMODE_QUIRKS;
                self.mode = Mode::BeforeHtml;
                false
            }
        }
    }

    fn initial_doctype(&mut self, tok: &mut Token) -> bool {
        let dt = self.create_doctype_from_token(tok);
        let (is_html, public, system) = {
            let n = &self.dom.nodes[dt];
            (n.data == b"html", n.public_id.clone(), n.system_id.clone())
        };
        if !is_html
            || !public.is_empty()
            || (system.len() == 19 && system != b"about:legacy-compat")
        {
            self.parse_error(tok, BADOTOININMO);
        }
        // doctype check
        let quirks = tok.typ & TOKEN_FORCE_QUIRKS != 0
            || !is_html
            || (!public.is_empty()
                && (DOCTYPE_PUBLIC_IS
                    .iter()
                    .any(|s| public.eq_ignore_ascii_case(s.as_bytes()))
                    || DOCTYPE_PUBLIC_START.iter().any(|s| starts_ci(&public, s))))
            || (!system.is_empty()
                && DOCTYPE_SYSTEM_IS
                    .iter()
                    .any(|s| system.eq_ignore_ascii_case(s.as_bytes())))
            || (!public.is_empty()
                && system.is_empty()
                && DOCTYPE_SYS_PUB_START.iter().any(|s| starts_ci(&public, s)));
        if quirks {
            self.compat_mode = CMODE_QUIRKS;
        } else if !public.is_empty() {
            let limq = (!system.is_empty()
                && DOCTYPE_SYS_PUB_START.iter().any(|s| starts_ci(&public, s)))
                || DOCTYPE_LIM_PUB_START.iter().any(|s| starts_ci(&public, s));
            if limq {
                self.compat_mode = CMODE_LIMITED_QUIRKS;
            }
        }
        let d = self.document;
        self.dom.append_child(d, dt);
        true
    }

    // ---------------------------------------------------------------------
    // before_html.c

    fn before_html_html(&mut self, html: NodeId) {
        self.open_elements.push(html);
        let d = self.document;
        self.dom.append_child(d, html);
    }

    fn before_html_anything_else(&mut self) -> bool {
        let html = self.dom.new_element(HTML, NS_HTML);
        self.before_html_html(html);
        self.mode = Mode::BeforeHead;
        false
    }

    fn im_before_html(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                HEAD | BODY | HTML | BR => self.before_html_anything_else(),
                _ => {
                    self.parse_error(tok, UNCLTOINBEHTMO);
                    true
                }
            };
        }
        match tok.tag_id {
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINBEHTMO);
                true
            }
            _EM_COMMENT => {
                let d = self.document;
                self.insert_comment(tok, Some(d));
                true
            }
            HTML => {
                let el = self.create_element_for_token(tok, NS_HTML);
                self.before_html_html(el);
                self.has_explicit_html_tag = true;
                self.mode = Mode::BeforeHead;
                true
            }
            t => {
                if t == _TEXT {
                    self.tok_skip_ws_begin(tok);
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.before_html_anything_else()
            }
        }
    }

    // ---------------------------------------------------------------------
    // before_head.c

    fn before_head_head(&mut self, tok: &mut Token) {
        let el = self.insert_html_element(tok);
        self.head = Some(el);
    }

    fn before_head_anything_else(&mut self) -> bool {
        let mut fake = Token {
            tag_id: HEAD,
            ..Default::default()
        };
        self.before_head_head(&mut fake);
        self.mode = Mode::InHead;
        false
    }

    fn im_before_head(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                HEAD | BODY | HTML | BR => self.before_head_anything_else(),
                _ => {
                    self.parse_error(tok, UNCLTOINBEHEMO);
                    true
                }
            };
        }
        match tok.tag_id {
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINBEHEMO);
                true
            }
            HTML => self.im_in_body(tok),
            HEAD => {
                self.before_head_head(tok);
                self.has_explicit_head_tag = true;
                self.mode = Mode::InHead;
                true
            }
            t => {
                if t == _TEXT {
                    self.tok_skip_ws_begin(tok);
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.before_head_anything_else()
            }
        }
    }

    // ---------------------------------------------------------------------
    // in_head.c

    fn in_head_anything_else(&mut self) -> bool {
        self.oe_pop();
        self.mode = Mode::AfterHead;
        false
    }

    fn im_in_head(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                HEAD => {
                    self.oe_pop();
                    self.mode = Mode::AfterHead;
                    true
                }
                BODY | HTML | BR => self.in_head_anything_else(),
                TEMPLATE => self.in_head_template_closed(tok),
                _ => {
                    self.parse_error(tok, UNCLTOINHEMO);
                    true
                }
            };
        }
        match tok.tag_id {
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINHEMO);
                true
            }
            HTML => self.im_in_body(tok),
            BASE | BASEFONT | BGSOUND | LINK | META => {
                self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                true
            }
            TITLE => {
                self.generic_rcdata_parsing(tok);
                true
            }
            NOSCRIPT => {
                if self.scripting {
                    self.generic_rawtext_parsing(tok);
                } else {
                    self.insert_html_element(tok);
                    self.mode = Mode::InHeadNoscript;
                }
                true
            }
            NOFRAMES | STYLE => {
                self.generic_rawtext_parsing(tok);
                true
            }
            SCRIPT => {
                let place = self.appropriate_place(None);
                let el = self.create_element_for_token(tok, NS_HTML);
                self.open_elements.push(el);
                if let Some((pos, before)) = place {
                    self.insert_node(pos, el, before);
                }
                self.set_tmp_tag_id(tok.tag_id);
                self.set_tkz_state(St::ScriptDataBefore);
                self.original_mode = self.mode;
                self.mode = Mode::Text;
                true
            }
            TEMPLATE => {
                self.insert_html_element(tok);
                self.af_push_marker();
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.template_modes.push(Mode::InTemplate);
                true
            }
            HEAD => {
                self.parse_error(tok, HETOINHEMO);
                true
            }
            t => {
                if t == _TEXT {
                    let ws = self.tok_split_ws_begin(tok);
                    if ws.text_start != ws.text_end {
                        self.insert_character(&ws);
                    }
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.in_head_anything_else()
            }
        }
    }

    fn in_head_template_closed(&mut self, tok: &mut Token) -> bool {
        if self.oe_find_reverse(TEMPLATE, NS_HTML).is_none() {
            self.parse_error(tok, TECLTOWIOPINHEMO);
            return true;
        }
        self.generate_all_implied_end_tags_thoroughly(_UNDEF, NS_UNDEF);
        if self
            .current_node()
            .map(|c| !self.node_is(c, TEMPLATE))
            .unwrap_or(true)
        {
            self.parse_error(tok, TEELISNOCUINHEMO);
        }
        self.oe_pop_until_tag(TEMPLATE, NS_HTML, true);
        self.af_up_to_last_marker();
        self.template_modes.pop();
        self.reset_insertion_mode_appropriately();
        true
    }

    // ---------------------------------------------------------------------
    // in_head_noscript.c

    fn in_head_noscript_anything_else(&mut self, tok: &mut Token) -> bool {
        self.parse_error(tok, UNTO);
        self.oe_pop();
        self.mode = Mode::InHead;
        false
    }

    fn im_in_head_noscript(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            if tok.tag_id == BR {
                return self.in_head_noscript_anything_else(tok);
            }
            self.parse_error(tok, UNTO);
            return true;
        }
        match tok.tag_id {
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINHENOMO);
                true
            }
            HTML => self.im_in_body(tok),
            _EM_COMMENT | BASEFONT | BGSOUND | LINK | META | NOFRAMES | STYLE => {
                self.im_in_head(tok)
            }
            HEAD | NOSCRIPT => {
                self.parse_error(tok, UNTO);
                true
            }
            t => {
                if t == _TEXT {
                    let ws = self.tok_split_ws_begin(tok);
                    if ws.text_start != ws.text_end {
                        self.insert_character(&ws);
                    }
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.in_head_noscript_anything_else(tok)
            }
        }
    }

    // ---------------------------------------------------------------------
    // after_head.c

    fn after_head_create_body(&mut self, tok: &mut Token) {
        self.insert_html_element(tok);
    }

    fn after_head_anything_else(&mut self) -> bool {
        let mut fake = Token {
            tag_id: BODY,
            ..Default::default()
        };
        self.after_head_create_body(&mut fake);
        self.mode = Mode::InBody;
        false
    }

    fn im_after_head(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TEMPLATE => self.im_in_head(tok),
                BODY | HTML | BR => self.after_head_anything_else(),
                _ => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
            };
        }
        match tok.tag_id {
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOAFHEMO);
                true
            }
            HTML => self.im_in_body(tok),
            BODY => {
                self.after_head_create_body(tok);
                self.has_explicit_body_tag = true;
                self.frameset_ok = false;
                self.mode = Mode::InBody;
                true
            }
            FRAMESET => {
                self.insert_html_element(tok);
                self.mode = Mode::InFrameset;
                true
            }
            BASE | BASEFONT | BGSOUND | LINK | META | NOFRAMES | SCRIPT | STYLE | TEMPLATE
            | TITLE => {
                let head = match self.head {
                    Some(h) => h,
                    None => return true,
                };
                self.parse_error(tok, UNTO);
                self.open_elements.push(head);
                self.im_in_head(tok);
                self.oe_remove_by_node(head);
                true
            }
            HEAD => {
                self.parse_error(tok, HETOAFHEMO);
                true
            }
            t => {
                if t == _TEXT {
                    let ws = self.tok_split_ws_begin(tok);
                    if ws.text_start != ws.text_end {
                        self.insert_character(&ws);
                    }
                    if tok.text_start == tok.text_end {
                        return true;
                    }
                }
                self.after_head_anything_else()
            }
        }
    }

    // ---------------------------------------------------------------------
    // in_body.c

    fn im_in_body_skip_new_line(&mut self, tok: &mut Token) -> bool {
        self.mode = self.original_mode;
        if tok.tag_id != _TEXT {
            return false;
        }
        self.tok_skip_one_newline_begin(tok);
        tok.text_start == tok.text_end
    }

    fn text_without_nulls(&mut self, tok: &Token) -> Vec<u8> {
        let mut s = self.tok_text(tok);
        if tok.null_count != 0 {
            self.parse_error(tok, NUCH);
            s.retain(|&c| c != 0);
        }
        s
    }

    fn in_body_text(&mut self, tok: &mut Token) -> bool {
        let s = self.text_without_nulls(tok);
        if s.is_empty() {
            return true;
        }
        self.in_body_text_append(s);
        true
    }

    /// lxb_html_tree_insertion_mode_in_body_text_append
    pub(crate) fn in_body_text_append(&mut self, s: Vec<u8>) {
        self.af_reconstruct();
        if self.frameset_ok
            && s.iter()
                .any(|&c| !matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20))
        {
            self.frameset_ok = false;
        }
        self.insert_character_for_data(s);
    }

    fn in_body_close_p_if_in_button_scope(&mut self, tok: &Token) {
        if self
            .element_in_scope(P, NS_HTML, CAT_SCOPE_BUTTON)
            .is_some()
        {
            self.close_p_element(tok);
        }
    }

    fn in_body_anything_else_closed(&mut self, tok: &mut Token) -> bool {
        let mut len = self.open_elements.len();
        while len != 0 {
            len -= 1;
            let n = self.open_elements[len];
            if self.node_is(n, tok.tag_id) {
                self.generate_implied_end_tags(tok.tag_id, NS_HTML);
                if Some(n) != self.current_node() {
                    self.parse_error(tok, UNELINOPELST);
                }
                self.oe_pop_until_node(n, true);
                return true;
            }
            if tag_is_category(self.ntag(n), self.nns(n), CAT_SPECIAL) {
                self.parse_error(tok, UNCLTO);
                return true;
            }
        }
        true
    }

    fn in_body_generic_closed(&mut self, tok: &mut Token) -> bool {
        if self
            .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE)
            .is_none()
        {
            self.parse_error(tok, UNCLTO);
            return true;
        }
        self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
        if self
            .current_node()
            .map(|c| !self.node_is(c, tok.tag_id))
            .unwrap_or(true)
        {
            self.parse_error(tok, UNELINOPELST);
        }
        self.oe_pop_until_tag(tok.tag_id, NS_HTML, true);
        true
    }

    fn in_body_abeikw(&mut self, tok: &mut Token) -> bool {
        self.af_reconstruct();
        self.insert_html_element(tok);
        self.oe_pop();
        self.acknowledge_self_closing(tok);
        self.frameset_ok = false;
        true
    }

    fn in_body_reconstruct_insert(&mut self, tok: &mut Token) -> bool {
        self.af_reconstruct();
        self.insert_html_element(tok);
        true
    }

    pub(crate) fn im_in_body(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TEMPLATE => self.im_in_head(tok),
                BODY | HTML => {
                    if self.element_in_scope(BODY, NS_HTML, CAT_SCOPE).is_none() {
                        self.parse_error(tok, NOBOELINSC);
                        return true;
                    }
                    if !self.check_scope_element() {
                        self.parse_error(tok, OPELISWR);
                    }
                    self.mode = Mode::AfterBody;
                    tok.tag_id == BODY
                }
                ADDRESS | ARTICLE | ASIDE | BLOCKQUOTE | BUTTON | CENTER | DETAILS | DIALOG
                | DIR | DIV | DL | FIELDSET | FIGCAPTION | FIGURE | FOOTER | HEADER | HGROUP
                | LISTING | MAIN | MENU | NAV | OL | PRE | SECTION | SUMMARY | UL => {
                    self.in_body_generic_closed(tok)
                }
                FORM => self.in_body_form_closed(tok),
                P => {
                    if self
                        .element_in_scope(P, NS_HTML, CAT_SCOPE_BUTTON)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        let mut fake = Token {
                            tag_id: P,
                            ..Default::default()
                        };
                        self.insert_html_element(&mut fake);
                    }
                    self.close_p_element(tok);
                    true
                }
                LI => {
                    if self
                        .element_in_scope(LI, NS_HTML, CAT_SCOPE_LIST_ITEM)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.generate_implied_end_tags(LI, NS_HTML);
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, LI))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                    }
                    self.oe_pop_until_tag(LI, NS_HTML, true);
                    true
                }
                DD | DT => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.generate_implied_end_tags(tok.tag_id, NS_HTML);
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, tok.tag_id))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                    }
                    self.oe_pop_until_tag(tok.tag_id, NS_HTML, true);
                    true
                }
                H1 | H2 | H3 | H4 | H5 | H6 => {
                    if self.element_in_scope_h123456().is_none() {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, tok.tag_id))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                    }
                    self.oe_pop_until_h123456();
                    true
                }
                A | B | BIG | CODE | EM | FONT | I | NOBR | S | SMALL | STRIKE | STRONG | TT
                | U => {
                    self.adoption_agency(tok);
                    true
                }
                APPLET | MARQUEE | OBJECT => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, tok.tag_id))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                    }
                    self.oe_pop_until_tag(tok.tag_id, NS_HTML, true);
                    self.af_up_to_last_marker();
                    true
                }
                BR => {
                    tok.typ &= !TOKEN_CLOSE;
                    tok.attrs.clear();
                    self.in_body_abeikw(tok)
                }
                _ => self.in_body_anything_else_closed(tok),
            };
        }
        match tok.tag_id {
            _TEXT => self.in_body_text(tok),
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINBOMO);
                true
            }
            HTML => {
                self.parse_error(tok, UNTO);
                if self.oe_find(TEMPLATE, NS_HTML).is_some() {
                    return true;
                }
                if let Some(html) = self.oe_get(0) {
                    let ns = self.nns(html);
                    self.append_attributes(html, tok, ns);
                }
                true
            }
            BASE | BASEFONT | BGSOUND | LINK | META | NOFRAMES | SCRIPT | STYLE | TEMPLATE
            | TITLE => self.im_in_head(tok),
            BODY => {
                self.parse_error(tok, UNTO);
                let node = match self.oe_get(1) {
                    Some(n) if self.ntag(n) == BODY => n,
                    _ => return true,
                };
                if self.oe_find_reverse(TEMPLATE, NS_HTML).is_some() {
                    return true;
                }
                self.frameset_ok = false;
                let ns = self.nns(node);
                self.append_attributes(node, tok, ns);
                true
            }
            FRAMESET => {
                self.parse_error(tok, UNTO);
                let node = match self.oe_get(1) {
                    Some(n) if self.ntag(n) == BODY => n,
                    _ => return true,
                };
                if !self.frameset_ok {
                    return true;
                }
                self.dom.remove(node);
                if let Some(first) = self.oe_get(0) {
                    self.oe_pop_until_node(first, false);
                }
                self.insert_html_element(tok);
                self.mode = Mode::InFrameset;
                true
            }
            _END_OF_FILE => {
                if !self.template_modes.is_empty() {
                    return self.im_in_template(tok);
                }
                if !self.check_scope_element() {
                    self.parse_error(tok, BAENOPELISWR);
                }
                true
            }
            ADDRESS | ARTICLE | ASIDE | BLOCKQUOTE | CENTER | DETAILS | DIALOG | DIR | DIV | DL
            | FIELDSET | FIGCAPTION | FIGURE | FOOTER | HEADER | HGROUP | MAIN | MENU | NAV
            | OL | P | SECTION | SUMMARY | UL => {
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                true
            }
            H1 | H2 | H3 | H4 | H5 | H6 => {
                self.in_body_close_p_if_in_button_scope(tok);
                if let Some(c) = self.current_node() {
                    if matches!(self.ntag(c), H1 | H2 | H3 | H4 | H5 | H6) {
                        self.parse_error(tok, UNELINOPELST);
                        self.oe_pop();
                    }
                }
                self.insert_html_element(tok);
                true
            }
            PRE | LISTING => {
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                self.original_mode = self.mode;
                self.mode = Mode::InBodySkipNewLine;
                self.frameset_ok = false;
                true
            }
            FORM => {
                let temp = self.oe_find_reverse(TEMPLATE, NS_HTML);
                if self.form.is_some() && temp.is_none() {
                    self.parse_error(tok, UNTO);
                    return true;
                }
                self.in_body_close_p_if_in_button_scope(tok);
                let el = self.insert_html_element(tok);
                if temp.is_none() {
                    self.form = Some(el);
                }
                true
            }
            LI => {
                self.frameset_ok = false;
                let mut idx = self.open_elements.len();
                while idx != 0 {
                    idx -= 1;
                    let node = self.open_elements[idx];
                    if self.node_is(node, LI) {
                        self.generate_implied_end_tags(LI, NS_HTML);
                        if self
                            .current_node()
                            .map(|c| !self.node_is(c, LI))
                            .unwrap_or(true)
                        {
                            self.parse_error(tok, UNELINOPELST);
                        }
                        self.oe_pop_until_tag(LI, NS_HTML, true);
                        break;
                    }
                    if tag_is_category(self.ntag(node), self.nns(node), CAT_SPECIAL)
                        && !self.node_is(node, ADDRESS)
                        && !self.node_is(node, DIV)
                        && !self.node_is(node, P)
                    {
                        break;
                    }
                }
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                true
            }
            DD | DT => {
                self.frameset_ok = false;
                let mut idx = self.open_elements.len();
                while idx != 0 {
                    idx -= 1;
                    let node = self.open_elements[idx];
                    let mut matched = None;
                    if self.node_is(node, DD) {
                        matched = Some(DD);
                    } else if self.node_is(node, DT) {
                        matched = Some(DT);
                    }
                    if let Some(t) = matched {
                        self.generate_implied_end_tags(t, NS_HTML);
                        if self
                            .current_node()
                            .map(|c| !self.node_is(c, t))
                            .unwrap_or(true)
                        {
                            self.parse_error(tok, UNELINOPELST);
                        }
                        self.oe_pop_until_tag(t, NS_HTML, true);
                        break;
                    }
                    if tag_is_category(self.ntag(node), self.nns(node), CAT_SPECIAL)
                        && !self.node_is(node, ADDRESS)
                        && !self.node_is(node, DIV)
                        && !self.node_is(node, P)
                    {
                        break;
                    }
                }
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                true
            }
            PLAINTEXT => {
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                self.set_tkz_state(St::PlaintextBefore);
                true
            }
            BUTTON => {
                if self.element_in_scope(BUTTON, NS_HTML, CAT_SCOPE).is_some() {
                    self.parse_error(tok, UNTO);
                    self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
                    self.oe_pop_until_tag(BUTTON, NS_HTML, true);
                }
                self.af_reconstruct();
                self.insert_html_element(tok);
                self.frameset_ok = false;
                true
            }
            A => {
                if let Some(node) = self.af_between_last_marker(A) {
                    self.parse_error(tok, UNELINACFOST);
                    self.adoption_agency(tok);
                    self.af_remove_by_node(node);
                    self.oe_remove_by_node(node);
                }
                self.af_reconstruct();
                let el = self.insert_html_element(tok);
                self.af_push_with_check_dupl(el);
                true
            }
            B | BIG | CODE | EM | FONT | I | S | SMALL | STRIKE | STRONG | TT | U => {
                self.af_reconstruct();
                let el = self.insert_html_element(tok);
                self.af_push_with_check_dupl(el);
                true
            }
            NOBR => {
                self.af_reconstruct();
                if self.element_in_scope(NOBR, NS_HTML, CAT_SCOPE).is_some() {
                    self.parse_error(tok, UNELINSC);
                    self.adoption_agency(tok);
                    self.af_reconstruct();
                }
                let el = self.insert_html_element(tok);
                self.af_push_with_check_dupl(el);
                true
            }
            APPLET | MARQUEE | OBJECT => {
                self.af_reconstruct();
                self.insert_html_element(tok);
                self.af_push_marker();
                self.frameset_ok = false;
                true
            }
            TABLE => {
                if self.compat_mode != CMODE_QUIRKS {
                    self.in_body_close_p_if_in_button_scope(tok);
                }
                self.insert_html_element(tok);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
                true
            }
            AREA | BR | EMBED | IMG | KEYGEN | WBR => self.in_body_abeikw(tok),
            INPUT => {
                self.af_reconstruct();
                let el = self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                match self.dom.attr_find(el, b"type") {
                    Some(i) => {
                        let hidden =
                            self.dom.nodes[el].attrs[i].value.as_deref() == Some(b"hidden");
                        if !hidden {
                            self.frameset_ok = false;
                        }
                    }
                    None => self.frameset_ok = false,
                }
                true
            }
            PARAM | SOURCE | TRACK => {
                self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                true
            }
            HR => {
                self.in_body_close_p_if_in_button_scope(tok);
                self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                self.frameset_ok = false;
                true
            }
            IMAGE => {
                self.parse_error(tok, UNTO);
                tok.tag_id = IMG;
                false
            }
            TEXTAREA => {
                self.insert_html_element(tok);
                self.set_tmp_tag_id(TEXTAREA);
                self.set_tkz_state(St::RcdataBefore);
                self.frameset_ok = false;
                self.original_mode = self.mode;
                self.mode = Mode::InBodySkipNewLineTextarea;
                true
            }
            XMP => {
                self.in_body_close_p_if_in_button_scope(tok);
                self.af_reconstruct();
                self.frameset_ok = false;
                self.generic_rawtext_parsing(tok);
                true
            }
            IFRAME => {
                self.frameset_ok = false;
                self.generic_rawtext_parsing(tok);
                true
            }
            NOEMBED => {
                self.generic_rawtext_parsing(tok);
                true
            }
            NOSCRIPT => {
                if !self.scripting {
                    return self.in_body_reconstruct_insert(tok);
                }
                self.generic_rawtext_parsing(tok);
                true
            }
            SELECT => {
                self.af_reconstruct();
                self.insert_html_element(tok);
                self.frameset_ok = false;
                self.mode = if matches!(
                    self.mode,
                    Mode::InTable
                        | Mode::InCaption
                        | Mode::InTableBody
                        | Mode::InRow
                        | Mode::InCell
                ) {
                    Mode::InSelectInTable
                } else {
                    Mode::InSelect
                };
                true
            }
            OPTGROUP | OPTION => {
                if let Some(c) = self.current_node() {
                    if self.node_is(c, OPTION) {
                        self.oe_pop();
                    }
                }
                self.af_reconstruct();
                self.insert_html_element(tok);
                true
            }
            RB | RTC => {
                if self.element_in_scope(RUBY, NS_HTML, CAT_SCOPE).is_some() {
                    self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
                }
                if self
                    .current_node()
                    .map(|c| !self.node_is(c, RUBY))
                    .unwrap_or(true)
                {
                    self.parse_error(tok, MIELINOPELST);
                }
                self.insert_html_element(tok);
                true
            }
            RP | RT => {
                if self.element_in_scope(RUBY, NS_HTML, CAT_SCOPE).is_some() {
                    self.generate_implied_end_tags(RTC, NS_HTML);
                }
                // lexbor: `!rtc || !ruby` is always true
                if let Some(c) = self.current_node() {
                    if !self.node_is(c, RTC) || !self.node_is(c, RUBY) {
                        self.parse_error(tok, MIELINOPELST);
                    }
                }
                self.insert_html_element(tok);
                true
            }
            MATH => {
                self.af_reconstruct();
                self.set_attr_adjust_mathml();
                self.insert_foreign_element(tok, NS_MATH);
                self.clear_attr_adjust();
                if tok.typ & TOKEN_CLOSE_SELF != 0 {
                    self.oe_pop();
                    self.acknowledge_self_closing(tok);
                }
                true
            }
            SVG => {
                self.af_reconstruct();
                self.set_attr_adjust_svg();
                self.insert_foreign_element(tok, NS_SVG);
                self.clear_attr_adjust();
                if tok.typ & TOKEN_CLOSE_SELF != 0 {
                    self.oe_pop();
                    self.acknowledge_self_closing(tok);
                }
                true
            }
            CAPTION | COL | COLGROUP | FRAME | HEAD | TBODY | TD | TFOOT | TH | THEAD | TR => {
                self.parse_error(tok, UNTO);
                true
            }
            _ => self.in_body_reconstruct_insert(tok),
        }
    }

    fn in_body_form_closed(&mut self, tok: &mut Token) -> bool {
        if self.oe_find_reverse(TEMPLATE, NS_HTML).is_none() {
            let node = self.form.take();
            let node = match node {
                None => {
                    self.parse_error(tok, UNCLTO);
                    return true;
                }
                Some(n) => n,
            };
            if self.element_in_scope_by_node(node, CAT_SCOPE).is_none() {
                self.parse_error(tok, UNCLTO);
                return true;
            }
            self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
            if self.current_node() != Some(node) {
                self.parse_error(tok, UNELINOPELST);
            }
            self.oe_remove_by_node(node);
            return true;
        }
        if self.element_in_scope(FORM, NS_HTML, CAT_SCOPE).is_none() {
            self.parse_error(tok, UNCLTO);
            return true;
        }
        self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
        if self
            .current_node()
            .map(|c| !self.node_is(c, FORM))
            .unwrap_or(true)
        {
            self.parse_error(tok, UNELINOPELST);
        }
        self.oe_pop_until_tag(FORM, NS_HTML, true);
        true
    }

    // ---------------------------------------------------------------------
    // text.c

    fn im_text(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _TEXT => {
                self.insert_character(tok);
                true
            }
            _END_OF_FILE => {
                self.parse_error(tok, UNENOFFI);
                self.oe_pop();
                self.mode = self.original_mode;
                false
            }
            _ => {
                self.oe_pop();
                self.mode = self.original_mode;
                true
            }
        }
    }

    // ---------------------------------------------------------------------
    // in_table.c

    fn clear_stack_back_to(&mut self, tags: &[u32]) {
        while let Some(c) = self.current_node() {
            if tags.contains(&self.ntag(c)) && self.nns(c) == NS_HTML {
                break;
            }
            self.oe_pop();
        }
    }

    fn clear_stack_back_to_table_context(&mut self) {
        self.clear_stack_back_to(&[TABLE, TEMPLATE, HTML]);
    }

    pub(crate) fn in_table_anything_else(&mut self, tok: &mut Token) -> bool {
        self.foster_parenting = true;
        self.im_in_body(tok);
        self.foster_parenting = false;
        true
    }

    fn im_in_table(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TABLE => {
                    let node = match self.element_in_scope(TABLE, NS_HTML, CAT_SCOPE_TABLE) {
                        None => {
                            self.parse_error(tok, UNCLTO);
                            return true;
                        }
                        Some(n) => n,
                    };
                    self.oe_pop_until_node(node, true);
                    self.reset_insertion_mode_appropriately();
                    true
                }
                BODY | CAPTION | COL | COLGROUP | HTML | TBODY | TD | TFOOT | TH | THEAD | TR => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                TEMPLATE => self.im_in_head(tok),
                _ => self.in_table_anything_else(tok),
            };
        }
        match tok.tag_id {
            _TEXT => {
                let node = self.current_node();
                if let Some(n) = node {
                    if self.nns(n) == NS_HTML
                        && matches!(self.ntag(n), TABLE | TBODY | TFOOT | THEAD | TR)
                    {
                        self.pending_table.clear();
                        self.pending_have_non_ws = false;
                        self.original_mode = self.mode;
                        self.mode = Mode::InTableText;
                        return false;
                    }
                }
                self.in_table_anything_else(tok)
            }
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINTAMO);
                true
            }
            CAPTION => {
                self.clear_stack_back_to_table_context();
                self.af_push_marker();
                self.insert_html_element(tok);
                self.mode = Mode::InCaption;
                true
            }
            COLGROUP => {
                self.clear_stack_back_to_table_context();
                self.insert_html_element(tok);
                self.mode = Mode::InColumnGroup;
                true
            }
            COL => {
                self.clear_stack_back_to_table_context();
                let mut fake = Token {
                    tag_id: COLGROUP,
                    ..Default::default()
                };
                self.insert_html_element(&mut fake);
                self.mode = Mode::InColumnGroup;
                false
            }
            TBODY | TFOOT | THEAD => {
                self.clear_stack_back_to_table_context();
                self.insert_html_element(tok);
                self.mode = Mode::InTableBody;
                true
            }
            TD | TH | TR => {
                self.clear_stack_back_to_table_context();
                let mut fake = Token {
                    tag_id: TBODY,
                    ..Default::default()
                };
                self.insert_html_element(&mut fake);
                self.mode = Mode::InTableBody;
                false
            }
            TABLE => {
                self.parse_error(tok, UNTO);
                let node = match self.element_in_scope(TABLE, NS_HTML, CAT_SCOPE_TABLE) {
                    None => return true,
                    Some(n) => n,
                };
                self.oe_pop_until_node(node, true);
                self.reset_insertion_mode_appropriately();
                false
            }
            STYLE | SCRIPT | TEMPLATE => self.im_in_head(tok),
            INPUT => {
                let hidden = tok.attrs.iter().any(|a| {
                    a.name.as_deref() == Some(b"type")
                        && a.value
                            .as_deref()
                            .map(|v| v.eq_ignore_ascii_case(b"hidden"))
                            .unwrap_or(false)
                });
                if !hidden {
                    return self.in_table_anything_else(tok);
                }
                self.parse_error(tok, UNTO);
                let el = self.insert_html_element(tok);
                self.oe_pop_until_node(el, true);
                self.acknowledge_self_closing(tok);
                true
            }
            FORM => {
                self.parse_error(tok, UNTO);
                if self.form.is_some() {
                    return true;
                }
                if self.oe_find_reverse(TEMPLATE, NS_HTML).is_some() {
                    return true;
                }
                let el = self.insert_html_element(tok);
                self.form = Some(el);
                self.oe_pop_until_node(el, true);
                true
            }
            _END_OF_FILE => self.im_in_body(tok),
            _ => self.in_table_anything_else(tok),
        }
    }

    // ---------------------------------------------------------------------
    // in_table_text.c

    fn im_in_table_text(&mut self, tok: &mut Token) -> bool {
        if tok.tag_id == _TEXT {
            if tok.null_count != 0 {
                self.parse_error(tok, NUCH);
            }
            let s = self.text_without_nulls(tok);
            if s.is_empty() {
                return true;
            }
            if has_non_ws(&s) {
                self.pending_have_non_ws = true;
            }
            self.pending_table.push(s);
            return true;
        }
        let list = std::mem::take(&mut self.pending_table);
        if self.pending_have_non_ws {
            self.parse_error(tok, CHINTATE);
            self.foster_parenting = true;
            for s in list {
                self.in_body_text_append(s);
            }
            self.foster_parenting = false;
        } else {
            for s in list {
                self.insert_character_for_data(s);
            }
        }
        self.mode = self.original_mode;
        false
    }

    // ---------------------------------------------------------------------
    // in_caption.c

    fn in_caption_close(&mut self, tok: &mut Token) -> bool {
        if self
            .element_in_scope(CAPTION, NS_HTML, CAT_SCOPE_TABLE)
            .is_none()
        {
            self.parse_error(tok, MIELINSC);
            return false;
        }
        self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
        if self
            .current_node()
            .map(|c| !self.node_is(c, CAPTION))
            .unwrap_or(true)
        {
            self.parse_error(tok, UNELINOPELST);
        }
        self.oe_pop_until_tag(CAPTION, NS_HTML, true);
        self.af_up_to_last_marker();
        self.mode = Mode::InTable;
        true
    }

    fn im_in_caption(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                CAPTION => {
                    self.in_caption_close(tok);
                    true
                }
                TABLE => !self.in_caption_close(tok),
                BODY | COL | COLGROUP | HTML | TBODY | TD | TFOOT | TH | THEAD | TR => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                _ => self.im_in_body(tok),
            };
        }
        match tok.tag_id {
            CAPTION | COL | COLGROUP | TBODY | TD | TFOOT | TH | THEAD | TR => {
                !self.in_caption_close(tok)
            }
            _ => self.im_in_body(tok),
        }
    }

    // ---------------------------------------------------------------------
    // in_column_group.c

    fn in_column_group_anything_else(&mut self, tok: &mut Token) -> bool {
        if self
            .current_node()
            .map(|c| !self.node_is(c, COLGROUP))
            .unwrap_or(true)
        {
            self.parse_error(tok, MIELINOPELST);
            return true;
        }
        self.oe_pop();
        self.mode = Mode::InTable;
        false
    }

    fn im_in_column_group(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                COLGROUP => {
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, COLGROUP))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, MIELINOPELST);
                        return true;
                    }
                    self.oe_pop();
                    self.mode = Mode::InTable;
                    true
                }
                COL => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                TEMPLATE => self.im_in_head(tok),
                _ => self.in_column_group_anything_else(tok),
            };
        }
        match tok.tag_id {
            _TEXT => {
                let ws = self.tok_split_ws_begin(tok);
                if ws.text_start != ws.text_end {
                    self.insert_character(&ws);
                }
                if tok.text_start == tok.text_end {
                    return true;
                }
                self.in_column_group_anything_else(tok)
            }
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            HTML => self.im_in_body(tok),
            COL => {
                self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                true
            }
            TEMPLATE => self.im_in_head(tok),
            _END_OF_FILE => self.im_in_body(tok),
            _ => self.in_column_group_anything_else(tok),
        }
    }

    // ---------------------------------------------------------------------
    // in_table_body.c

    fn clear_stack_back_to_table_body(&mut self) {
        self.clear_stack_back_to(&[TBODY, TFOOT, THEAD, TEMPLATE, HTML]);
    }

    fn im_in_table_body(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TBODY | TFOOT | THEAD => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        self.parse_error(tok, MIELINSC);
                        return true;
                    }
                    self.clear_stack_back_to_table_body();
                    self.oe_pop();
                    self.mode = Mode::InTable;
                    true
                }
                TABLE => self.in_table_body_ct(tok),
                BODY | CAPTION | COL | COLGROUP | HTML | TD | TH | TR => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                _ => self.im_in_table(tok),
            };
        }
        match tok.tag_id {
            TR => {
                self.clear_stack_back_to_table_body();
                self.insert_html_element(tok);
                self.mode = Mode::InRow;
                true
            }
            TH | TD => {
                self.parse_error(tok, UNTO);
                self.clear_stack_back_to_table_body();
                let mut fake = tok.clone();
                fake.tag_id = TR;
                fake.attrs.clear();
                self.insert_html_element(&mut fake);
                self.mode = Mode::InRow;
                false
            }
            CAPTION | COL | COLGROUP | TBODY | TFOOT | THEAD => self.in_table_body_ct(tok),
            _ => self.im_in_table(tok),
        }
    }

    fn in_table_body_ct(&mut self, tok: &mut Token) -> bool {
        if self.element_in_scope_tbody_thead_tfoot().is_none() {
            self.parse_error(tok, MIELINSC);
            return true;
        }
        self.clear_stack_back_to_table_body();
        self.oe_pop();
        self.mode = Mode::InTable;
        false
    }

    // ---------------------------------------------------------------------
    // in_row.c

    fn clear_stack_back_to_table_row(&mut self) {
        self.clear_stack_back_to(&[TR, TEMPLATE, HTML]);
    }

    fn im_in_row(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TR => {
                    if self
                        .element_in_scope(TR, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.clear_stack_back_to_table_row();
                    self.oe_pop();
                    self.mode = Mode::InTableBody;
                    true
                }
                TABLE => self.in_row_ct(tok),
                TBODY | TFOOT | THEAD => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    if self
                        .element_in_scope(TR, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        return true;
                    }
                    self.clear_stack_back_to_table_row();
                    self.oe_pop();
                    self.mode = Mode::InTableBody;
                    false
                }
                BODY | CAPTION | COL | COLGROUP | HTML | TD | TH => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                _ => self.im_in_table(tok),
            };
        }
        match tok.tag_id {
            TH | TD => {
                self.clear_stack_back_to_table_row();
                self.insert_html_element(tok);
                self.mode = Mode::InCell;
                self.af_push_marker();
                true
            }
            CAPTION | COL | COLGROUP | TBODY | TFOOT | THEAD | TR => self.in_row_ct(tok),
            _ => self.im_in_table(tok),
        }
    }

    fn in_row_ct(&mut self, tok: &mut Token) -> bool {
        if self
            .element_in_scope(TR, NS_HTML, CAT_SCOPE_TABLE)
            .is_none()
        {
            self.parse_error(tok, UNTO);
            return true;
        }
        self.clear_stack_back_to_table_row();
        self.oe_pop();
        self.mode = Mode::InTableBody;
        false
    }

    // ---------------------------------------------------------------------
    // in_cell.c

    fn close_cell(&mut self, tok: &mut Token) {
        self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
        if let Some(c) = self.current_node() {
            if !self.node_is(c, TD) && !self.node_is(c, TH) {
                self.parse_error(tok, MIELINOPELST);
            }
        }
        self.oe_pop_until_td_th();
        self.af_up_to_last_marker();
        self.mode = Mode::InRow;
    }

    fn im_in_cell(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                TD | TH => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.generate_implied_end_tags(_UNDEF, NS_UNDEF);
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, tok.tag_id))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, MIELINOPELST);
                    }
                    self.oe_pop_until_tag(tok.tag_id, NS_HTML, true);
                    self.af_up_to_last_marker();
                    self.mode = Mode::InRow;
                    true
                }
                BODY | CAPTION | COL | COLGROUP | HTML => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
                TABLE | TBODY | TFOOT | THEAD | TR => {
                    if self
                        .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE_TABLE)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.close_cell(tok);
                    false
                }
                _ => self.im_in_body(tok),
            };
        }
        match tok.tag_id {
            CAPTION | COL | COLGROUP | TBODY | TD | TFOOT | TH | THEAD | TR => {
                if self.element_in_scope_td_th().is_none() {
                    self.parse_error(tok, MIELINSC);
                    return true;
                }
                self.close_cell(tok);
                false
            }
            _ => self.im_in_body(tok),
        }
    }

    // ---------------------------------------------------------------------
    // in_select.c

    fn im_in_select(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            return match tok.tag_id {
                OPTGROUP => {
                    if let Some(c) = self.current_node() {
                        if self.node_is(c, OPTION) && self.open_elements.len() > 1 {
                            let n = self.open_elements[self.open_elements.len() - 2];
                            if self.node_is(n, OPTGROUP) {
                                self.oe_pop();
                            }
                        }
                    }
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, OPTGROUP))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                        return true;
                    }
                    self.oe_pop();
                    true
                }
                OPTION => {
                    if self
                        .current_node()
                        .map(|c| !self.node_is(c, OPTION))
                        .unwrap_or(true)
                    {
                        self.parse_error(tok, UNELINOPELST);
                        return true;
                    }
                    self.oe_pop();
                    true
                }
                SELECT => {
                    if self
                        .element_in_scope(SELECT, NS_HTML, CAT_SCOPE_SELECT)
                        .is_none()
                    {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.oe_pop_until_tag(SELECT, NS_HTML, true);
                    self.reset_insertion_mode_appropriately();
                    true
                }
                TEMPLATE => self.im_in_head(tok),
                _ => {
                    self.parse_error(tok, UNCLTO);
                    true
                }
            };
        }
        match tok.tag_id {
            _TEXT => {
                let s = self.text_without_nulls(tok);
                if s.is_empty() {
                    return true;
                }
                self.insert_character_for_data(s);
                true
            }
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINSEMO);
                true
            }
            HTML => self.im_in_body(tok),
            OPTION => {
                if let Some(c) = self.current_node() {
                    if self.node_is(c, OPTION) {
                        self.oe_pop();
                    }
                }
                self.insert_html_element(tok);
                true
            }
            OPTGROUP | HR => {
                if let Some(c) = self.current_node() {
                    if self.node_is(c, OPTION) {
                        self.oe_pop();
                    }
                }
                if let Some(c) = self.current_node() {
                    if self.node_is(c, OPTGROUP) {
                        self.oe_pop();
                    }
                }
                self.insert_html_element(tok);
                if tok.tag_id == HR {
                    self.oe_pop();
                    self.acknowledge_self_closing(tok);
                }
                true
            }
            SELECT => {
                self.parse_error(tok, UNTO);
                if self
                    .element_in_scope(SELECT, NS_HTML, CAT_SCOPE_SELECT)
                    .is_none()
                {
                    return true;
                }
                self.oe_pop_until_tag(SELECT, NS_HTML, true);
                self.reset_insertion_mode_appropriately();
                true
            }
            INPUT | KEYGEN | TEXTAREA => {
                self.parse_error(tok, UNTO);
                if self
                    .element_in_scope(SELECT, NS_HTML, CAT_SCOPE_SELECT)
                    .is_none()
                {
                    return true;
                }
                self.oe_pop_until_tag(SELECT, NS_HTML, true);
                self.reset_insertion_mode_appropriately();
                false
            }
            SCRIPT | TEMPLATE => self.im_in_head(tok),
            _END_OF_FILE => self.im_in_body(tok),
            _ => {
                self.parse_error(tok, UNTO);
                true
            }
        }
    }

    fn im_in_select_in_table(&mut self, tok: &mut Token) -> bool {
        if tok.tag_id >= _LAST_ENTRY {
            return self.im_in_select(tok);
        }
        let ct = matches!(
            tok.tag_id,
            CAPTION | TABLE | TBODY | TFOOT | THEAD | TR | TH | TD
        );
        if Self::closed(tok) {
            if ct {
                self.parse_error(tok, UNCLTO);
                if self
                    .element_in_scope(tok.tag_id, NS_HTML, CAT_SCOPE_TABLE)
                    .is_none()
                {
                    return true;
                }
                self.oe_pop_until_tag(SELECT, NS_HTML, true);
                self.reset_insertion_mode_appropriately();
                return false;
            }
            return self.im_in_select(tok);
        }
        if ct {
            self.parse_error(tok, UNTO);
            self.oe_pop_until_tag(SELECT, NS_HTML, true);
            self.reset_insertion_mode_appropriately();
            return false;
        }
        self.im_in_select(tok)
    }

    // ---------------------------------------------------------------------
    // in_template.c

    fn in_template_switch(&mut self, m: Mode) -> bool {
        self.template_modes.pop();
        self.template_modes.push(m);
        self.mode = m;
        false
    }

    pub(crate) fn im_in_template(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            if tok.tag_id == TEMPLATE {
                return self.im_in_head(tok);
            }
            self.parse_error(tok, UNCLTO);
            return true;
        }
        match tok.tag_id {
            _TEXT | _EM_COMMENT | _EM_DOCTYPE => self.im_in_body(tok),
            BASE | BASEFONT | BGSOUND | LINK | META | NOFRAMES | SCRIPT | STYLE | TEMPLATE
            | TITLE => self.im_in_head(tok),
            CAPTION | COLGROUP | TBODY | TFOOT | THEAD => self.in_template_switch(Mode::InTable),
            COL => self.in_template_switch(Mode::InColumnGroup),
            TR => self.in_template_switch(Mode::InTableBody),
            TD | TH => self.in_template_switch(Mode::InRow),
            _END_OF_FILE => {
                if self.oe_find(TEMPLATE, NS_HTML).is_none() {
                    return true;
                }
                self.parse_error(tok, UNENOFFI);
                self.oe_pop_until_tag(TEMPLATE, NS_HTML, true);
                self.af_up_to_last_marker();
                self.template_modes.pop();
                self.reset_insertion_mode_appropriately();
                false
            }
            _ => self.in_template_switch(Mode::InBody),
        }
    }

    // ---------------------------------------------------------------------
    // after_body.c / in_frameset.c / after_frameset.c / after_after_*.c

    fn im_after_body(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _EM_COMMENT => {
                let html = self.oe_get(0);
                if let Some(h) = html {
                    self.insert_comment(tok, Some(h));
                }
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOAFBOMO);
                true
            }
            HTML => {
                if Self::closed(tok) {
                    if self.fragment.is_some() {
                        self.parse_error(tok, UNCLTO);
                        return true;
                    }
                    self.mode = Mode::AfterAfterBody;
                    return true;
                }
                self.im_in_body(tok)
            }
            _END_OF_FILE => true,
            t => {
                if t == _TEXT {
                    let mut ws = tok.clone();
                    self.tok_skip_ws_begin(&mut ws);
                    if ws.text_start == ws.text_end {
                        return self.im_in_body(tok);
                    }
                }
                self.parse_error(tok, UNTO);
                self.mode = Mode::InBody;
                false
            }
        }
    }

    fn frameset_text(&mut self, tok: &mut Token) -> bool {
        let s = self.tok_text(tok);
        let ws = stay_only_whitespace(&s);
        let same = ws.len() == s.len();
        if !ws.is_empty() {
            self.insert_character_for_data(ws);
        }
        if same {
            return true;
        }
        self.parse_error(tok, UNTO);
        true
    }

    fn im_in_frameset(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            if tok.tag_id == FRAMESET {
                let node = self.current_node();
                if node == self.oe_get(0) {
                    self.parse_error(tok, UNELINOPELST);
                    return true;
                }
                self.oe_pop();
                let node = self.current_node();
                if self.fragment.is_none()
                    && node.map(|n| !self.node_is(n, FRAMESET)).unwrap_or(true)
                {
                    self.mode = Mode::AfterFrameset;
                }
                return true;
            }
            self.parse_error(tok, UNTO);
            return true;
        }
        match tok.tag_id {
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOINFRMO);
                true
            }
            HTML => self.im_in_body(tok),
            FRAMESET => {
                self.insert_html_element(tok);
                true
            }
            FRAME => {
                self.insert_html_element(tok);
                self.oe_pop();
                self.acknowledge_self_closing(tok);
                true
            }
            NOFRAMES => self.im_in_head(tok),
            _END_OF_FILE => {
                if self.current_node() != self.oe_get(0) {
                    self.parse_error(tok, UNELINOPELST);
                }
                true
            }
            _TEXT => self.frameset_text(tok),
            _ => {
                self.parse_error(tok, UNTO);
                true
            }
        }
    }

    fn im_after_frameset(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOAFFRMO);
                true
            }
            HTML => {
                if Self::closed(tok) {
                    self.mode = Mode::AfterAfterFrameset;
                    return true;
                }
                self.im_in_body(tok)
            }
            NOFRAMES => self.im_in_head(tok),
            _END_OF_FILE => true,
            _TEXT => self.frameset_text(tok),
            _ => {
                self.parse_error(tok, UNTO);
                true
            }
        }
    }

    fn im_after_after_body(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _EM_COMMENT => {
                let d = self.document;
                self.insert_comment(tok, Some(d));
                true
            }
            _EM_DOCTYPE | HTML => self.im_in_body(tok),
            _END_OF_FILE => true,
            t => {
                if t == _TEXT {
                    let mut ws = tok.clone();
                    self.tok_skip_ws_begin(&mut ws);
                    if ws.text_start == ws.text_end {
                        return self.im_in_body(tok);
                    }
                }
                self.parse_error(tok, UNTO);
                self.mode = Mode::InBody;
                false
            }
        }
    }

    fn im_after_after_frameset(&mut self, tok: &mut Token) -> bool {
        match tok.tag_id {
            _EM_COMMENT => {
                let d = self.document;
                self.insert_comment(tok, Some(d));
                true
            }
            _EM_DOCTYPE | HTML => self.im_in_body(tok),
            _END_OF_FILE => true,
            NOFRAMES => self.im_in_head(tok),
            t => {
                if t == _TEXT {
                    let mut ws = tok.clone();
                    self.tok_skip_ws_begin(&mut ws);
                    if ws.text_start == ws.text_end {
                        return self.im_in_body(tok);
                    }
                }
                self.parse_error(tok, UNTO);
                true
            }
        }
    }

    // ---------------------------------------------------------------------
    // foreign_content.c

    fn foreign_anything_else_closed(&mut self, tok: &mut Token) -> bool {
        if self.open_elements.is_empty() {
            return self.mode_call(self.mode, tok);
        }
        let mut idx = self.open_elements.len() - 1;
        if idx > 0 && self.ntag(self.open_elements[idx]) != tok.tag_id {
            self.parse_error(tok, UNELINOPELST);
        }
        while idx != 0 {
            let n = self.open_elements[idx];
            if self.ntag(n) == tok.tag_id {
                self.oe_pop_until_node(n, true);
                return true;
            }
            idx -= 1;
            if self.nns(self.open_elements[idx]) == NS_HTML {
                break;
            }
        }
        self.mode_call(self.mode, tok)
    }

    fn foreign_script_closed(&mut self, tok: &mut Token) -> bool {
        match self.current_node() {
            Some(n) if self.ntag(n) == SCRIPT && self.nns(n) == NS_SVG => {
                self.oe_pop();
                true
            }
            _ => self.foreign_anything_else_closed(tok),
        }
    }

    fn foreign_anything_else(&mut self, tok: &mut Token) -> bool {
        let node = match self.adjusted_current_node() {
            Some(n) => n,
            None => return true,
        };
        let ns = self.nns(node);
        if ns == NS_MATH {
            self.set_attr_adjust_mathml();
        } else if ns == NS_SVG {
            self.set_attr_adjust_svg();
        }
        let el = self.insert_foreign_element(tok, ns);
        if ns == NS_SVG {
            let tag = self.ntag(el);
            if (tag as usize) < SVG_FIXNAME.len() {
                if let Some(fixed) = SVG_FIXNAME[tag as usize] {
                    self.dom.nodes[el].qualified = Some(fixed.as_bytes().to_vec());
                }
            }
        }
        self.clear_attr_adjust();
        if tok.typ & TOKEN_CLOSE_SELF == 0 {
            return true;
        }
        let cur = self.current_node();
        if tok.tag_id == SCRIPT && cur.map(|c| self.nns(c) == NS_SVG).unwrap_or(false) {
            return self.foreign_script_closed(tok);
        }
        self.oe_pop();
        true
    }

    fn foreign_text(&mut self, tok: &mut Token) -> bool {
        let mut s = self.tok_text(tok);
        if tok.null_count != 0 {
            self.parse_error(tok, NUCH);
            let mut out = Vec::with_capacity(s.len());
            for &c in &s {
                if c == 0 {
                    out.extend_from_slice("\u{FFFD}".as_bytes());
                } else {
                    out.push(c);
                }
            }
            s = out;
        }
        if s.is_empty() {
            return true;
        }
        if self.frameset_ok {
            let rep = "\u{FFFD}".as_bytes();
            let mut i = 0;
            while i < s.len() {
                if s[i] == rep[0] {
                    if s.len() - i < rep.len() || &s[i..i + rep.len()] != rep {
                        self.frameset_ok = false;
                        break;
                    }
                    i += rep.len();
                    continue;
                }
                if !matches!(s[i], 0x09 | 0x0A | 0x0C | 0x0D | 0x20) {
                    self.frameset_ok = false;
                    break;
                }
                i += 1;
            }
        }
        self.insert_character_for_data(s);
        true
    }

    fn foreign_all(&mut self, tok: &mut Token) -> bool {
        if tok.tag_id == FONT {
            let has = tok.attrs.iter().any(|a| {
                matches!(
                    a.name.as_deref(),
                    Some(b"color") | Some(b"face") | Some(b"size")
                )
            });
            if !has {
                return self.foreign_anything_else(tok);
            }
        }
        self.parse_error(tok, UNTO);
        if self.fragment.is_some() {
            return self.foreign_anything_else(tok);
        }
        loop {
            self.oe_pop();
            match self.current_node() {
                None => break,
                Some(n) => {
                    if self.mathml_text_integration_point(n)
                        || self.html_integration_point(n)
                        || self.nns(n) == NS_HTML
                    {
                        break;
                    }
                }
            }
        }
        false
    }

    pub(crate) fn foreign_content(&mut self, tok: &mut Token) -> bool {
        if Self::closed(tok) {
            if tok.tag_id == SCRIPT {
                return self.foreign_script_closed(tok);
            }
            return self.foreign_anything_else_closed(tok);
        }
        match tok.tag_id {
            _TEXT => self.foreign_text(tok),
            _EM_COMMENT => {
                self.insert_comment(tok, None);
                true
            }
            _EM_DOCTYPE => {
                self.parse_error(tok, DOTOFOCOMO);
                true
            }
            B | BIG | BLOCKQUOTE | BODY | BR | CENTER | CODE | DD | DIV | DL | DT | EM | EMBED
            | H1 | H2 | H3 | H4 | H5 | H6 | HEAD | HR | I | IMG | LI | LISTING | MENU | META
            | NOBR | OL | P | PRE | RUBY | S | SMALL | SPAN | STRONG | STRIKE | SUB | TABLE
            | TT | U | UL | VAR | FONT => self.foreign_all(tok),
            _ => self.foreign_anything_else(tok),
        }
    }
}

#[allow(dead_code)]
fn _kind_check(k: LKind) -> bool {
    k == LKind::Text
}
