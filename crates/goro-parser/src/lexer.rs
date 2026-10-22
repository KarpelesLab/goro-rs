use crate::token::{Span, Token, TokenKind, keyword_or_identifier};

/// Lexer state (PHP has multiple scanning modes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum LexerMode {
    /// Outside PHP tags - everything is inline HTML
    Initial,
    /// Inside <?php ... ?> or <?= ... ?>
    Scripting,
    /// Inside a double-quoted string
    DoubleQuote,
    /// Inside a backtick string
    Backtick,
    /// Inside a heredoc
    Heredoc,
}

pub struct Lexer<'a> {
    source: &'a [u8],
    pos: usize,
    line: u32,
    mode_stack: Vec<LexerMode>,
    /// For heredoc: the label we're looking for to end the string
    #[allow(dead_code)]
    heredoc_label: Vec<u8>,
    /// Pending tokens (used when one lexer step produces multiple tokens)
    pending: Vec<Token>,
    /// When true, `skip_whitespace` emits Whitespace/Comment tokens into
    /// `pending` instead of silently skipping them. Used by token_get_all()
    /// and PhpToken::tokenize(). The parser never enables this flag.
    preserve_trivia: bool,
    /// Accumulated trivia tokens (only used when preserve_trivia is true).
    trivia_buffer: Vec<Token>,
    /// Closing delimiter of the interpolated string being scanned (`"`, or
    /// `` ` `` for a shell-exec string).
    dq_term: u8,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        let _ = crate::take_lexer_error();
        Self {
            source,
            pos: 0,
            line: 1,
            mode_stack: vec![LexerMode::Initial],
            heredoc_label: Vec::new(),
            pending: Vec::new(),
            preserve_trivia: false,
            trivia_buffer: Vec::new(),
            dq_term: b'"',
        }
    }

    /// Skip a leading `#!...` line (the CLI does this for the main script).
    pub fn skip_shebang(&mut self) {
        if self.pos == 0 && self.source.starts_with(b"#!") {
            match self.source.iter().position(|&b| b == b'\n') {
                Some(nl) => {
                    self.pos = nl + 1;
                    self.line += 1;
                }
                None => self.pos = self.source.len(),
            }
        }
    }

    /// Turn on whitespace/comment token preservation. Must be set before
    /// calling `tokenize()`.
    pub fn set_preserve_trivia(&mut self, preserve: bool) {
        self.preserve_trivia = preserve;
    }

    fn mode(&self) -> LexerMode {
        *self.mode_stack.last().unwrap_or(&LexerMode::Initial)
    }

    fn push_mode(&mut self, mode: LexerMode) {
        self.mode_stack.push(mode);
    }

    fn pop_mode(&mut self) {
        if self.mode_stack.len() > 1 {
            self.mode_stack.pop();
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.source.get(self.pos).copied()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
        }
        Some(ch)
    }

    fn remaining(&self) -> &[u8] {
        &self.source[self.pos..]
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.remaining().starts_with(prefix)
    }

    #[allow(dead_code)]
    fn starts_with_ci(&self, prefix: &[u8]) -> bool {
        let rem = self.remaining();
        if rem.len() < prefix.len() {
            return false;
        }
        for i in 0..prefix.len() {
            if !rem[i].eq_ignore_ascii_case(&prefix[i]) {
                return false;
            }
        }
        true
    }

    fn skip_whitespace(&mut self) {
        loop {
            // Accumulate a run of whitespace.
            let ws_start = self.pos;
            let ws_line = self.line;
            while let Some(ch) = self.peek() {
                match ch {
                    b' ' | b'\t' | b'\r' | b'\n' => {
                        self.advance();
                    }
                    _ => break,
                }
            }
            if self.preserve_trivia && self.pos > ws_start {
                let bytes = self.source[ws_start..self.pos].to_vec();
                self.trivia_buffer.push(Token::new(
                    TokenKind::Whitespace(bytes),
                    Span::new(ws_start as u32, self.pos as u32, ws_line),
                ));
            }
            // Check for a comment.
            match self.peek() {
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    let c_start = self.pos;
                    let c_line = self.line;
                    self.advance();
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == b'\n' {
                            // Do NOT consume the newline - PHP emits it as T_WHITESPACE
                            break;
                        }
                        if ch == b'?' && self.peek_at(1) == Some(b'>') {
                            break;
                        }
                        self.advance();
                    }
                    if self.preserve_trivia {
                        let bytes = self.source[c_start..self.pos].to_vec();
                        self.trivia_buffer.push(Token::new(
                            TokenKind::LineComment(bytes),
                            Span::new(c_start as u32, self.pos as u32, c_line),
                        ));
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    let c_start = self.pos;
                    let c_line = self.line;
                    self.advance();
                    self.advance();
                    // A doc comment is /** ... */ where the character after the
                    // second `*` is whitespace (space, tab, CR, LF) or `/`
                    // (empty doc comment /**/). PHP rejects /*** ... */ etc.
                    let is_doc = self.peek() == Some(b'*')
                        && matches!(
                            self.peek_at(1),
                            Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(b'/')
                        );
                    loop {
                        match self.advance() {
                            Some(b'*') if self.peek() == Some(b'/') => {
                                self.advance();
                                break;
                            }
                            None => {
                                crate::push_lexer_error(
                                    format!("Unterminated comment starting line {}", c_line),
                                    self.line,
                                );
                                break;
                            }
                            _ => {}
                        }
                    }
                    if self.preserve_trivia {
                        let bytes = self.source[c_start..self.pos].to_vec();
                        let tok = if is_doc {
                            TokenKind::DocComment(bytes)
                        } else {
                            TokenKind::BlockComment(bytes)
                        };
                        self.trivia_buffer.push(Token::new(
                            tok,
                            Span::new(c_start as u32, self.pos as u32, c_line),
                        ));
                    } else if is_doc {
                        // Emit doc-comments into the regular token stream even
                        // when preserve_trivia is off — the parser uses them to
                        // attach to the next declaration. Block (non-doc)
                        // comments are silently skipped.
                        let bytes = self.source[c_start..self.pos].to_vec();
                        self.trivia_buffer.push(Token::new(
                            TokenKind::DocComment(bytes),
                            Span::new(c_start as u32, self.pos as u32, c_line),
                        ));
                    }
                }
                Some(b'#') => {
                    if self.peek_at(1) == Some(b'[') {
                        return;
                    }
                    let c_start = self.pos;
                    let c_line = self.line;
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == b'\n' {
                            // Do NOT consume the newline - PHP emits it as T_WHITESPACE
                            break;
                        }
                        if ch == b'?' && self.peek_at(1) == Some(b'>') {
                            break;
                        }
                        self.advance();
                    }
                    if self.preserve_trivia {
                        let bytes = self.source[c_start..self.pos].to_vec();
                        self.trivia_buffer.push(Token::new(
                            TokenKind::LineComment(bytes),
                            Span::new(c_start as u32, self.pos as u32, c_line),
                        ));
                    }
                }
                _ => return,
            }
        }
    }

    fn scan_identifier(&mut self) -> Vec<u8> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == b'_' || ch >= 0x80 {
                self.advance();
            } else {
                break;
            }
        }
        self.source[start..self.pos].to_vec()
    }

    /// Consume any `("\\"{LABEL})+` continuing the label just scanned;
    /// returns whether at least one segment was consumed.
    fn scan_name_tail(&mut self) -> bool {
        let mut any = false;
        while self.peek() == Some(b'\\') && self.peek_at(1).is_some_and(is_label_start) {
            self.advance();
            self.scan_identifier();
            any = true;
        }
        any
    }

    fn scan_number(&mut self) -> TokenKind {
        let start = self.pos;
        let mut is_float = false;

        // Check for 0x, 0b, 0o prefixes
        if self.peek() == Some(b'0') {
            match self.peek_at(1) {
                Some(b'x' | b'X') if self.peek_at(2).is_some_and(|c| c.is_ascii_hexdigit()) => {
                    self.advance();
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch.is_ascii_hexdigit() || ch == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let s: String = self.source[start..self.pos]
                        .iter()
                        .filter(|b| **b != b'_')
                        .map(|&b| b as char)
                        .collect();
                    match i64::from_str_radix(&s[2..], 16) {
                        Ok(n) => return TokenKind::LongNumber(n),
                        Err(_) => {
                            // Overflow: try u64 first for better precision
                            if let Ok(n) = u64::from_str_radix(&s[2..], 16) {
                                return TokenKind::DoubleNumber(n as f64);
                            }
                            let mut val = 0.0f64;
                            for ch in s[2..].chars() {
                                val = val * 16.0 + ch.to_digit(16).unwrap_or(0) as f64;
                            }
                            return TokenKind::DoubleNumber(val);
                        }
                    }
                }
                Some(b'b' | b'B') if matches!(self.peek_at(2), Some(b'0' | b'1')) => {
                    self.advance();
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == b'0' || ch == b'1' || ch == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let s: String = self.source[start..self.pos]
                        .iter()
                        .filter(|b| **b != b'_')
                        .map(|&b| b as char)
                        .collect();
                    match i64::from_str_radix(&s[2..], 2) {
                        Ok(n) => return TokenKind::LongNumber(n),
                        Err(_) => {
                            // Overflow: try u64 first for better precision, then fall back to f64
                            if let Ok(n) = u64::from_str_radix(&s[2..], 2) {
                                return TokenKind::DoubleNumber(n as f64);
                            }
                            // Truly large numbers: parse string as decimal first
                            let binary_str = &s[2..];
                            // Convert binary string to decimal string to avoid FP accumulation errors
                            let mut val = 0.0f64;
                            for ch in binary_str.chars() {
                                val = val * 2.0 + ch.to_digit(2).unwrap_or(0) as f64;
                            }
                            return TokenKind::DoubleNumber(val);
                        }
                    }
                }
                Some(b'o' | b'O')
                    if self.peek_at(2).is_some_and(|c| (b'0'..=b'7').contains(&c)) =>
                {
                    self.advance();
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if (b'0'..=b'7').contains(&ch) || ch == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let s: String = self.source[start..self.pos]
                        .iter()
                        .filter(|b| **b != b'_')
                        .map(|&b| b as char)
                        .collect();
                    match i64::from_str_radix(&s[2..], 8) {
                        Ok(n) => return TokenKind::LongNumber(n),
                        Err(_) => {
                            let mut val = 0.0f64;
                            for ch in s[2..].chars() {
                                val = val * 8.0 + ch.to_digit(8).unwrap_or(0) as f64;
                            }
                            return TokenKind::DoubleNumber(val);
                        }
                    }
                }
                _ => {
                    // Traditional octal: an integer literal with a leading 0
                    // (LNUM in zend_language_scanner.l). Digits 8/9 make it an
                    // "Invalid numeric literal"; overflow yields a float.
                    let after_zero = self.peek_at(1);
                    let starts_digits = after_zero.is_some_and(|c| c.is_ascii_digit())
                        || (after_zero == Some(b'_')
                            && self.peek_at(2).is_some_and(|c| c.is_ascii_digit()));
                    if starts_digits {
                        let oct_start = self.pos;
                        self.advance(); // skip leading 0
                        while let Some(ch) = self.peek() {
                            if ch.is_ascii_digit()
                                || (ch == b'_'
                                    && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()))
                            {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let is_float_lit = match self.peek() {
                            Some(b'e') | Some(b'E') => true,
                            Some(b'.') => {
                                let next = self.peek_at(1);
                                !(next.is_some_and(|c| {
                                    (c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
                                        && c != b'e'
                                        && c != b'E'
                                }) || next == Some(b'.'))
                            }
                            _ => false,
                        };
                        if !is_float_lit {
                            let digits: Vec<u8> = self.source[oct_start + 1..self.pos]
                                .iter()
                                .copied()
                                .filter(|b| *b != b'_')
                                .collect();
                            if digits.iter().any(|d| *d == b'8' || *d == b'9') {
                                crate::push_lexer_error(
                                    "Invalid numeric literal".to_string(),
                                    self.line,
                                );
                                return TokenKind::LongNumber(0);
                            }
                            let mut v: i64 = 0;
                            let mut overflow = false;
                            for d in &digits {
                                match v
                                    .checked_mul(8)
                                    .and_then(|x| x.checked_add((*d - b'0') as i64))
                                {
                                    Some(n) => v = n,
                                    None => {
                                        overflow = true;
                                        break;
                                    }
                                }
                            }
                            if !overflow {
                                return TokenKind::LongNumber(v);
                            }
                            let mut f = 0.0f64;
                            for d in &digits {
                                f = f * 8.0 + (*d - b'0') as f64;
                            }
                            return TokenKind::DoubleNumber(f);
                        }
                        // A float literal like 08.5 / 012e3: parse as decimal.
                        self.pos = oct_start;
                    }
                }
            }
        }

        // Decimal digits
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == b'_' {
                self.advance();
            } else {
                break;
            }
        }

        // Fractional part
        if self.peek() == Some(b'.') {
            let next = self.peek_at(1);
            // Allow "12." (dot followed by non-digit) and "12.5" (dot followed by digit)
            // But NOT "12.method" (dot followed by letter/underscore - that's a method call on int)
            let is_method_call = next.is_some_and(|c| {
                (c.is_ascii_alphabetic() || c == b'_' || c >= 0x80) && c != b'e' && c != b'E'
            });
            // Also not ".." (range operator)
            let is_double_dot = next == Some(b'.');
            if !is_method_call && !is_double_dot {
                is_float = true;
                self.advance(); // consume '.'
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() || ch == b'_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // Exponent part
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.advance();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.advance();
            }
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == b'_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let s: String = self.source[start..self.pos]
            .iter()
            .filter(|b| **b != b'_')
            .map(|&b| b as char)
            .collect();

        if is_float {
            TokenKind::DoubleNumber(s.parse::<f64>().unwrap_or(0.0))
        } else {
            // Try parsing as i64, fall back to float for overflow
            match s.parse::<i64>() {
                Ok(n) => TokenKind::LongNumber(n),
                Err(_) => TokenKind::DoubleNumber(s.parse::<f64>().unwrap_or(0.0)),
            }
        }
    }

    fn scan_single_quoted_string(&mut self) -> Vec<u8> {
        // Opening quote already consumed
        let mut result = Vec::new();
        loop {
            match self.advance() {
                Some(b'\'') => break,
                Some(b'\\') => match self.peek() {
                    Some(b'\'') => {
                        self.advance();
                        result.push(b'\'');
                    }
                    Some(b'\\') => {
                        self.advance();
                        result.push(b'\\');
                    }
                    _ => {
                        result.push(b'\\');
                    }
                },
                Some(ch) => result.push(ch),
                None => break,
            }
        }
        result
    }

    fn scan_double_quoted_string(&mut self) -> TokenKind {
        // Opening quote already consumed
        let mut result = Vec::new();
        loop {
            match self.peek() {
                Some(c) if c == self.dq_term => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    match self.advance() {
                        Some(b'`') if self.dq_term == b'`' => result.push(b'`'),
                        Some(b'"') if self.dq_term == b'`' => {
                            result.push(b'\\');
                            result.push(b'"');
                        }
                        Some(b'n') => result.push(b'\n'),
                        Some(b'r') => result.push(b'\r'),
                        Some(b't') => result.push(b'\t'),
                        Some(b'v') => result.push(0x0B),
                        Some(b'e') => result.push(0x1B),
                        Some(b'f') => result.push(0x0C),
                        Some(b'\\') => result.push(b'\\'),
                        Some(b'$') => result.push(b'$'),
                        Some(b'"') => result.push(b'"'),
                        Some(b'u') if self.peek() == Some(b'{') => {
                            // \u{HEX} - Unicode codepoint encoded as UTF-8.
                            self.advance(); // consume '{'
                            let mut hex_str = String::new();
                            let mut bad = false;
                            loop {
                                match self.peek() {
                                    Some(b'}') => break,
                                    Some(ch) if ch.is_ascii_hexdigit() => {
                                        hex_str.push(ch as char);
                                        self.advance();
                                    }
                                    _ => {
                                        bad = true;
                                        break;
                                    }
                                }
                            }
                            if !bad {
                                self.advance(); // consume '}'
                            }
                            encode_unicode_escape(&hex_str, bad, self.line, &mut result);
                        }
                        Some(x_ch @ (b'x' | b'X')) => {
                            let mut hex = Vec::new();
                            for _ in 0..2 {
                                if let Some(ch) = self.peek() {
                                    if ch.is_ascii_hexdigit() {
                                        hex.push(self.advance().unwrap());
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if !hex.is_empty() {
                                let s: String = hex.iter().map(|&b| b as char).collect();
                                result.push(u8::from_str_radix(&s, 16).unwrap_or(0));
                            } else {
                                // \x not followed by hex digits: keep literal \x
                                result.push(b'\\');
                                result.push(x_ch);
                            }
                        }
                        Some(ch) if ch.is_ascii_digit() && ch < b'8' => {
                            // Octal
                            let mut oct = vec![ch];
                            for _ in 0..2 {
                                if let Some(ch) = self.peek() {
                                    if ch.is_ascii_digit() && ch < b'8' {
                                        oct.push(self.advance().unwrap());
                                    } else {
                                        break;
                                    }
                                }
                            }
                            let s: String = oct.iter().map(|&b| b as char).collect();
                            let v = u32::from_str_radix(&s, 8).unwrap_or(0);
                            if v > 0xFF {
                                crate::push_lexer_warning(
                                    format!(
                                        "Octal escape sequence overflow \\{} is greater than \\377",
                                        s
                                    ),
                                    self.line,
                                );
                            }
                            result.push((v & 0xFF) as u8);
                        }
                        Some(ch) => {
                            result.push(b'\\');
                            result.push(ch);
                        }
                        None => result.push(b'\\'),
                    }
                }
                Some(b'$')
                    if self
                        .peek_at(1)
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80) =>
                {
                    // Variable interpolation
                    // Always emit the current result as an InterpolatedStringPart
                    // (even if empty, for the first variable in the string)
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    self.advance(); // consume $
                    let var_name = self.scan_identifier();
                    self.pending.push(Token::new(
                        TokenKind::Variable(var_name),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    // Check for ->property access (e.g., "$obj->name")
                    if self.peek() == Some(b'-') && self.peek_at(1) == Some(b'>') {
                        self.advance(); // consume -
                        self.advance(); // consume >
                        self.pending.push(Token::new(
                            TokenKind::Arrow,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        if self
                            .peek()
                            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
                        {
                            let prop_name = self.scan_identifier();
                            self.pending.push(Token::new(
                                TokenKind::Identifier(prop_name),
                                Span::new(self.pos as u32, self.pos as u32, self.line),
                            ));
                        }
                    }
                    // "$obj?->name" (only before a label)
                    else if self.peek() == Some(b'?')
                        && self.peek_at(1) == Some(b'-')
                        && self.peek_at(2) == Some(b'>')
                        && self
                            .peek_at(3)
                            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
                    {
                        self.advance();
                        self.advance();
                        self.advance();
                        self.pending.push(Token::new(
                            TokenKind::NullsafeArrow,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        let prop_name = self.scan_identifier();
                        self.pending.push(Token::new(
                            TokenKind::Identifier(prop_name),
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                    }
                    // Check for [index] access (e.g., "$arr[0]", "$arr[$key]")
                    else if self.peek() == Some(b'[') {
                        self.advance(); // consume [
                        self.pending.push(Token::new(
                            TokenKind::OpenBracket,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        // Scan index: could be number, string, or variable
                        match self.peek() {
                            // "$a[3]" / "$a[-3]": T_NUM_STRING
                            Some(b'0'..=b'9') | Some(b'-')
                                if self.peek().is_some_and(|c| c.is_ascii_digit())
                                    || self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) =>
                            {
                                let neg = self.peek() == Some(b'-');
                                let start = self.pos + neg as usize;
                                let (num_kind, end) = var_offset_number(self.source, start, neg);
                                while self.pos < end {
                                    self.advance();
                                }
                                self.pending.push(Token::new(
                                    num_kind,
                                    Span::new(self.pos as u32, self.pos as u32, self.line),
                                ));
                            }
                            Some(b'$') => {
                                self.advance();
                                let idx_var = self.scan_identifier();
                                self.pending.push(Token::new(
                                    TokenKind::Variable(idx_var),
                                    Span::new(self.pos as u32, self.pos as u32, self.line),
                                ));
                            }
                            // <ST_VAR_OFFSET>[ \n\r\t\\'#]: "Invalid char in
                            // var offset", an empty T_ENCAPSED_AND_WHITESPACE
                            // the grammar rejects.
                            Some(b'\'' | b' ' | b'\n' | b'\r' | b'\t' | b'\\' | b'#') => {
                                crate::push_lexer_error(
                                    "syntax error, unexpected string content \"\", expecting \"-\" or identifier or variable or number".into(),
                                    self.line,
                                );
                            }
                            Some(b'"') => {
                                // Skip for now - string key in interpolation
                            }
                            _ => {
                                // Bare identifier as key
                                if self.peek().is_some_and(|c| {
                                    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
                                }) {
                                    let key = self.scan_identifier();
                                    self.pending.push(Token::new(
                                        TokenKind::Identifier(key),
                                        Span::new(self.pos as u32, self.pos as u32, self.line),
                                    ));
                                }
                            }
                        }
                        if self.peek() == Some(b']') {
                            self.advance();
                            self.pending.push(Token::new(
                                TokenKind::CloseBracket,
                                Span::new(self.pos as u32, self.pos as u32, self.line),
                            ));
                        }
                    }
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    // ${var} / ${expr} interpolation (deprecated syntax)
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    let (toks, end, msg) =
                        lex_dollar_brace_interpolation(self.source, self.pos, self.line);
                    crate::push_lexer_deprecated(msg.to_string(), self.line);
                    self.pending.extend(toks);
                    while self.pos < end {
                        self.advance();
                    }
                }
                Some(b'{')
                    if self.peek_at(1).is_some_and(|c| c == b'$')
                        && self.peek_at(2).is_some_and(|c| {
                            c.is_ascii_alphabetic()
                                || c == b'_'
                                || c >= 0x80
                                || c == b'$'
                                || c == b'{'
                        }) =>
                {
                    // {$expr} interpolation: the braced part is PHP code.
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    let (toks, end) = lex_braced_interpolation(self.source, self.pos, self.line);
                    self.pending.extend(toks);
                    while self.pos < end {
                        self.advance();
                    }
                }
                Some(b'{') => {
                    self.advance();
                    result.push(b'{');
                }
                Some(ch) => {
                    self.advance();
                    result.push(ch);
                }
                None => break,
            }
        }

        // If we emitted interpolation tokens, this is the end part
        if !self.pending.is_empty() {
            // Push the final string part and return the first pending token
            self.pending.push(Token::new(
                TokenKind::InterpolatedStringEnd(result),
                Span::new(self.pos as u32, self.pos as u32, self.line),
            ));
            let first = self.pending.remove(0);
            return first.kind;
        }

        TokenKind::ConstantString(result)
    }

    fn try_scan_cast(&mut self) -> Option<TokenKind> {
        // We're positioned at '(' - check for (int), (float), etc.
        let saved_pos = self.pos;
        let saved_line = self.line;

        self.advance(); // consume (

        // Skip whitespace inside cast
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.advance();
        }

        let ident_start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphabetic() {
                self.advance();
            } else {
                break;
            }
        }
        let ident = &self.source[ident_start..self.pos];

        // Skip whitespace after type
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.advance();
        }

        if self.peek() != Some(b')') {
            self.pos = saved_pos;
            self.line = saved_line;
            return None;
        }

        let lower: Vec<u8> = ident.iter().map(|b| b.to_ascii_lowercase()).collect();
        let kind = match lower.as_slice() {
            b"int" | b"integer" => TokenKind::IntCast,
            b"float" | b"double" => TokenKind::FloatCast,
            b"real" => TokenKind::RealCast,
            b"string" | b"binary" => TokenKind::StringCast,
            b"bool" | b"boolean" => TokenKind::BoolCast,
            b"array" => TokenKind::ArrayCast,
            b"object" => TokenKind::ObjectCast,
            b"unset" => TokenKind::UnsetCast,
            // In PHP 8.5, (void) is a valid cast used as a statement:
            // (void)expr; - evaluates and discards the expression
            // Using it as a value (e.g., $x = (void)$y) is a parse error
            b"void" => TokenKind::VoidCast,
            _ => {
                self.pos = saved_pos;
                self.line = saved_line;
                return None;
            }
        };

        let canonical = match lower.as_slice() {
            b"integer" => Some(("integer", "int")),
            b"double" => Some(("double", "float")),
            b"binary" => Some(("binary", "string")),
            b"boolean" => Some(("boolean", "bool")),
            _ => None,
        };
        if let Some((used, canon)) = canonical {
            crate::push_lexer_deprecated(
                format!(
                    "Non-canonical cast ({}) is deprecated, use the ({}) cast instead",
                    used, canon
                ),
                self.line,
            );
        }

        self.advance(); // consume )
        Some(kind)
    }

    pub fn next_token(&mut self) -> Token {
        // Return pending tokens first
        if let Some(token) = self.pending.first().cloned() {
            self.pending.remove(0);
            return token;
        }

        let token = match self.mode() {
            LexerMode::Initial => self.scan_initial(),
            LexerMode::Scripting => self.scan_scripting(),
            LexerMode::DoubleQuote | LexerMode::Backtick | LexerMode::Heredoc => {
                // TODO: implement string interpolation scanning modes
                self.scan_scripting()
            }
        };

        // If skip_whitespace accumulated trivia tokens (only when preserve_trivia
        // is on), emit them first and buffer the real token for the next call.
        if !self.trivia_buffer.is_empty() {
            let mut trivia = std::mem::take(&mut self.trivia_buffer);
            let first = trivia.remove(0);
            // Push remaining trivia, then the real token, onto the pending queue.
            self.pending.extend(trivia);
            self.pending.push(token);
            return first;
        }

        token
    }

    fn scan_initial(&mut self) -> Token {
        let start = self.pos;
        let start_line = self.line;

        // Look for <?php or <?=
        loop {
            if self.pos >= self.source.len() {
                // End of file - emit any remaining HTML
                if self.pos > start {
                    return Token::new(
                        TokenKind::InlineHtml(self.source[start..self.pos].to_vec()),
                        Span::new(start as u32, self.pos as u32, start_line),
                    );
                }
                return Token::new(
                    TokenKind::Eof,
                    Span::new(self.pos as u32, self.pos as u32, self.line),
                );
            }

            if self.source.len() >= self.pos + 5
                && self.source[self.pos..self.pos + 5].eq_ignore_ascii_case(b"<?php")
                && self
                    .peek_at(5)
                    .is_none_or(|c| c == b' ' || c == b'\t' || c == b'\n' || c == b'\r')
            {
                // Emit HTML before the tag
                if self.pos > start {
                    return Token::new(
                        TokenKind::InlineHtml(self.source[start..self.pos].to_vec()),
                        Span::new(start as u32, self.pos as u32, start_line),
                    );
                }
                let tag_start = self.pos;
                self.pos += 5; // skip "<?php"
                // PHP's T_OPEN_TAG includes the trailing whitespace character
                // that follows the literal "<?php" (one space, tab, or newline).
                if let Some(c) = self.peek() {
                    if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                        if c == b'\r' && self.peek_at(1) == Some(b'\n') {
                            self.advance();
                            self.advance();
                        } else {
                            self.advance();
                        }
                    }
                }
                self.push_mode(LexerMode::Scripting);
                return Token::new(
                    TokenKind::OpenTag,
                    Span::new(tag_start as u32, self.pos as u32, start_line),
                );
            }

            if self.starts_with(b"<?=") {
                if self.pos > start {
                    return Token::new(
                        TokenKind::InlineHtml(self.source[start..self.pos].to_vec()),
                        Span::new(start as u32, self.pos as u32, start_line),
                    );
                }
                let tag_start = self.pos;
                self.pos += 3;
                self.push_mode(LexerMode::Scripting);
                return Token::new(
                    TokenKind::OpenTagShort,
                    Span::new(tag_start as u32, self.pos as u32, self.line),
                );
            }

            if self.starts_with(b"<?") && crate::short_open_tag() {
                if self.pos > start {
                    return Token::new(
                        TokenKind::InlineHtml(self.source[start..self.pos].to_vec()),
                        Span::new(start as u32, self.pos as u32, start_line),
                    );
                }
                let tag_start = self.pos;
                self.pos += 2;
                self.push_mode(LexerMode::Scripting);
                return Token::new(
                    TokenKind::OpenTag,
                    Span::new(tag_start as u32, self.pos as u32, start_line),
                );
            }

            self.advance();
        }
    }

    fn scan_scripting(&mut self) -> Token {
        self.skip_whitespace();

        let start = self.pos;
        let start_line = self.line;

        let Some(ch) = self.peek() else {
            return Token::new(
                TokenKind::Eof,
                Span::new(start as u32, start as u32, self.line),
            );
        };

        let kind = match ch {
            b'$' if self.peek_at(1) == Some(b'{') => {
                // ${expr} - variable variable with expression
                self.advance(); // skip $
                self.advance(); // skip {
                // Scan expression until matching }
                let mut depth = 1;
                let start = self.pos;
                let start_line_inner = self.line;
                while depth > 0 {
                    match self.peek() {
                        Some(b'{') => {
                            depth += 1;
                            self.advance();
                        }
                        Some(b'}') => {
                            depth -= 1;
                            if depth > 0 {
                                self.advance();
                            }
                        }
                        None => break,
                        _ => {
                            self.advance();
                        }
                    }
                }
                let expr_bytes = self.source[start..self.pos].to_vec();
                if self.peek() == Some(b'}') {
                    self.advance(); // skip }
                }
                // Anything other than `${name}` / `${$name}` is a real
                // expression: rewind and let the parser handle `${ expr }`.
                let is_ident = |b: &[u8]| {
                    !b.is_empty()
                        && (b[0].is_ascii_alphabetic() || b[0] == b'_' || b[0] >= 0x80)
                        && b.iter()
                            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c >= 0x80)
                };
                // Note `${name}` is a constant lookup in PHP, not `$name`.
                let simple = expr_bytes.first() == Some(&b'$') && is_ident(&expr_bytes[1..]);
                if !simple {
                    self.pos = start;
                    self.line = start_line_inner;
                    return Token::new(
                        TokenKind::DollarOpenBrace,
                        Span::new((start - 2) as u32, start as u32, start_line_inner),
                    );
                }
                // If the expression is a simple variable name, treat as VariableVariable
                let expr_str: Vec<u8> = expr_bytes.iter().copied().filter(|b| *b != b'$').collect();
                if !expr_str.is_empty() && expr_str[0] != b'{' {
                    // Simple case: ${varname} or ${$varname}
                    if expr_bytes.starts_with(b"$") {
                        // ${$var} - variable variable
                        let name: Vec<u8> = expr_bytes[1..].to_vec();
                        TokenKind::VariableVariable(name)
                    } else {
                        // ${name} - simple variable access
                        TokenKind::Variable(expr_bytes)
                    }
                } else {
                    TokenKind::Variable(expr_bytes)
                }
            }
            b'$' if self.peek_at(1) == Some(b'$')
                && matches!(self.peek_at(2), Some(b'$') | Some(b'{')) =>
            {
                // `$` of `$$$v` / `$${expr}`: one level of indirection over
                // the simple variable that follows.
                self.advance();
                TokenKind::Dollar
            }
            b'$' if self.peek_at(1) == Some(b'$') => {
                // $$var - variable variable
                self.advance(); // skip first $
                self.advance(); // skip second $
                if self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
                {
                    let name = self.scan_identifier();
                    TokenKind::VariableVariable(name)
                } else {
                    // Just $$ without a valid identifier
                    TokenKind::Variable(b"$".to_vec())
                }
            }
            b'$' if self
                .peek_at(1)
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80) =>
            {
                self.advance(); // skip $
                let name = self.scan_identifier();
                TokenKind::Variable(name)
            }

            b'0'..=b'9' => self.scan_number(),

            b'\'' => {
                self.advance();
                let s = self.scan_single_quoted_string();
                TokenKind::ConstantString(s)
            }

            b'"' => {
                self.advance();
                self.scan_double_quoted_string()
            }

            // `cmd` is shell_exec("cmd") (deprecated in PHP 8.5).
            b'`' if !self.preserve_trivia => {
                self.advance();
                crate::push_lexer_deprecated(
                    "The backtick (`) operator is deprecated, use shell_exec() instead".to_string(),
                    start_line,
                );
                self.dq_term = b'`';
                let first = self.scan_double_quoted_string();
                self.dq_term = b'"';
                let rest = std::mem::take(&mut self.pending);
                let span = Span::new(self.pos as u32, self.pos as u32, start_line);
                self.pending.push(Token::new(TokenKind::OpenParen, span));
                self.pending.push(Token::new(first, span));
                self.pending.extend(rest);
                self.pending.push(Token::new(TokenKind::CloseParen, span));
                TokenKind::Identifier(b"shell_exec".to_vec())
            }

            b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xFF => {
                let ident = self.scan_identifier();

                // `{LABEL}("\\"{LABEL})+`: a namespaced name is one token
                // (T_NAME_QUALIFIED, or T_NAME_RELATIVE for `namespace\...`).
                if self.scan_name_tail() {
                    let text = self.source[start..self.pos].to_vec();
                    let kind = if ident.eq_ignore_ascii_case(b"namespace") {
                        TokenKind::NameRelative(text)
                    } else {
                        TokenKind::NameQualified(text)
                    };
                    return Token::new(kind, Span::new(start as u32, self.pos as u32, start_line));
                }

                // Handle b"...", b'...', and b<<< binary string prefix (PHP treats them as regular strings)
                if (ident == b"b" || ident == b"B") && self.pos < self.source.len() {
                    if self.source[self.pos] == b'"' {
                        self.advance();
                        let tok = self.scan_double_quoted_string();
                        return Token::new(
                            tok,
                            Span::new(self.pos as u32, self.pos as u32, start_line),
                        );
                    } else if self.source[self.pos] == b'\'' {
                        self.advance();
                        let s = self.scan_single_quoted_string();
                        return Token::new(
                            TokenKind::ConstantString(s),
                            Span::new(self.pos as u32, self.pos as u32, start_line),
                        );
                    } else if self.remaining().starts_with(b"<<<") {
                        // b<<< heredoc/nowdoc - skip the <<< and parse as heredoc
                        self.advance(); // <
                        self.advance(); // <
                        self.advance(); // <
                        let tok = self.scan_heredoc_start();
                        return Token::new(
                            tok,
                            Span::new(start as u32, self.pos as u32, start_line),
                        );
                    }
                }

                // Special handling: "yield from" is two words but one token
                if ident.eq_ignore_ascii_case(b"yield") {
                    let saved = self.pos;
                    let saved_line = self.line;
                    self.skip_whitespace();
                    if self.remaining().len() >= 4 {
                        let next4: Vec<u8> = self.remaining()[..4]
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        if next4 == b"from"
                            && self
                                .peek_at(4)
                                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != b'_' && c < 0x80)
                        {
                            self.pos += 4;
                            return Token::new(
                                TokenKind::YieldFrom,
                                Span::new(start as u32, self.pos as u32, start_line),
                            );
                        }
                    }
                    self.pos = saved;
                    self.line = saved_line;
                    TokenKind::Yield
                } else if ident.eq_ignore_ascii_case(b"enum") {
                    // `enum` is only a keyword when whitespace and a name
                    // follow (other than extends/implements), like the
                    // `"enum"{WHITESPACE_OR_COMMENTS}("extends"|"implements")`
                    // rules of zend_language_scanner.
                    let saved = self.pos;
                    let saved_line = self.line;
                    self.skip_whitespace();
                    let moved = self.pos > saved;
                    let rest = self.remaining();
                    let word_is = |w: &[u8]| {
                        rest.len() >= w.len()
                            && rest[..w.len()].eq_ignore_ascii_case(w)
                            && rest.get(w.len()).is_none_or(|c| {
                                !c.is_ascii_alphanumeric() && *c != b'_' && *c < 0x80
                            })
                    };
                    let is_kw = moved
                        && rest
                            .first()
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_' || *c >= 0x80)
                        && !word_is(b"extends")
                        && !word_is(b"implements");
                    self.pos = saved;
                    self.line = saved_line;
                    if is_kw {
                        TokenKind::Enum
                    } else {
                        TokenKind::Identifier(ident.clone())
                    }
                } else {
                    keyword_or_identifier(&ident)
                }
            }

            // Operators and delimiters
            b'+' => {
                self.advance();
                match self.peek() {
                    Some(b'+') => {
                        self.advance();
                        TokenKind::Increment
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::PlusAssign
                    }
                    _ => TokenKind::Plus,
                }
            }
            b'-' => {
                self.advance();
                match self.peek() {
                    Some(b'-') => {
                        self.advance();
                        TokenKind::Decrement
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::MinusAssign
                    }
                    Some(b'>') => {
                        self.advance();
                        TokenKind::Arrow
                    }
                    _ => TokenKind::Minus,
                }
            }
            b'*' => {
                self.advance();
                match self.peek() {
                    Some(b'*') => {
                        self.advance();
                        if self.peek() == Some(b'=') {
                            self.advance();
                            TokenKind::PowAssign
                        } else {
                            TokenKind::Pow
                        }
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::StarAssign
                    }
                    _ => TokenKind::Star,
                }
            }
            b'/' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::SlashAssign
                } else {
                    TokenKind::Slash
                }
            }
            b'%' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::PercentAssign
                } else {
                    TokenKind::Percent
                }
            }
            b'.' => {
                self.advance();
                match self.peek() {
                    Some(b'=') => {
                        self.advance();
                        TokenKind::DotAssign
                    }
                    Some(b'.') if self.peek_at(1) == Some(b'.') => {
                        self.advance();
                        self.advance();
                        TokenKind::Ellipsis
                    }
                    Some(ch) if ch.is_ascii_digit() => {
                        // Float starting with .
                        self.pos = start;
                        self.line = start_line;
                        self.scan_number()
                    }
                    _ => TokenKind::Dot,
                }
            }
            b'&' => {
                self.advance();
                match self.peek() {
                    Some(b'&') => {
                        self.advance();
                        TokenKind::BooleanAnd
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::AmpersandAssign
                    }
                    _ => TokenKind::Ampersand,
                }
            }
            b'|' => {
                self.advance();
                match self.peek() {
                    Some(b'|') => {
                        self.advance();
                        TokenKind::BooleanOr
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::PipeAssign
                    }
                    Some(b'>') => {
                        self.advance();
                        TokenKind::PipeGreater
                    }
                    _ => TokenKind::Pipe,
                }
            }
            b'^' => {
                self.advance();
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::CaretAssign
                } else {
                    TokenKind::Caret
                }
            }
            b'~' => {
                self.advance();
                TokenKind::Tilde
            }
            b'!' => {
                self.advance();
                match self.peek() {
                    Some(b'=') => {
                        self.advance();
                        if self.peek() == Some(b'=') {
                            self.advance();
                            TokenKind::NotIdentical
                        } else {
                            TokenKind::NotEqual
                        }
                    }
                    _ => TokenKind::BooleanNot,
                }
            }
            b'<' => {
                self.advance();
                match self.peek() {
                    Some(b'<') => {
                        self.advance();
                        match self.peek() {
                            Some(b'=') => {
                                self.advance();
                                TokenKind::ShiftLeftAssign
                            }
                            Some(b'<') => {
                                // Heredoc / Nowdoc
                                self.advance();
                                self.scan_heredoc_start()
                            }
                            _ => TokenKind::ShiftLeft,
                        }
                    }
                    Some(b'=') => {
                        self.advance();
                        if self.peek() == Some(b'>') {
                            self.advance();
                            TokenKind::Spaceship
                        } else {
                            TokenKind::LessEqual
                        }
                    }
                    Some(b'>') => {
                        self.advance();
                        TokenKind::NotEqual // <> is equivalent to !=
                    }
                    _ => TokenKind::Less,
                }
            }
            b'>' => {
                self.advance();
                match self.peek() {
                    Some(b'>') => {
                        self.advance();
                        if self.peek() == Some(b'=') {
                            self.advance();
                            TokenKind::ShiftRightAssign
                        } else {
                            TokenKind::ShiftRight
                        }
                    }
                    Some(b'=') => {
                        self.advance();
                        TokenKind::GreaterEqual
                    }
                    _ => TokenKind::Greater,
                }
            }
            b'=' => {
                self.advance();
                match self.peek() {
                    Some(b'=') => {
                        self.advance();
                        if self.peek() == Some(b'=') {
                            self.advance();
                            TokenKind::Identical
                        } else {
                            TokenKind::Equal
                        }
                    }
                    Some(b'>') => {
                        self.advance();
                        TokenKind::DoubleArrow
                    }
                    _ => TokenKind::Assign,
                }
            }
            b'?' => {
                self.advance();
                match self.peek() {
                    Some(b'?') => {
                        self.advance();
                        if self.peek() == Some(b'=') {
                            self.advance();
                            TokenKind::NullCoalesceAssign
                        } else {
                            TokenKind::NullCoalesce
                        }
                    }
                    Some(b'>') => {
                        // Close tag ?>
                        self.advance();
                        self.pop_mode();
                        // ?> is followed by an optional newline that is consumed
                        if self.peek() == Some(b'\n') {
                            self.advance();
                        } else if self.peek() == Some(b'\r') {
                            self.advance();
                            if self.peek() == Some(b'\n') {
                                self.advance();
                            }
                        }
                        TokenKind::CloseTag
                    }
                    Some(b'-') if self.peek_at(1) == Some(b'>') => {
                        self.advance();
                        self.advance();
                        TokenKind::NullsafeArrow
                    }
                    _ => TokenKind::QuestionMark,
                }
            }
            b'@' => {
                self.advance();
                TokenKind::At
            }
            b'(' => {
                // Try cast first
                if let Some(cast) = self.try_scan_cast() {
                    cast
                } else {
                    self.advance();
                    TokenKind::OpenParen
                }
            }
            b')' => {
                self.advance();
                TokenKind::CloseParen
            }
            b'[' => {
                self.advance();
                TokenKind::OpenBracket
            }
            b']' => {
                self.advance();
                TokenKind::CloseBracket
            }
            b'{' => {
                self.advance();
                TokenKind::OpenBrace
            }
            b'}' => {
                self.advance();
                TokenKind::CloseBrace
            }
            b';' => {
                self.advance();
                TokenKind::Semicolon
            }
            b',' => {
                self.advance();
                TokenKind::Comma
            }
            b':' => {
                self.advance();
                if self.peek() == Some(b':') {
                    self.advance();
                    TokenKind::DoubleColon
                } else {
                    TokenKind::Colon
                }
            }
            b'\\' => {
                self.advance();
                // `"\\"{LABEL}("\\"{LABEL})*`: T_NAME_FULLY_QUALIFIED.
                if self.peek().is_some_and(is_label_start) {
                    self.scan_identifier();
                    self.scan_name_tail();
                    TokenKind::NameFullyQualified(self.source[start..self.pos].to_vec())
                } else {
                    TokenKind::Backslash
                }
            }

            b'#' if self.peek_at(1) == Some(b'[') => {
                self.advance(); // skip #
                self.advance(); // skip [
                TokenKind::AttributeOpen
            }

            _ => {
                // Unknown character - skip it and produce an identifier
                if ch < 0x20 || ch == 0x7F {
                    // T_BAD_CHARACTER
                    crate::push_lexer_error(
                        format!("syntax error, unexpected character 0x{:02X}", ch),
                        self.line,
                    );
                }
                self.advance();
                TokenKind::Identifier(vec![ch])
            }
        };

        Token::new(kind, Span::new(start as u32, self.pos as u32, start_line))
    }

    fn scan_heredoc_start(&mut self) -> TokenKind {
        // We've consumed <<<, now scan the label
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.advance();
        }

        let is_nowdoc = self.peek() == Some(b'\'');
        if is_nowdoc || self.peek() == Some(b'"') {
            self.advance(); // opening quote
        }

        let label_start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let label = self.source[label_start..self.pos].to_vec();

        if (is_nowdoc || self.peek() == Some(b'"')) && matches!(self.peek(), Some(b'\'' | b'"')) {
            self.advance(); // closing quote
        }

        // Consume the newline after the label
        let label_newline = matches!(self.peek(), Some(b'\r' | b'\n'));
        if self.peek() == Some(b'\r') {
            self.advance();
        }
        if self.peek() == Some(b'\n') {
            self.advance();
        }

        // Now scan until we find the closing label
        let mut content = Vec::new();
        let mut closing_indent = Vec::new();
        let mut found_closing = false;
        loop {
            // Check for closing label at start of line
            let _line_start = self.pos;
            // Allow optional whitespace (indented heredoc)
            let mut indent = Vec::new();
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                indent.push(self.advance().unwrap());
            }

            if self.remaining().starts_with(&label) {
                let after_label = self.pos + label.len();
                let next_ch = self.source.get(after_label).copied();
                // The closing label may be followed by anything but a label character.
                if !matches!(next_ch, Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80)
                {
                    self.pos = after_label;
                    closing_indent = indent;
                    found_closing = true;
                    // Consume ; and newline if present
                    break;
                }
            }

            // Not the closing label, add the indent back and scan the line
            content.extend_from_slice(&indent);
            // A line ends at "\n", "\r\n" or a bare "\r" ({NEWLINE}).
            loop {
                match self.advance() {
                    Some(b'\n') => {
                        content.push(b'\n');
                        break;
                    }
                    Some(b'\r') => {
                        content.push(b'\r');
                        if self.peek() == Some(b'\n') {
                            self.advance();
                            content.push(b'\n');
                        }
                        break;
                    }
                    Some(ch) => content.push(ch),
                    None => break,
                }
            }
            if self.pos >= self.source.len() {
                break;
            }
        }

        if !found_closing && label_newline {
            crate::push_lexer_error("syntax error, unexpected end of file, expecting variable or heredoc end or \"${\" or \"{$\"".into(), self.line);
        }
        // Remove the newline before the closing label from content
        if content.last() == Some(&b'\n') {
            content.pop();
            if content.last() == Some(&b'\r') {
                content.pop();
            }
        } else if found_closing && content.last() == Some(&b'\r') {
            content.pop();
        }

        // PHP 7.3+ flexible heredoc: strip closing label's indentation from each
        // content line (strip_multiline_string_indentation).
        if !closing_indent.is_empty() {
            let indent_len = closing_indent.len();
            let using_spaces = closing_indent[0] == b' ';
            let closing_line = self.line;
            let mut stripped = Vec::new();
            // Lines with their terminators ("\n", "\r\n" or "\r").
            let mut lines: Vec<(&[u8], &[u8])> = Vec::new();
            if !content.is_empty() {
                let mut st = 0;
                let mut k = 0;
                while k < content.len() {
                    match content[k] {
                        b'\n' => {
                            lines.push((&content[st..k], &content[k..k + 1]));
                            k += 1;
                            st = k;
                        }
                        b'\r' => {
                            let nl = if content.get(k + 1) == Some(&b'\n') {
                                2
                            } else {
                                1
                            };
                            lines.push((&content[st..k], &content[k..k + nl]));
                            k += nl;
                            st = k;
                        }
                        _ => k += 1,
                    }
                }
                lines.push((&content[st..], &[]));
            }
            let first_line = closing_line.saturating_sub(lines.len() as u32);
            // Found while scanning the body, before its newlines are counted.
            if closing_indent.iter().any(|&c| c != closing_indent[0]) {
                crate::push_lexer_error(
                    "Invalid indentation - tabs and spaces cannot be mixed".into(),
                    first_line,
                );
            }
            for (i, &(line, newline)) in lines.iter().enumerate() {
                let body = line;
                let mut skip = 0;
                while skip < indent_len && skip < body.len() {
                    let c = body[skip];
                    if c != b' ' && c != b'\t' {
                        crate::push_lexer_error(
                            format!(
                                "Invalid body indentation level (expecting an indentation level of at least {})",
                                indent_len
                            ),
                            first_line + i as u32,
                        );
                        break;
                    }
                    if (c == b' ') != using_spaces {
                        crate::push_lexer_error(
                            "Invalid indentation - tabs and spaces cannot be mixed".into(),
                            first_line + i as u32,
                        );
                        break;
                    }
                    skip += 1;
                }
                // Whitespace-only lines need not be fully indented.
                stripped.extend_from_slice(&line[skip.min(line.len())..]);
                stripped.extend_from_slice(newline);
            }
            content = stripped;
        }

        if is_nowdoc {
            TokenKind::ConstantString(content)
        } else {
            self.process_heredoc_interpolation(content)
        }
    }

    /// Process heredoc content for variable interpolation and escape sequences,
    /// producing the same token pattern as double-quoted strings.
    fn process_heredoc_interpolation(&mut self, content: Vec<u8>) -> TokenKind {
        let len = content.len();
        let mut i = 0;
        let mut result = Vec::new();

        // Helper closure-like inline processing
        loop {
            if i >= len {
                break;
            }
            let ch = content[i];
            match ch {
                b'\\' => {
                    // Escape sequences (same as double-quoted strings)
                    i += 1;
                    if i >= len {
                        result.push(b'\\');
                        break;
                    }
                    match content[i] {
                        b'n' => {
                            result.push(b'\n');
                            i += 1;
                        }
                        b'r' => {
                            result.push(b'\r');
                            i += 1;
                        }
                        b't' => {
                            result.push(b'\t');
                            i += 1;
                        }
                        b'v' => {
                            result.push(0x0B);
                            i += 1;
                        }
                        b'e' => {
                            result.push(0x1B);
                            i += 1;
                        }
                        b'f' => {
                            result.push(0x0C);
                            i += 1;
                        }
                        b'\\' => {
                            result.push(b'\\');
                            i += 1;
                        }
                        b'$' => {
                            result.push(b'$');
                            i += 1;
                        }
                        b'u' if i + 1 < len && content[i + 1] == b'{' => {
                            // \u{HEX} unicode escape
                            let mut j = i + 2;
                            let mut hex_str = String::new();
                            let mut bad = false;
                            while j < len && content[j] != b'}' {
                                if !content[j].is_ascii_hexdigit() {
                                    bad = true;
                                    break;
                                }
                                hex_str.push(content[j] as char);
                                j += 1;
                            }
                            if j >= len {
                                bad = true;
                            }
                            encode_unicode_escape(&hex_str, bad, self.line, &mut result);
                            i = if bad { j } else { j + 1 };
                        }
                        b'x' | b'X' => {
                            let x_ch = content[i];
                            i += 1;
                            let mut hex = Vec::new();
                            for _ in 0..2 {
                                if i < len && content[i].is_ascii_hexdigit() {
                                    hex.push(content[i]);
                                    i += 1;
                                } else {
                                    break;
                                }
                            }
                            if !hex.is_empty() {
                                let s: String = hex.iter().map(|&b| b as char).collect();
                                result.push(u8::from_str_radix(&s, 16).unwrap_or(0));
                            } else {
                                // \x not followed by hex digits: keep literal \x
                                result.push(b'\\');
                                result.push(x_ch);
                            }
                        }
                        c if c.is_ascii_digit() && c < b'8' => {
                            let mut oct = vec![c];
                            i += 1;
                            for _ in 0..2 {
                                if i < len && content[i].is_ascii_digit() && content[i] < b'8' {
                                    oct.push(content[i]);
                                    i += 1;
                                } else {
                                    break;
                                }
                            }
                            let s: String = oct.iter().map(|&b| b as char).collect();
                            let v = u32::from_str_radix(&s, 8).unwrap_or(0);
                            if v > 0xFF {
                                crate::push_lexer_warning(
                                    format!(
                                        "Octal escape sequence overflow \\{} is greater than \\377",
                                        s
                                    ),
                                    self.line,
                                );
                            }
                            result.push((v & 0xFF) as u8);
                        }
                        other => {
                            result.push(b'\\');
                            result.push(other);
                            i += 1;
                        }
                    }
                }
                b'$' if i + 1 < len
                    && (content[i + 1].is_ascii_alphabetic()
                        || content[i + 1] == b'_'
                        || content[i + 1] >= 0x80) =>
                {
                    // Variable interpolation
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    i += 1; // skip $

                    // Scan variable name
                    let var_start = i;
                    while i < len
                        && (content[i].is_ascii_alphanumeric()
                            || content[i] == b'_'
                            || content[i] >= 0x80)
                    {
                        i += 1;
                    }
                    let var_name = content[var_start..i].to_vec();
                    self.pending.push(Token::new(
                        TokenKind::Variable(var_name),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));

                    // Check for ->property access
                    if i + 1 < len && content[i] == b'-' && content[i + 1] == b'>' {
                        i += 2; // skip ->
                        self.pending.push(Token::new(
                            TokenKind::Arrow,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        if i < len
                            && (content[i].is_ascii_alphabetic()
                                || content[i] == b'_'
                                || content[i] >= 0x80)
                        {
                            let prop_start = i;
                            while i < len
                                && (content[i].is_ascii_alphanumeric()
                                    || content[i] == b'_'
                                    || content[i] >= 0x80)
                            {
                                i += 1;
                            }
                            let prop_name = content[prop_start..i].to_vec();
                            self.pending.push(Token::new(
                                TokenKind::Identifier(prop_name),
                                Span::new(self.pos as u32, self.pos as u32, self.line),
                            ));
                        }
                    }
                    // "$obj?->name" (only before a label)
                    else if i + 3 < len
                        && &content[i..i + 3] == b"?->"
                        && (content[i + 3].is_ascii_alphabetic()
                            || content[i + 3] == b'_'
                            || content[i + 3] >= 0x80)
                    {
                        i += 3;
                        self.pending.push(Token::new(
                            TokenKind::NullsafeArrow,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        let prop_start = i;
                        while i < len
                            && (content[i].is_ascii_alphanumeric()
                                || content[i] == b'_'
                                || content[i] >= 0x80)
                        {
                            i += 1;
                        }
                        self.pending.push(Token::new(
                            TokenKind::Identifier(content[prop_start..i].to_vec()),
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                    }
                    // Check for [index] access
                    else if i < len && content[i] == b'[' {
                        i += 1; // skip [
                        self.pending.push(Token::new(
                            TokenKind::OpenBracket,
                            Span::new(self.pos as u32, self.pos as u32, self.line),
                        ));
                        // Scan index: could be number, variable, or bare identifier
                        if i < len {
                            match content[i] {
                                b'0'..=b'9' | b'-'
                                    if content[i] != b'-'
                                        || (i + 1 < len && content[i + 1].is_ascii_digit()) =>
                                {
                                    let neg = content[i] == b'-';
                                    if neg {
                                        i += 1;
                                    }
                                    let (num_kind, end) = var_offset_number(&content, i, neg);
                                    i = end;
                                    self.pending.push(Token::new(
                                        num_kind,
                                        Span::new(self.pos as u32, self.pos as u32, self.line),
                                    ));
                                }
                                b'$' => {
                                    i += 1; // skip $
                                    let idx_start = i;
                                    while i < len
                                        && (content[i].is_ascii_alphanumeric()
                                            || content[i] == b'_'
                                            || content[i] >= 0x80)
                                    {
                                        i += 1;
                                    }
                                    let idx_var = content[idx_start..i].to_vec();
                                    self.pending.push(Token::new(
                                        TokenKind::Variable(idx_var),
                                        Span::new(self.pos as u32, self.pos as u32, self.line),
                                    ));
                                }
                                c if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                                    let key_start = i;
                                    while i < len
                                        && (content[i].is_ascii_alphanumeric()
                                            || content[i] == b'_'
                                            || content[i] >= 0x80)
                                    {
                                        i += 1;
                                    }
                                    let key = content[key_start..i].to_vec();
                                    self.pending.push(Token::new(
                                        TokenKind::Identifier(key),
                                        Span::new(self.pos as u32, self.pos as u32, self.line),
                                    ));
                                }
                                _ => {}
                            }
                        }
                        if i < len && content[i] == b']' {
                            i += 1;
                            self.pending.push(Token::new(
                                TokenKind::CloseBracket,
                                Span::new(self.pos as u32, self.pos as u32, self.line),
                            ));
                        }
                    }
                }
                b'$' if i + 1 < len && content[i + 1] == b'{' => {
                    // ${var} / ${expr} interpolation (deprecated syntax)
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    let (toks, end, msg) = lex_dollar_brace_interpolation(&content, i, self.line);
                    crate::push_lexer_deprecated(msg.to_string(), self.line);
                    self.pending.extend(toks);
                    i = end;
                }
                b'{' if i + 2 < len
                    && content[i + 1] == b'$'
                    && (content[i + 2].is_ascii_alphabetic()
                        || content[i + 2] == b'_'
                        || content[i + 2] >= 0x80
                        || content[i + 2] == b'$'
                        || content[i + 2] == b'{') =>
                {
                    // {$expr} interpolation: the braced part is PHP code.
                    self.pending.push(Token::new(
                        TokenKind::InterpolatedStringPart(result.clone()),
                        Span::new(self.pos as u32, self.pos as u32, self.line),
                    ));
                    result.clear();
                    let (toks, end) = lex_braced_interpolation(&content, i, self.line);
                    self.pending.extend(toks);
                    i = end;
                }
                _ => {
                    result.push(ch);
                    i += 1;
                }
            }
        }

        // If we emitted interpolation tokens, this is the end part
        if !self.pending.is_empty() {
            self.pending.push(Token::new(
                TokenKind::InterpolatedStringEnd(result),
                Span::new(self.pos as u32, self.pos as u32, self.line),
            ));
            let first = self.pending.remove(0);
            return first.kind;
        }

        TokenKind::ConstantString(result)
    }

    /// Tokenize the entire source into a vector
    pub fn tokenize(&mut self) -> Vec<Token> {
        let _ = crate::take_lexer_error();
        let mut tokens = Vec::new();
        // `__halt_compiler();` at the outermost scope stops the lexer
        // (zend_stop_lexing()): what follows is data, not code.
        let mut depth: i32 = 0;
        let mut halt = 0u8;
        let mut prev_member = false;
        loop {
            let token = self.next_token();
            if token.kind == TokenKind::Eof {
                tokens.push(token);
                break;
            }
            let mut stop = false;
            match &token.kind {
                TokenKind::Whitespace(_)
                | TokenKind::LineComment(_)
                | TokenKind::BlockComment(_)
                | TokenKind::DocComment(_) => {}
                kind => {
                    halt = match kind {
                        TokenKind::Identifier(n)
                            if depth == 0
                                && !prev_member
                                && n.eq_ignore_ascii_case(b"__halt_compiler") =>
                        {
                            1
                        }
                        TokenKind::OpenParen if halt == 1 => 2,
                        TokenKind::CloseParen if halt == 2 => 3,
                        TokenKind::Semicolon | TokenKind::CloseTag if halt == 3 => {
                            stop = true;
                            0
                        }
                        _ => 0,
                    };
                    match kind {
                        TokenKind::OpenBrace => depth += 1,
                        TokenKind::CloseBrace => depth -= 1,
                        _ => {}
                    }
                    prev_member = matches!(
                        kind,
                        TokenKind::Arrow
                            | TokenKind::NullsafeArrow
                            | TokenKind::DoubleColon
                            | TokenKind::Function
                            | TokenKind::Backslash
                    );
                }
            }
            let span = token.span;
            tokens.push(token);
            if stop {
                tokens.push(Token::new(TokenKind::Eof, span));
                break;
            }
        }
        tokens
    }
}

/// `{$...}` inside a string (T_CURLY_OPEN): `src[start]` is the `{`. The
/// braced expression is lexed as PHP code; returns its tokens between
/// InterpExprStart / InterpExprEnd, and the index after the closing `}`.
/// A numeric `"$a[...]"` offset (ST_VAR_OFFSET's T_NUM_STRING) starting at
/// `p` (after any `-`): an integer only when written canonically
/// (`0|[1-9][0-9]*`) and in range, else the digits as a string;
/// `encaps_var_offset: '-' T_NUM_STRING` negates it (zend_negate_num_string:
/// `-0` and strings keep their spelling). Returns the token and end offset.
fn var_offset_number(s: &[u8], p: usize, negative: bool) -> (TokenKind, usize) {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    // [0-9]+("_"[0-9]+)* style runs
    let run = |mut i: usize, ok: &dyn Fn(u8) -> bool| -> Option<usize> {
        if !ok(at(i)) {
            return None;
        }
        while ok(at(i)) || (at(i) == b'_' && ok(at(i + 1))) {
            i += 1;
        }
        Some(i)
    };
    let dec = |c: u8| c.is_ascii_digit();
    let mut end = run(p, &dec).unwrap_or(p);
    for (pfx, ok) in [
        (
            b'x',
            &(|c: u8| c.is_ascii_hexdigit()) as &dyn Fn(u8) -> bool,
        ),
        (b'b', &|c: u8| c == b'0' || c == b'1'),
        (b'o', &|c: u8| (b'0'..=b'7').contains(&c)),
    ] {
        if at(p) == b'0' && at(p + 1).eq_ignore_ascii_case(&pfx) {
            if let Some(e) = run(p + 2, ok) {
                end = end.max(e);
            }
        }
    }
    let text = &s[p..end];
    // [0]|([1-9][0-9]*)
    let canonical_end = if at(p) == b'0' {
        p + 1
    } else {
        let mut i = p;
        while at(i).is_ascii_digit() {
            i += 1;
        }
        i
    };
    let long = if canonical_end == end {
        std::str::from_utf8(text)
            .ok()
            .and_then(|t| t.parse::<i64>().ok())
    } else {
        None
    };
    let kind = match long {
        Some(0) if negative => TokenKind::ConstantString(b"-0".to_vec()),
        Some(n) => TokenKind::LongNumber(if negative { -n } else { n }),
        None => {
            let mut v = Vec::with_capacity(text.len() + 1);
            if negative {
                v.push(b'-');
            }
            v.extend_from_slice(text);
            TokenKind::ConstantString(v)
        }
    };
    (kind, end)
}

fn is_label_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn lex_braced_interpolation(src: &[u8], start: usize, line: u32) -> (Vec<Token>, usize) {
    lex_interp_code(src, start, line, b"", b"")
}

/// `${...}` inside a string (T_DOLLAR_OPEN_CURLY_BRACES): `src[start]` is
/// the `$`.  `${name}` is the variable `$name`; `${name[...]}` an element
/// of it (both "${var}", T_STRING_VARNAME); anything else is a variable
/// variable `${expr}`.  Returns the tokens, the index after the closing
/// `}`, and the deprecation message.
fn lex_dollar_brace_interpolation(
    src: &[u8],
    start: usize,
    line: u32,
) -> (Vec<Token>, usize, &'static str) {
    let brace = start + 1;
    let mut j = brace + 1;
    if j < src.len() && (src[j].is_ascii_alphabetic() || src[j] == b'_' || src[j] >= 0x80) {
        while j < src.len() && (src[j].is_ascii_alphanumeric() || src[j] == b'_' || src[j] >= 0x80)
        {
            j += 1;
        }
        if j < src.len() && src[j] == b'}' {
            let tok = Token::new(
                TokenKind::Variable(src[brace + 1..j].to_vec()),
                Span::new(start as u32, start as u32, line),
            );
            return (vec![tok], j + 1, DEPRECATED_DOLLAR_BRACE_VAR);
        }
        if j < src.len() && src[j] == b'[' {
            let (toks, end) = lex_interp_code(src, brace, line, b"$", b"");
            return (toks, end, DEPRECATED_DOLLAR_BRACE_VAR);
        }
    }
    let (toks, end) = lex_interp_code(src, brace, line, b"${", b"}");
    (toks, end, DEPRECATED_DOLLAR_BRACE_EXPR)
}

const DEPRECATED_DOLLAR_BRACE_VAR: &str =
    "Using ${var} in strings is deprecated, use {$var} instead";
const DEPRECATED_DOLLAR_BRACE_EXPR: &str =
    "Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead";

/// The braced code starting at `src[start]` (a `{`), wrapped in `prefix` /
/// `suffix`, lexed as PHP code between InterpExprStart / InterpExprEnd.
fn lex_interp_code(
    src: &[u8],
    start: usize,
    line: u32,
    prefix: &[u8],
    suffix: &[u8],
) -> (Vec<Token>, usize) {
    let mut i = start + 1;
    let mut depth = 1usize;
    while i < src.len() {
        match src[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < src.len() && src[i] != q {
                    if src[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let inner_end = i.min(src.len());
    let mut code = b"<?php ".to_vec();
    code.extend_from_slice(prefix);
    code.extend_from_slice(&src[start + 1..inner_end]);
    code.extend_from_slice(suffix);
    // The sub-lexer must not lose (or own) the outer lexer's pending error.
    let outer_err = crate::take_lexer_error();
    let toks = Lexer::new(&code).tokenize();
    let inner_err = crate::take_lexer_error();
    if let Some((m, l)) = outer_err {
        crate::push_lexer_error(m, l);
    }
    if let Some((m, l)) = inner_err {
        crate::push_lexer_error(m, l + line - 1);
    }
    let mut out = vec![Token::new(
        TokenKind::InterpExprStart,
        Span::new(start as u32, start as u32, line),
    )];
    for mut t in toks {
        match t.kind {
            TokenKind::Eof => break,
            TokenKind::OpenTag => continue,
            _ => {
                t.span.line += line - 1;
                out.push(t);
            }
        }
    }
    let end_line = line
        + src[start..inner_end]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32;
    out.push(Token::new(
        TokenKind::InterpExprEnd,
        Span::new(inner_end as u32, inner_end as u32, end_line),
    ));
    (out, (inner_end + 1).min(src.len()))
}

/// Encode a `\u{...}` escape like zend_language_scanner.l: invalid or empty
/// sequences and codepoints above U+10FFFF are compile errors; surrogate
/// halves are encoded as plain 3-byte sequences.
fn encode_unicode_escape(hex: &str, bad: bool, line: u32, out: &mut Vec<u8>) {
    if bad || hex.is_empty() {
        crate::push_lexer_error("Invalid UTF-8 codepoint escape sequence".to_string(), line);
        return;
    }
    let cp = match u64::from_str_radix(hex, 16) {
        Ok(v) if v <= 0x10FFFF => v as u32,
        _ => {
            crate::push_lexer_error(
                "Invalid UTF-8 codepoint escape sequence: Codepoint too large".to_string(),
                line,
            );
            return;
        }
    };
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push(0xC0 | (cp >> 6) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x10000 {
        out.push(0xE0 | (cp >> 12) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else {
        out.push(0xF0 | (cp >> 18) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_world() {
        let src = b"<?php echo \"Hello, World!\\n\";";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();

        assert_eq!(tokens[0].kind, TokenKind::OpenTag);
        assert_eq!(tokens[1].kind, TokenKind::Echo);
        assert_eq!(
            tokens[2].kind,
            TokenKind::ConstantString(b"Hello, World!\n".to_vec())
        );
        assert_eq!(tokens[3].kind, TokenKind::Semicolon);
        assert_eq!(tokens[4].kind, TokenKind::Eof);
    }

    #[test]
    fn test_inline_html() {
        let src = b"Hello <?php echo 42; ?> World";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();

        assert_eq!(tokens[0].kind, TokenKind::InlineHtml(b"Hello ".to_vec()));
        assert_eq!(tokens[1].kind, TokenKind::OpenTag);
        assert_eq!(tokens[2].kind, TokenKind::Echo);
        assert_eq!(tokens[3].kind, TokenKind::LongNumber(42));
        assert_eq!(tokens[4].kind, TokenKind::Semicolon);
        assert_eq!(tokens[5].kind, TokenKind::CloseTag);
        assert_eq!(tokens[6].kind, TokenKind::InlineHtml(b" World".to_vec()));
    }

    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is test input, not PI
    fn test_variables_and_assignment() {
        let src = b"<?php $x = 10; $y = 3.14;";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();

        assert_eq!(tokens[1].kind, TokenKind::Variable(b"x".to_vec()));
        assert_eq!(tokens[2].kind, TokenKind::Assign);
        assert_eq!(tokens[3].kind, TokenKind::LongNumber(10));
        assert_eq!(tokens[5].kind, TokenKind::Variable(b"y".to_vec()));
        assert_eq!(tokens[7].kind, TokenKind::DoubleNumber(3.14));
    }

    #[test]
    fn test_operators() {
        let src = b"<?php $a + $b ** 2 === $c ?? $d;";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();

        assert_eq!(tokens[2].kind, TokenKind::Plus);
        assert_eq!(tokens[4].kind, TokenKind::Pow);
        assert_eq!(tokens[6].kind, TokenKind::Identical);
        assert_eq!(tokens[8].kind, TokenKind::NullCoalesce);
    }

    #[test]
    fn test_hex_binary_octal() {
        let src = b"<?php 0xFF; 0b1010; 0o77;";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();

        assert_eq!(tokens[1].kind, TokenKind::LongNumber(255));
        assert_eq!(tokens[3].kind, TokenKind::LongNumber(10));
        assert_eq!(tokens[5].kind, TokenKind::LongNumber(63));
    }
}
