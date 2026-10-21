//! Faithful port of the token boundaries produced by php-src's re2c scanner
//! (`Zend/zend_language_scanner.l`) when it runs outside parser mode, i.e. as
//! used by `zend_highlight()`, `zend_strip()` and the tokenizer.
//!
//! goro's main [`crate::Lexer`] is tuned for the parser (it folds strings and
//! heredocs into single tokens); this scanner instead reproduces PHP's raw
//! token stream exactly: inline HTML, open/close tags, whitespace, comments,
//! the `"`/`T_ENCAPSED_AND_WHITESPACE`/`T_VARIABLE` pieces of interpolated
//! strings, heredoc start/body/end tokens, and so on. Concatenating the text
//! of all tokens yields the input (except for a skipped shebang line).
//!
//! Only token kinds and byte ranges are produced; semantic values are not
//! computed (the highlighter only needs to know which kinds carry one, see
//! [`token_has_value`]).

pub const T_LNUMBER: i32 = 260;
pub const T_DNUMBER: i32 = 261;
pub const T_STRING: i32 = 262;
pub const T_NAME_FULLY_QUALIFIED: i32 = 263;
pub const T_NAME_RELATIVE: i32 = 264;
pub const T_NAME_QUALIFIED: i32 = 265;
pub const T_VARIABLE: i32 = 266;
pub const T_INLINE_HTML: i32 = 267;
pub const T_ENCAPSED_AND_WHITESPACE: i32 = 268;
pub const T_CONSTANT_ENCAPSED_STRING: i32 = 269;
pub const T_STRING_VARNAME: i32 = 270;
pub const T_NUM_STRING: i32 = 271;
pub const T_INCLUDE: i32 = 272;
pub const T_INCLUDE_ONCE: i32 = 273;
pub const T_EVAL: i32 = 274;
pub const T_REQUIRE: i32 = 275;
pub const T_REQUIRE_ONCE: i32 = 276;
pub const T_LOGICAL_OR: i32 = 277;
pub const T_LOGICAL_XOR: i32 = 278;
pub const T_LOGICAL_AND: i32 = 279;
pub const T_PRINT: i32 = 280;
pub const T_YIELD: i32 = 281;
pub const T_YIELD_FROM: i32 = 282;
pub const T_INSTANCEOF: i32 = 283;
pub const T_NEW: i32 = 284;
pub const T_CLONE: i32 = 285;
pub const T_EXIT: i32 = 286;
pub const T_IF: i32 = 287;
pub const T_ELSEIF: i32 = 288;
pub const T_ELSE: i32 = 289;
pub const T_ENDIF: i32 = 290;
pub const T_ECHO: i32 = 291;
pub const T_DO: i32 = 292;
pub const T_WHILE: i32 = 293;
pub const T_ENDWHILE: i32 = 294;
pub const T_FOR: i32 = 295;
pub const T_ENDFOR: i32 = 296;
pub const T_FOREACH: i32 = 297;
pub const T_ENDFOREACH: i32 = 298;
pub const T_DECLARE: i32 = 299;
pub const T_ENDDECLARE: i32 = 300;
pub const T_AS: i32 = 301;
pub const T_SWITCH: i32 = 302;
pub const T_ENDSWITCH: i32 = 303;
pub const T_CASE: i32 = 304;
pub const T_DEFAULT: i32 = 305;
pub const T_MATCH: i32 = 306;
pub const T_BREAK: i32 = 307;
pub const T_CONTINUE: i32 = 308;
pub const T_GOTO: i32 = 309;
pub const T_FUNCTION: i32 = 310;
pub const T_FN: i32 = 311;
pub const T_CONST: i32 = 312;
pub const T_RETURN: i32 = 313;
pub const T_TRY: i32 = 314;
pub const T_CATCH: i32 = 315;
pub const T_FINALLY: i32 = 316;
pub const T_THROW: i32 = 317;
pub const T_USE: i32 = 318;
pub const T_INSTEADOF: i32 = 319;
pub const T_GLOBAL: i32 = 320;
pub const T_STATIC: i32 = 321;
pub const T_ABSTRACT: i32 = 322;
pub const T_FINAL: i32 = 323;
pub const T_PRIVATE: i32 = 324;
pub const T_PROTECTED: i32 = 325;
pub const T_PUBLIC: i32 = 326;
pub const T_READONLY: i32 = 327;
pub const T_VAR: i32 = 328;
pub const T_UNSET: i32 = 329;
pub const T_ISSET: i32 = 330;
pub const T_EMPTY: i32 = 331;
pub const T_HALT_COMPILER: i32 = 332;
pub const T_CLASS: i32 = 333;
pub const T_TRAIT: i32 = 334;
pub const T_INTERFACE: i32 = 335;
pub const T_ENUM: i32 = 336;
pub const T_EXTENDS: i32 = 337;
pub const T_IMPLEMENTS: i32 = 338;
pub const T_NAMESPACE: i32 = 339;
pub const T_LIST: i32 = 340;
pub const T_ARRAY: i32 = 341;
pub const T_CALLABLE: i32 = 342;
pub const T_LINE: i32 = 343;
pub const T_FILE: i32 = 344;
pub const T_DIR: i32 = 345;
pub const T_CLASS_C: i32 = 346;
pub const T_TRAIT_C: i32 = 347;
pub const T_METHOD_C: i32 = 348;
pub const T_FUNC_C: i32 = 349;
pub const T_NS_C: i32 = 350;
pub const T_ATTRIBUTE: i32 = 351;
pub const T_PLUS_EQUAL: i32 = 352;
pub const T_MINUS_EQUAL: i32 = 353;
pub const T_MUL_EQUAL: i32 = 354;
pub const T_DIV_EQUAL: i32 = 355;
pub const T_CONCAT_EQUAL: i32 = 356;
pub const T_MOD_EQUAL: i32 = 357;
pub const T_AND_EQUAL: i32 = 358;
pub const T_OR_EQUAL: i32 = 359;
pub const T_XOR_EQUAL: i32 = 360;
pub const T_SL_EQUAL: i32 = 361;
pub const T_SR_EQUAL: i32 = 362;
pub const T_COALESCE_EQUAL: i32 = 363;
pub const T_BOOLEAN_OR: i32 = 364;
pub const T_BOOLEAN_AND: i32 = 365;
pub const T_IS_EQUAL: i32 = 366;
pub const T_IS_NOT_EQUAL: i32 = 367;
pub const T_IS_IDENTICAL: i32 = 368;
pub const T_IS_NOT_IDENTICAL: i32 = 369;
pub const T_IS_SMALLER_OR_EQUAL: i32 = 370;
pub const T_IS_GREATER_OR_EQUAL: i32 = 371;
pub const T_SPACESHIP: i32 = 372;
pub const T_SL: i32 = 373;
pub const T_SR: i32 = 374;
pub const T_INC: i32 = 375;
pub const T_DEC: i32 = 376;
pub const T_INT_CAST: i32 = 377;
pub const T_DOUBLE_CAST: i32 = 378;
pub const T_STRING_CAST: i32 = 379;
pub const T_ARRAY_CAST: i32 = 380;
pub const T_OBJECT_CAST: i32 = 381;
pub const T_BOOL_CAST: i32 = 382;
pub const T_UNSET_CAST: i32 = 383;
pub const T_OBJECT_OPERATOR: i32 = 384;
pub const T_NULLSAFE_OBJECT_OPERATOR: i32 = 385;
pub const T_DOUBLE_ARROW: i32 = 386;
pub const T_COMMENT: i32 = 387;
pub const T_DOC_COMMENT: i32 = 388;
pub const T_OPEN_TAG: i32 = 389;
pub const T_OPEN_TAG_WITH_ECHO: i32 = 390;
pub const T_CLOSE_TAG: i32 = 391;
pub const T_WHITESPACE: i32 = 392;
pub const T_START_HEREDOC: i32 = 393;
pub const T_END_HEREDOC: i32 = 394;
pub const T_DOLLAR_OPEN_CURLY_BRACES: i32 = 395;
pub const T_CURLY_OPEN: i32 = 396;
pub const T_PAAMAYIM_NEKUDOTAYIM: i32 = 397;
pub const T_NS_SEPARATOR: i32 = 398;
pub const T_ELLIPSIS: i32 = 399;
pub const T_COALESCE: i32 = 400;
pub const T_POW: i32 = 401;
pub const T_POW_EQUAL: i32 = 402;
pub const T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG: i32 = 403;
pub const T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG: i32 = 404;
pub const T_BAD_CHARACTER: i32 = 405;
pub const T_PRIVATE_SET: i32 = 406;
pub const T_PROTECTED_SET: i32 = 407;
pub const T_PUBLIC_SET: i32 = 408;
pub const T_PROPERTY_C: i32 = 409;
pub const T_VOID_CAST: i32 = 410;
pub const T_PIPE: i32 = 411;

/// One raw scanner token. `id` is either a single-character token (its byte
/// value, < 256) or one of the `T_*` ids above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanToken {
    pub id: i32,
    pub start: usize,
    pub end: usize,
    /// Line of the first byte of the token (1-based).
    pub line: u32,
}

/// Whether the scanner hands the token's text to the caller as a semantic
/// value (`zendlval` is set). zend_highlight() colors such tokens with
/// `highlight.default` and value-less ones (keywords, operators) with
/// `highlight.keyword`.
pub fn token_has_value(id: i32) -> bool {
    matches!(
        id,
        T_LNUMBER
            | T_DNUMBER
            | T_STRING
            | T_NAME_FULLY_QUALIFIED
            | T_NAME_RELATIVE
            | T_NAME_QUALIFIED
            | T_VARIABLE
            | T_INLINE_HTML
            | T_ENCAPSED_AND_WHITESPACE
            | T_CONSTANT_ENCAPSED_STRING
            | T_STRING_VARNAME
            | T_NUM_STRING
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    Initial,
    InScripting,
    LookingForProperty,
    DoubleQuotes,
    Backquote,
    Heredoc,
    Nowdoc,
    EndHeredoc,
    LookingForVarname,
    VarOffset,
}

struct HeredocLabel {
    label: Vec<u8>,
    indentation: usize,
}

#[inline]
fn is_label_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

#[inline]
fn is_label_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t')
}

pub struct Scanner<'a> {
    s: &'a [u8],
    pos: usize,
    state: St,
    stack: Vec<St>,
    heredocs: Vec<HeredocLabel>,
    dq_scanned: usize,
    short_tags: bool,
    line: u32,
    line_pos: usize,
}

impl<'a> Scanner<'a> {
    /// Create a scanner starting in the INITIAL (inline HTML) state.
    /// `skip_shebang` mimics CG(skip_shebang) for files opened by the CLI.
    pub fn new(s: &'a [u8], short_tags: bool, skip_shebang: bool) -> Self {
        let mut sc = Scanner {
            s,
            pos: 0,
            state: St::Initial,
            stack: Vec::new(),
            heredocs: Vec::new(),
            dq_scanned: 0,
            short_tags,
            line: 1,
            line_pos: 0,
        };
        if skip_shebang && s.starts_with(b"#!") {
            // <SHEBANG>"#!" .* {NEWLINE}
            if let Some(nl) = s.iter().position(|&c| c == b'\n') {
                sc.pos = nl + 1;
            } else if let Some(cr) = s.iter().rposition(|&c| c == b'\r') {
                sc.pos = cr + 1;
            }
        }
        sc
    }

    #[inline]
    fn at(&self, i: usize) -> u8 {
        self.s.get(i).copied().unwrap_or(0)
    }

    fn starts_ci(&self, i: usize, lit: &[u8]) -> bool {
        lit.iter().enumerate().all(|(k, &c)| self.at(i + k).eq_ignore_ascii_case(&c))
    }

    fn starts(&self, i: usize, lit: &[u8]) -> bool {
        self.s.get(i..i + lit.len()) == Some(lit)
    }

    fn label_end(&self, mut i: usize) -> usize {
        while i < self.s.len() && is_label_char(self.s[i]) {
            i += 1;
        }
        i
    }

    fn push_state(&mut self, new: St) {
        self.stack.push(self.state);
        self.state = new;
    }

    fn pop_state(&mut self) {
        self.state = self.stack.pop().unwrap_or(St::InScripting);
    }

    fn tok(&mut self, id: i32, start: usize, end: usize) -> Option<ScanToken> {
        // Advance the line counter to `start`.
        let mut line = self.line;
        for &c in &self.s[self.line_pos.min(start)..start] {
            if c == b'\n' {
                line += 1;
            }
        }
        // Count bare "\r" line endings too.
        if self.line_pos < start {
            let seg = &self.s[self.line_pos..start];
            for (k, &c) in seg.iter().enumerate() {
                if c == b'\r' && seg.get(k + 1).copied().unwrap_or_else(|| self.at(start)) != b'\n' {
                    line += 1;
                }
            }
        }
        self.line = line;
        self.line_pos = start;
        self.pos = end;
        Some(ScanToken { id, start, end, line })
    }

    /// {WHITESPACE_OR_COMMENTS}* starting at `i`; returns the end position.
    fn skip_ws_comments(&self, mut j: usize) -> usize {
        let n = self.s.len();
        loop {
            let c = self.at(j);
            if j < n && is_ws(c) {
                j += 1;
                continue;
            }
            if self.starts(j, b"/*") {
                let mut k = j + 2;
                let mut found = None;
                while k + 1 < n {
                    if self.s[k] == 0 {
                        break;
                    }
                    if self.s[k] == b'*' && self.s[k + 1] == b'/' {
                        found = Some(k + 2);
                        break;
                    }
                    k += 1;
                }
                match found {
                    Some(e) => {
                        j = e;
                        continue;
                    }
                    None => break,
                }
            }
            if self.starts(j, b"//") || (c == b'#' && j < n && self.at(j + 1) != b'[') {
                let mut k = if c == b'#' { j + 1 } else { j + 2 };
                while k < n && !matches!(self.s[k], 0 | b'\n' | b'\r') {
                    k += 1;
                }
                if k < n && matches!(self.s[k], b'\n' | b'\r') {
                    j = k + 1;
                    continue;
                }
                break;
            }
            break;
        }
        j
    }

    fn digits_run(&self, i: usize, pred: fn(u8) -> bool) -> Option<usize> {
        if !pred(self.at(i)) {
            return None;
        }
        let mut j = i + 1;
        loop {
            while pred(self.at(j)) {
                j += 1;
            }
            if self.at(j) == b'_' && pred(self.at(j + 1)) {
                j += 1;
                continue;
            }
            break;
        }
        Some(j)
    }

    fn lnum(&self, i: usize) -> Option<usize> {
        self.digits_run(i, |c| c.is_ascii_digit())
    }

    fn prefixed_num(&self, i: usize, letter: u8, pred: fn(u8) -> bool) -> Option<usize> {
        if self.at(i) == b'0' && self.at(i + 1).to_ascii_lowercase() == letter {
            self.digits_run(i + 2, pred)
        } else {
            None
        }
    }

    /// Possible ends of a DNUM match starting at `i`.
    fn dnum_ends(&self, i: usize) -> Vec<usize> {
        let mut v = Vec::new();
        // {LNUM}?"."{LNUM}
        let a = self.lnum(i).unwrap_or(i);
        if self.at(a) == b'.' {
            if let Some(e) = self.lnum(a + 1) {
                v.push(e);
            }
        }
        // {LNUM}"."{LNUM}?
        if let Some(a) = self.lnum(i) {
            if self.at(a) == b'.' {
                v.push(a + 1);
                if let Some(e) = self.lnum(a + 1) {
                    v.push(e);
                }
            }
        }
        v
    }

    /// Longest numeric literal at `i` in ST_IN_SCRIPTING: (end, token id).
    fn scan_number(&self, i: usize) -> Option<(usize, i32)> {
        let mut best: Option<(usize, u8)> = None; // kind: b'b', b'o', b'l', b'h', b'd'
        let consider = |end: Option<usize>, kind: u8, best: &mut Option<(usize, u8)>| {
            if let Some(e) = end {
                if best.is_none_or(|(be, _)| e > be) {
                    *best = Some((e, kind));
                }
            }
        };
        consider(self.prefixed_num(i, b'b', |c| c == b'0' || c == b'1'), b'b', &mut best);
        consider(self.prefixed_num(i, b'o', |c| (b'0'..=b'7').contains(&c)), b'o', &mut best);
        consider(self.lnum(i), b'l', &mut best);
        consider(self.prefixed_num(i, b'x', |c| c.is_ascii_hexdigit()), b'h', &mut best);
        let dn = self.dnum_ends(i);
        for &e in &dn {
            consider(Some(e), b'd', &mut best);
        }
        // EXPONENT_DNUM: ({LNUM}|{DNUM})[eE][+-]?{LNUM}
        let mut bases: Vec<usize> = dn;
        if let Some(e) = self.lnum(i) {
            bases.push(e);
        }
        for b in bases {
            if matches!(self.at(b), b'e' | b'E') {
                let mut k = b + 1;
                if matches!(self.at(k), b'+' | b'-') {
                    k += 1;
                }
                consider(self.lnum(k), b'd', &mut best);
            }
        }
        let (end, kind) = best?;
        let text: Vec<u8> = self.s[i..end].iter().copied().filter(|&c| c != b'_').collect();
        let overflow = |digits: &[u8], radix: u32| -> bool {
            let mut v: u128 = 0;
            for &c in digits {
                let d = (c as char).to_digit(radix).unwrap_or(0) as u128;
                v = v.saturating_mul(radix as u128).saturating_add(d);
            }
            v > i64::MAX as u128
        };
        let id = match kind {
            b'd' => T_DNUMBER,
            b'h' => if overflow(&text[2..], 16) { T_DNUMBER } else { T_LNUMBER },
            b'b' => if overflow(&text[2..], 2) { T_DNUMBER } else { T_LNUMBER },
            b'o' => if overflow(&text[2..], 8) { T_DNUMBER } else { T_LNUMBER },
            _ => {
                if text[0] == b'0' {
                    let cut = text.iter().position(|&c| c == b'8' || c == b'9').unwrap_or(text.len());
                    if overflow(&text[..cut], 8) { T_DNUMBER } else { T_LNUMBER }
                } else if overflow(&text, 10) {
                    T_DNUMBER
                } else {
                    T_LNUMBER
                }
            }
        };
        Some((end, id))
    }

    /// "#" / "//" comment body starting after the introducer.
    fn line_comment(&mut self, p: usize, intro: usize) -> Option<ScanToken> {
        let n = self.s.len();
        let mut cur = p + intro;
        while cur < n {
            let ch = self.s[cur];
            cur += 1;
            match ch {
                b'\r' | b'\n' => {
                    cur -= 1;
                    break;
                }
                b'?' if self.at(cur) == b'>' => {
                    cur -= 1;
                    break;
                }
                _ => {}
            }
        }
        self.tok(T_COMMENT, p, cur)
    }

    fn block_comment(&mut self, p: usize) -> Option<ScanToken> {
        let n = self.s.len();
        let mut yyleng = 2;
        let doc = self.starts(p, b"/**") && p + 3 < n && is_ws(self.s[p + 3]);
        if doc {
            yyleng = 3;
            while p + yyleng < n && is_ws(self.s[p + yyleng]) {
                yyleng += 1;
            }
        }
        let mut cur = p + yyleng;
        while cur < n {
            let ch = self.s[cur];
            cur += 1;
            if ch == b'*' && self.at(cur) == b'/' {
                break;
            }
        }
        if cur < n {
            cur += 1;
        }
        self.tok(if doc { T_DOC_COMMENT } else { T_COMMENT }, p, cur)
    }

    fn single_quoted(&mut self, p: usize, bprefix: usize) -> Option<ScanToken> {
        let n = self.s.len();
        let mut cur = p + bprefix + 1;
        loop {
            if cur < n {
                if self.s[cur] == b'\'' {
                    return self.tok(T_CONSTANT_ENCAPSED_STRING, p, cur + 1);
                }
                let ch = self.s[cur];
                cur += 1;
                if ch == b'\\' && cur < n {
                    cur += 1;
                }
            } else {
                return self.tok(T_ENCAPSED_AND_WHITESPACE, p, n);
            }
        }
    }

    fn double_quoted(&mut self, p: usize, bprefix: usize) -> Option<ScanToken> {
        let n = self.s.len();
        let yyleng = bprefix + 1;
        let mut cur = p + yyleng;
        while cur < n {
            let ch = self.s[cur];
            cur += 1;
            match ch {
                b'"' => return self.tok(T_CONSTANT_ENCAPSED_STRING, p, cur),
                b'$' => {
                    if is_label_start(self.at(cur)) || self.at(cur) == b'{' {
                        cur -= 1;
                        break;
                    }
                }
                b'{' => {
                    if self.at(cur) == b'$' {
                        cur -= 1;
                        break;
                    }
                }
                b'\\' => {
                    if cur < n {
                        cur += 1;
                    }
                }
                _ => {}
            }
        }
        self.dq_scanned = cur - p - yyleng;
        self.state = St::DoubleQuotes;
        self.tok(b'"' as i32, p, p + yyleng)
    }

    /// b?"<<<"{TABS_AND_SPACES}({LABEL}|'{LABEL}'|"{LABEL}"){NEWLINE} at `p`
    /// (`q` = position of "<<<").
    fn try_heredoc(&mut self, p: usize, q: usize) -> Option<Option<ScanToken>> {
        let n = self.s.len();
        if !self.starts(q, b"<<<") {
            return None;
        }
        let mut j = q + 3;
        while matches!(self.at(j), b' ' | b'\t') && j < n {
            j += 1;
        }
        let quote = self.at(j);
        let (lstart, lend, after) = if quote == b'\'' || quote == b'"' {
            if !is_label_start(self.at(j + 1)) || j + 1 >= n {
                return None;
            }
            let e = self.label_end(j + 1);
            if self.at(e) != quote || e >= n {
                return None;
            }
            (j + 1, e, e + 1)
        } else {
            if !is_label_start(quote) || j >= n {
                return None;
            }
            let e = self.label_end(j);
            (j, e, e)
        };
        let end = match self.at(after) {
            b'\n' if after < n => after + 1,
            b'\r' if after < n => {
                if self.at(after + 1) == b'\n' && after + 1 < n {
                    after + 2
                } else {
                    after + 1
                }
            }
            _ => return None,
        };
        let label = self.s[lstart..lend].to_vec();
        self.state = if quote == b'\'' { St::Nowdoc } else { St::Heredoc };
        let len = label.len();
        self.heredocs.push(HeredocLabel { label, indentation: 0 });
        let mut cur = end;
        let mut indentation = 0;
        while cur < n && matches!(self.s[cur], b' ' | b'\t') {
            cur += 1;
            indentation += 1;
        }
        if cur == n {
            return Some(self.tok(T_START_HEREDOC, p, end));
        }
        if len < n - cur && self.s[cur..cur + len] == self.s[lstart..lend] && !is_label_char(self.at(cur + len)) {
            if let Some(h) = self.heredocs.last_mut() {
                h.indentation = indentation;
            }
            self.state = St::EndHeredoc;
        }
        let _ = p;
        Some(self.tok(T_START_HEREDOC, p, end))
    }

    fn scan_scripting(&mut self) -> Option<ScanToken> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            return None;
        }
        let c = self.s[p];
        let c1 = self.at(p + 1);
        let c2 = self.at(p + 2);
        match c {
            b' ' | b'\n' | b'\r' | b'\t' => {
                let mut e = p + 1;
                while e < n && is_ws(self.s[e]) {
                    e += 1;
                }
                self.tok(T_WHITESPACE, p, e)
            }
            b'$' => {
                if is_label_start(c1) && p + 1 < n {
                    let e = self.label_end(p + 1);
                    self.tok(T_VARIABLE, p, e)
                } else {
                    self.tok(b'$' as i32, p, p + 1)
                }
            }
            b'0'..=b'9' => {
                let (e, id) = self.scan_number(p).unwrap();
                self.tok(id, p, e)
            }
            b'.' => {
                if c1.is_ascii_digit() {
                    let (e, id) = self.scan_number(p).unwrap();
                    self.tok(id, p, e)
                } else if c1 == b'.' && c2 == b'.' {
                    self.tok(T_ELLIPSIS, p, p + 3)
                } else if c1 == b'=' {
                    self.tok(T_CONCAT_EQUAL, p, p + 2)
                } else {
                    self.tok(b'.' as i32, p, p + 1)
                }
            }
            b'\\' => {
                if is_label_start(c1) && p + 1 < n {
                    let mut q = self.label_end(p + 1);
                    while self.at(q) == b'\\' && is_label_start(self.at(q + 1)) && q + 1 < n {
                        q = self.label_end(q + 1);
                    }
                    self.tok(T_NAME_FULLY_QUALIFIED, p, q)
                } else {
                    self.tok(T_NS_SEPARATOR, p, p + 1)
                }
            }
            b'#' => {
                if c1 == b'[' {
                    self.tok(T_ATTRIBUTE, p, p + 2)
                } else {
                    self.line_comment(p, 1)
                }
            }
            b'/' => {
                if c1 == b'/' {
                    self.line_comment(p, 2)
                } else if c1 == b'*' {
                    self.block_comment(p)
                } else if c1 == b'=' {
                    self.tok(T_DIV_EQUAL, p, p + 2)
                } else {
                    self.tok(b'/' as i32, p, p + 1)
                }
            }
            b'?' => {
                if c1 == b'>' {
                    let mut e = p + 2;
                    if self.at(e) == b'\n' && e < n {
                        e += 1;
                    } else if self.at(e) == b'\r' && e < n {
                        e += 1;
                        if self.at(e) == b'\n' && e < n {
                            e += 1;
                        }
                    }
                    self.state = St::Initial;
                    self.tok(T_CLOSE_TAG, p, e)
                } else if c1 == b'-' && c2 == b'>' {
                    self.push_state(St::LookingForProperty);
                    self.tok(T_NULLSAFE_OBJECT_OPERATOR, p, p + 3)
                } else if c1 == b'?' && c2 == b'=' {
                    self.tok(T_COALESCE_EQUAL, p, p + 3)
                } else if c1 == b'?' {
                    self.tok(T_COALESCE, p, p + 2)
                } else {
                    self.tok(b'?' as i32, p, p + 1)
                }
            }
            b'(' => {
                let mut j = p + 1;
                while matches!(self.at(j), b' ' | b'\t') {
                    j += 1;
                }
                let mut k = j;
                while self.at(k).is_ascii_alphabetic() {
                    k += 1;
                }
                let word = self.s[j.min(n)..k.min(n)].to_ascii_lowercase();
                let cast = match word.as_slice() {
                    b"int" | b"integer" => Some(T_INT_CAST),
                    b"float" | b"double" | b"real" => Some(T_DOUBLE_CAST),
                    b"string" | b"binary" => Some(T_STRING_CAST),
                    b"array" => Some(T_ARRAY_CAST),
                    b"object" => Some(T_OBJECT_CAST),
                    b"bool" | b"boolean" => Some(T_BOOL_CAST),
                    b"unset" => Some(T_UNSET_CAST),
                    b"void" => Some(T_VOID_CAST),
                    _ => None,
                };
                if let Some(id) = cast {
                    let mut m = k;
                    while matches!(self.at(m), b' ' | b'\t') {
                        m += 1;
                    }
                    if self.at(m) == b')' && m < n {
                        return self.tok(id, p, m + 1);
                    }
                }
                self.tok(b'(' as i32, p, p + 1)
            }
            b'\'' => self.single_quoted(p, 0),
            b'"' => self.double_quoted(p, 0),
            b'`' => {
                self.state = St::Backquote;
                self.tok(b'`' as i32, p, p + 1)
            }
            b'{' => {
                self.push_state(St::InScripting);
                self.tok(b'{' as i32, p, p + 1)
            }
            b'}' => {
                if !self.stack.is_empty() {
                    self.pop_state();
                }
                self.tok(b'}' as i32, p, p + 1)
            }
            b'<' => {
                if let Some(t) = self.try_heredoc(p, p) {
                    return t;
                }
                if c1 == b'<' && c2 == b'=' {
                    self.tok(T_SL_EQUAL, p, p + 3)
                } else if c1 == b'<' {
                    self.tok(T_SL, p, p + 2)
                } else if c1 == b'=' && c2 == b'>' {
                    self.tok(T_SPACESHIP, p, p + 3)
                } else if c1 == b'=' {
                    self.tok(T_IS_SMALLER_OR_EQUAL, p, p + 2)
                } else if c1 == b'>' {
                    self.tok(T_IS_NOT_EQUAL, p, p + 2)
                } else {
                    self.tok(b'<' as i32, p, p + 1)
                }
            }
            b'>' => {
                if c1 == b'>' && c2 == b'=' {
                    self.tok(T_SR_EQUAL, p, p + 3)
                } else if c1 == b'>' {
                    self.tok(T_SR, p, p + 2)
                } else if c1 == b'=' {
                    self.tok(T_IS_GREATER_OR_EQUAL, p, p + 2)
                } else {
                    self.tok(b'>' as i32, p, p + 1)
                }
            }
            b'=' => {
                if c1 == b'=' && c2 == b'=' {
                    self.tok(T_IS_IDENTICAL, p, p + 3)
                } else if c1 == b'=' {
                    self.tok(T_IS_EQUAL, p, p + 2)
                } else if c1 == b'>' {
                    self.tok(T_DOUBLE_ARROW, p, p + 2)
                } else {
                    self.tok(b'=' as i32, p, p + 1)
                }
            }
            b'!' => {
                if c1 == b'=' && c2 == b'=' {
                    self.tok(T_IS_NOT_IDENTICAL, p, p + 3)
                } else if c1 == b'=' {
                    self.tok(T_IS_NOT_EQUAL, p, p + 2)
                } else {
                    self.tok(b'!' as i32, p, p + 1)
                }
            }
            b'+' => match c1 {
                b'+' => self.tok(T_INC, p, p + 2),
                b'=' => self.tok(T_PLUS_EQUAL, p, p + 2),
                _ => self.tok(b'+' as i32, p, p + 1),
            },
            b'-' => match c1 {
                b'-' => self.tok(T_DEC, p, p + 2),
                b'=' => self.tok(T_MINUS_EQUAL, p, p + 2),
                b'>' => {
                    self.push_state(St::LookingForProperty);
                    self.tok(T_OBJECT_OPERATOR, p, p + 2)
                }
                _ => self.tok(b'-' as i32, p, p + 1),
            },
            b'*' => {
                if c1 == b'*' && c2 == b'=' {
                    self.tok(T_POW_EQUAL, p, p + 3)
                } else if c1 == b'*' {
                    self.tok(T_POW, p, p + 2)
                } else if c1 == b'=' {
                    self.tok(T_MUL_EQUAL, p, p + 2)
                } else {
                    self.tok(b'*' as i32, p, p + 1)
                }
            }
            b'%' => {
                if c1 == b'=' {
                    self.tok(T_MOD_EQUAL, p, p + 2)
                } else {
                    self.tok(b'%' as i32, p, p + 1)
                }
            }
            b'^' => {
                if c1 == b'=' {
                    self.tok(T_XOR_EQUAL, p, p + 2)
                } else {
                    self.tok(b'^' as i32, p, p + 1)
                }
            }
            b'|' => match c1 {
                b'|' => self.tok(T_BOOLEAN_OR, p, p + 2),
                b'=' => self.tok(T_OR_EQUAL, p, p + 2),
                b'>' => self.tok(T_PIPE, p, p + 2),
                _ => self.tok(b'|' as i32, p, p + 1),
            },
            b'&' => {
                if c1 == b'&' {
                    self.tok(T_BOOLEAN_AND, p, p + 2)
                } else if c1 == b'=' {
                    self.tok(T_AND_EQUAL, p, p + 2)
                } else {
                    let w = self.skip_ws_comments(p + 1);
                    if (self.at(w) == b'$' && w < n) || self.starts(w, b"...") {
                        self.tok(T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG, p, p + 1)
                    } else {
                        self.tok(T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG, p, p + 1)
                    }
                }
            }
            b':' => {
                if c1 == b':' {
                    self.tok(T_PAAMAYIM_NEKUDOTAYIM, p, p + 2)
                } else {
                    self.tok(b':' as i32, p, p + 1)
                }
            }
            b';' | b',' | b'~' | b'@' | b'[' | b']' | b')' => self.tok(c as i32, p, p + 1),
            _ if is_label_start(c) => self.scan_label(p),
            _ => self.tok(T_BAD_CHARACTER, p, p + 1),
        }
    }

    fn scan_label(&mut self, p: usize) -> Option<ScanToken> {
        let n = self.s.len();
        let c = self.s[p];
        if c == b'b' || c == b'B' {
            match self.at(p + 1) {
                b'\'' if p + 1 < n => return self.single_quoted(p, 1),
                b'"' if p + 1 < n => return self.double_quoted(p, 1),
                b'<' => {
                    if let Some(t) = self.try_heredoc(p, p + 1) {
                        return t;
                    }
                }
                _ => {}
            }
        }
        let e = self.label_end(p);
        if self.at(e) == b'\\' && is_label_start(self.at(e + 1)) && e + 1 < n {
            let mut q = e;
            while self.at(q) == b'\\' && is_label_start(self.at(q + 1)) && q + 1 < n {
                q = self.label_end(q + 1);
            }
            let id = if self.s[p..e].eq_ignore_ascii_case(b"namespace") {
                T_NAME_RELATIVE
            } else {
                T_NAME_QUALIFIED
            };
            return self.tok(id, p, q);
        }
        let word = self.s[p..e].to_ascii_lowercase();
        match word.as_slice() {
            b"yield" => {
                let w = self.skip_ws_comments(e);
                if w > e && self.starts_ci(w, b"from") && !is_label_char(self.at(w + 4)) && w + 4 <= n {
                    return self.tok(T_YIELD_FROM, p, w + 4);
                }
                return self.tok(T_YIELD, p, e);
            }
            b"enum" => {
                let w = self.skip_ws_comments(e);
                if w > e {
                    if self.starts_ci(w, b"extends") || self.starts_ci(w, b"implements") {
                        return self.tok(T_STRING, p, e);
                    }
                    if is_label_start(self.at(w)) && w < n {
                        return self.tok(T_ENUM, p, e);
                    }
                }
                return self.tok(T_STRING, p, e);
            }
            b"public" | b"protected" | b"private" if self.starts_ci(e, b"(set)") => {
                let id = match word.as_slice() {
                    b"public" => T_PUBLIC_SET,
                    b"protected" => T_PROTECTED_SET,
                    _ => T_PRIVATE_SET,
                };
                return self.tok(id, p, e + 5);
            }
            _ => {}
        }
        let id = keyword_id(&word).unwrap_or(T_STRING);
        self.tok(id, p, e)
    }

    fn scan_initial(&mut self) -> Option<ScanToken> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            return None;
        }
        
        let cursor = if self.starts(p, b"<?=") {
            self.state = St::InScripting;
            return self.tok(T_OPEN_TAG_WITH_ECHO, p, p + 3);
        } else if self.starts_ci(p, b"<?php") && p + 5 <= n {
            let c = self.at(p + 5);
            if p + 5 < n && matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                let mut e = p + 6;
                if c == b'\r' && self.at(e) == b'\n' && e < n {
                    e += 1;
                }
                self.state = St::InScripting;
                return self.tok(T_OPEN_TAG, p, e);
            }
            if p + 5 == n {
                self.state = St::InScripting;
                return self.tok(T_OPEN_TAG, p, p + 5);
            }
            if self.short_tags {
                self.state = St::InScripting;
                return self.tok(T_OPEN_TAG, p, p + 2);
            }
            p + 5
        } else if self.starts(p, b"<?") {
            if self.short_tags {
                self.state = St::InScripting;
                return self.tok(T_OPEN_TAG, p, p + 2);
            }
            p + 2
        } else {
            p + 1
        };
        // inline_char_handler
        let mut cur = cursor;
        loop {
            match self.s[cur.min(n)..].iter().position(|&c| c == b'<') {
                Some(i) => cur = cur + i + 1,
                None => cur = n,
            }
            if cur >= n {
                break;
            }
            if self.s[cur] == b'?' {
                if self.short_tags
                    || self.at(cur + 1) == b'='
                    || (self.starts_ci(cur + 1, b"php")
                        && cur + 4 <= n
                        && (cur + 4 == n || matches!(self.at(cur + 4), b' ' | b'\t' | b'\n' | b'\r')))
                {
                    cur -= 1;
                    break;
                }
            }
        }
        self.tok(T_INLINE_HTML, p, cur.min(n))
    }

    fn scan_looking_for_property(&mut self) -> Option<Option<ScanToken>> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            self.pop_state();
            return None;
        }
        let c = self.s[p];
        if is_ws(c) {
            let mut e = p + 1;
            while e < n && is_ws(self.s[e]) {
                e += 1;
            }
            return Some(self.tok(T_WHITESPACE, p, e));
        }
        if self.starts(p, b"->") {
            return Some(self.tok(T_OBJECT_OPERATOR, p, p + 2));
        }
        if self.starts(p, b"?->") {
            return Some(self.tok(T_NULLSAFE_OBJECT_OPERATOR, p, p + 3));
        }
        if is_label_start(c) {
            let e = self.label_end(p);
            self.pop_state();
            return Some(self.tok(T_STRING, p, e));
        }
        if c == b'#' {
            return Some(self.line_comment(p, 1));
        }
        if self.starts(p, b"//") {
            return Some(self.line_comment(p, 2));
        }
        if self.starts(p, b"/*") {
            return Some(self.block_comment(p));
        }
        self.pop_state();
        None
    }

    /// Rules shared by ST_DOUBLE_QUOTES, ST_BACKQUOTE and ST_HEREDOC that
    /// start with `$` or `{`.
    fn scan_interpolation_start(&mut self, p: usize) -> Option<ScanToken> {
        let n = self.s.len();
        if self.starts(p, b"${") {
            self.push_state(St::LookingForVarname);
            return self.tok(T_DOLLAR_OPEN_CURLY_BRACES, p, p + 2);
        }
        if self.starts(p, b"{$") {
            self.push_state(St::InScripting);
            return self.tok(T_CURLY_OPEN, p, p + 1);
        }
        if self.at(p) == b'$' && is_label_start(self.at(p + 1)) && p + 1 < n {
            let e = self.label_end(p + 1);
            if self.at(e) == b'[' && e < n {
                self.push_state(St::VarOffset);
            } else if (self.starts(e, b"->") && is_label_start(self.at(e + 2)) && e + 2 < n)
                || (self.starts(e, b"?->") && is_label_start(self.at(e + 3)) && e + 3 < n)
            {
                self.push_state(St::LookingForProperty);
            }
            return self.tok(T_VARIABLE, p, e);
        }
        None
    }

    fn scan_quoted(&mut self, quote: u8) -> Option<ScanToken> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            return None;
        }
        if let Some(t) = self.scan_interpolation_start(p) {
            return Some(t);
        }
        if self.s[p] == quote {
            self.state = St::InScripting;
            return self.tok(quote as i32, p, p + 1);
        }
        let mut cur;
        if quote == b'"' && self.dq_scanned > 0 {
            cur = p + self.dq_scanned;
            self.dq_scanned = 0;
        } else {
            cur = p + 1;
            if self.s[p] == b'\\' && cur < n {
                cur += 1;
            }
            while cur < n {
                let ch = self.s[cur];
                cur += 1;
                if ch == quote {
                    cur -= 1;
                    break;
                }
                match ch {
                    b'$' => {
                        if is_label_start(self.at(cur)) || self.at(cur) == b'{' {
                            cur -= 1;
                            break;
                        }
                    }
                    b'{' => {
                        if self.at(cur) == b'$' {
                            cur -= 1;
                            break;
                        }
                    }
                    b'\\' => {
                        if cur < n {
                            cur += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.tok(T_ENCAPSED_AND_WHITESPACE, p, cur.min(n))
    }

    fn scan_heredoc_body(&mut self, nowdoc: bool) -> Option<ScanToken> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            return None;
        }
        if !nowdoc {
            if let Some(t) = self.scan_interpolation_start(p) {
                return Some(t);
            }
        }
        let label = match self.heredocs.last() {
            Some(h) => h.label.clone(),
            None => Vec::new(),
        };
        let len = label.len();
        let mut cur = p;
        while cur < n {
            let ch = self.s[cur];
            cur += 1;
            match ch {
                b'\r' | b'\n' => {
                    if ch == b'\r' && self.at(cur) == b'\n' && cur < n {
                        cur += 1;
                    }
                    let mut indentation = 0;
                    while cur < n && matches!(self.s[cur], b' ' | b'\t') {
                        cur += 1;
                        indentation += 1;
                    }
                    if cur == n {
                        return self.tok(T_ENCAPSED_AND_WHITESPACE, p, n);
                    }
                    if is_label_start(self.s[cur]) && len < n - cur && self.s[cur..cur + len] == label[..] {
                        if is_label_char(self.at(cur + len)) {
                            continue;
                        }
                        cur -= indentation;
                        if let Some(h) = self.heredocs.last_mut() {
                            h.indentation = indentation;
                        }
                        self.state = St::EndHeredoc;
                        return self.tok(T_ENCAPSED_AND_WHITESPACE, p, cur);
                    }
                }
                b'$' if !nowdoc => {
                    if is_label_start(self.at(cur)) || self.at(cur) == b'{' {
                        cur -= 1;
                        break;
                    }
                }
                b'{' if !nowdoc => {
                    if self.at(cur) == b'$' {
                        cur -= 1;
                        break;
                    }
                }
                b'\\' if !nowdoc => {
                    if cur < n && self.s[cur] != b'\n' && self.s[cur] != b'\r' {
                        cur += 1;
                    }
                }
                _ => {}
            }
        }
        self.tok(T_ENCAPSED_AND_WHITESPACE, p, cur.min(n))
    }

    fn scan_var_offset(&mut self) -> Option<ScanToken> {
        let p = self.pos;
        let n = self.s.len();
        if p >= n {
            return None;
        }
        let c = self.s[p];
        if c.is_ascii_digit() {
            let mut best = if c == b'0' {
                p + 1
            } else {
                let mut e = p + 1;
                while self.at(e).is_ascii_digit() {
                    e += 1;
                }
                e
            };
            for e in [
                self.lnum(p),
                self.prefixed_num(p, b'x', |c| c.is_ascii_hexdigit()),
                self.prefixed_num(p, b'b', |c| c == b'0' || c == b'1'),
                self.prefixed_num(p, b'o', |c| (b'0'..=b'7').contains(&c)),
            ]
            .into_iter()
            .flatten()
            {
                best = best.max(e);
            }
            return self.tok(T_NUM_STRING, p, best);
        }
        if c == b'$' && is_label_start(self.at(p + 1)) && p + 1 < n {
            let e = self.label_end(p + 1);
            return self.tok(T_VARIABLE, p, e);
        }
        if c == b']' {
            self.pop_state();
            return self.tok(b']' as i32, p, p + 1);
        }
        if is_label_start(c) {
            let e = self.label_end(p);
            return self.tok(T_STRING, p, e);
        }
        if b";:,.|^&+-/*=%!~$<>?@[(){}\"`".contains(&c) {
            return self.tok(c as i32, p, p + 1);
        }
        if matches!(c, b' ' | b'\n' | b'\r' | b'\t' | b'\\' | b'\'' | b'#') {
            self.pop_state();
            return self.tok(T_ENCAPSED_AND_WHITESPACE, p, p);
        }
        self.tok(T_BAD_CHARACTER, p, p + 1)
    }

    /// Return the next token, or None at the end of input.
    pub fn next_token(&mut self) -> Option<ScanToken> {
        loop {
            match self.state {
                St::Initial => return self.scan_initial(),
                St::InScripting => return self.scan_scripting(),
                St::LookingForProperty => {
                    if let Some(t) = self.scan_looking_for_property() {
                        return t;
                    }
                    if self.pos >= self.s.len() && self.state == St::LookingForProperty {
                        return None;
                    }
                }
                St::DoubleQuotes => return self.scan_quoted(b'"'),
                St::Backquote => return self.scan_quoted(b'`'),
                St::Heredoc => return self.scan_heredoc_body(false),
                St::Nowdoc => return self.scan_heredoc_body(true),
                St::EndHeredoc => {
                    let p = self.pos;
                    let h = self.heredocs.pop();
                    self.state = St::InScripting;
                    let len = h.map(|h| h.indentation + h.label.len()).unwrap_or(0);
                    let e = (p + len).min(self.s.len());
                    return self.tok(T_END_HEREDOC, p, e);
                }
                St::LookingForVarname => {
                    let p = self.pos;
                    let n = self.s.len();
                    if p < n && is_label_start(self.s[p]) {
                        let e = self.label_end(p);
                        if matches!(self.at(e), b'[' | b'}') && e < n {
                            self.pop_state();
                            self.push_state(St::InScripting);
                            return self.tok(T_STRING_VARNAME, p, e);
                        }
                    }
                    self.pop_state();
                    self.push_state(St::InScripting);
                }
                St::VarOffset => return self.scan_var_offset(),
            }
        }
    }
}

fn keyword_id(word: &[u8]) -> Option<i32> {
    Some(match word {
        b"exit" | b"die" => T_EXIT,
        b"fn" => T_FN,
        b"function" => T_FUNCTION,
        b"const" => T_CONST,
        b"return" => T_RETURN,
        b"try" => T_TRY,
        b"catch" => T_CATCH,
        b"finally" => T_FINALLY,
        b"throw" => T_THROW,
        b"if" => T_IF,
        b"elseif" => T_ELSEIF,
        b"endif" => T_ENDIF,
        b"else" => T_ELSE,
        b"while" => T_WHILE,
        b"endwhile" => T_ENDWHILE,
        b"do" => T_DO,
        b"for" => T_FOR,
        b"endfor" => T_ENDFOR,
        b"foreach" => T_FOREACH,
        b"endforeach" => T_ENDFOREACH,
        b"declare" => T_DECLARE,
        b"enddeclare" => T_ENDDECLARE,
        b"instanceof" => T_INSTANCEOF,
        b"as" => T_AS,
        b"switch" => T_SWITCH,
        b"match" => T_MATCH,
        b"endswitch" => T_ENDSWITCH,
        b"case" => T_CASE,
        b"default" => T_DEFAULT,
        b"break" => T_BREAK,
        b"continue" => T_CONTINUE,
        b"goto" => T_GOTO,
        b"echo" => T_ECHO,
        b"print" => T_PRINT,
        b"class" => T_CLASS,
        b"interface" => T_INTERFACE,
        b"trait" => T_TRAIT,
        b"extends" => T_EXTENDS,
        b"implements" => T_IMPLEMENTS,
        b"new" => T_NEW,
        b"clone" => T_CLONE,
        b"var" => T_VAR,
        b"eval" => T_EVAL,
        b"include" => T_INCLUDE,
        b"include_once" => T_INCLUDE_ONCE,
        b"require" => T_REQUIRE,
        b"require_once" => T_REQUIRE_ONCE,
        b"namespace" => T_NAMESPACE,
        b"use" => T_USE,
        b"insteadof" => T_INSTEADOF,
        b"global" => T_GLOBAL,
        b"isset" => T_ISSET,
        b"empty" => T_EMPTY,
        b"__halt_compiler" => T_HALT_COMPILER,
        b"static" => T_STATIC,
        b"abstract" => T_ABSTRACT,
        b"final" => T_FINAL,
        b"private" => T_PRIVATE,
        b"protected" => T_PROTECTED,
        b"public" => T_PUBLIC,
        b"readonly" => T_READONLY,
        b"unset" => T_UNSET,
        b"list" => T_LIST,
        b"array" => T_ARRAY,
        b"callable" => T_CALLABLE,
        b"or" => T_LOGICAL_OR,
        b"and" => T_LOGICAL_AND,
        b"xor" => T_LOGICAL_XOR,
        b"__class__" => T_CLASS_C,
        b"__trait__" => T_TRAIT_C,
        b"__function__" => T_FUNC_C,
        b"__property__" => T_PROPERTY_C,
        b"__method__" => T_METHOD_C,
        b"__line__" => T_LINE,
        b"__file__" => T_FILE,
        b"__dir__" => T_DIR,
        b"__namespace__" => T_NS_C,
        _ => return None,
    })
}

/// Scan the whole input.
pub fn scan_all(s: &[u8], short_tags: bool, skip_shebang: bool) -> Vec<ScanToken> {
    let mut sc = Scanner::new(s, short_tags, skip_shebang);
    let mut out = Vec::new();
    while let Some(t) = sc.next_token() {
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concat(src: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        for t in scan_all(src, false, false) {
            v.extend_from_slice(&src[t.start..t.end]);
        }
        v
    }

    /// Debug helper: SCAN_DUMP=<file with cases separated by \x01> prints the
    /// token stream in the same shape as a token_get_all() dump.
    #[test]
    #[ignore]
    fn dump() {
        let Ok(path) = std::env::var("SCAN_DUMP") else { return };
        let data = std::fs::read(path).unwrap();
        let mut out = String::new();
        for case in data.split(|&c| c == 1) {
            out.push_str("===\n");
            for t in scan_all(case, false, false) {
                let text: String = case[t.start..t.end].iter().map(|b| format!("{:02x}", b)).collect();
                if t.id < 256 {
                    out.push_str(&format!("C {}\n", text));
                } else {
                    let name = crate::token::token_id_to_name(t.id as i64).unwrap_or("?");
                    out.push_str(&format!("{} {} {}\n", name, text, t.line));
                }
            }
        }
        std::fs::write(std::env::var("SCAN_OUT").unwrap(), out).unwrap();
    }

    #[test]
    fn roundtrip() {
        for src in [
            &b"<?php echo \"a $b {$c->d} ${e} $f[1] $g->h\\n\"; `ls $x`;"[..],
            b"<?php $x = <<<EOT\nsome string    \nEOT\n  $y = 2;\n?>",
            b"<?php $x = <<<'EOT'\nsome $string\n  EOT;\n",
            b"hello <?= $a ?>\n\nbar<?php\n#[Attr]\nfunction f(int $x): ?int { return (int)$x; } // c\n# d\n/** doc */",
            b"<?php \"abc $x",
            b"<?php /* unterminated",
        ] {
            assert_eq!(concat(src), src.to_vec());
        }
    }
}
