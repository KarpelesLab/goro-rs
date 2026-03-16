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
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            mode_stack: vec![LexerMode::Initial],
            heredoc_label: Vec::new(),
            pending: Vec::new(),
        }
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
        while let Some(ch) = self.peek() {
            match ch {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.advance();
                }
                b'/' if self.peek_at(1) == Some(b'/') => {
                    // Line comment
                    self.advance();
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == b'\n' {
                            self.advance();
                            break;
                        }
                        // Also handle ?> inside line comments - it ends the comment AND PHP mode
                        if ch == b'?' && self.peek_at(1) == Some(b'>') {
                            break;
                        }
                        self.advance();
                    }
                }
                b'/' if self.peek_at(1) == Some(b'*') => {
                    // Block comment
                    self.advance();
                    self.advance();
                    loop {
                        match self.advance() {
                            Some(b'*') if self.peek() == Some(b'/') => {
                                self.advance();
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                }
                b'#' => {
                    // # comment (but not #[)
                    if self.peek_at(1) == Some(b'[') {
                        // PHP 8 attribute - skip #[...] entirely
                        self.advance(); // #
                        self.advance(); // [
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Some(b'[') => depth += 1,
                                Some(b']') => depth -= 1,
                                None => break,
                                _ => {}
                            }
                        }
                        continue;
                    }
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == b'\n' {
                            self.advance();
                            break;
                        }
                        if ch == b'?' && self.peek_at(1) == Some(b'>') {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
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

    fn scan_number(&mut self) -> TokenKind {
        let start = self.pos;
        let mut is_float = false;

        // Check for 0x, 0b, 0o prefixes
        if self.peek() == Some(b'0') {
            match self.peek_at(1) {
                Some(b'x' | b'X') => {
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
                    return TokenKind::LongNumber(i64::from_str_radix(&s[2..], 16).unwrap_or(0));
                }
                Some(b'b' | b'B') => {
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
                    return TokenKind::LongNumber(i64::from_str_radix(&s[2..], 2).unwrap_or(0));
                }
                Some(b'o' | b'O') => {
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
                    return TokenKind::LongNumber(i64::from_str_radix(&s[2..], 8).unwrap_or(0));
                }
                _ => {}
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
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
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
        // For now: simple approach - no interpolation, just escape sequences
        let mut result = Vec::new();
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    match self.advance() {
                        Some(b'n') => result.push(b'\n'),
                        Some(b'r') => result.push(b'\r'),
                        Some(b't') => result.push(b'\t'),
                        Some(b'v') => result.push(0x0B),
                        Some(b'e') => result.push(0x1B),
                        Some(b'f') => result.push(0x0C),
                        Some(b'\\') => result.push(b'\\'),
                        Some(b'$') => result.push(b'$'),
                        Some(b'"') => result.push(b'"'),
                        Some(b'x' | b'X') => {
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
                            result.push(u8::from_str_radix(&s, 8).unwrap_or(0));
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
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') =>
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
                            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                        {
                            let prop_name = self.scan_identifier();
                            self.pending.push(Token::new(
                                TokenKind::Identifier(prop_name),
                                Span::new(self.pos as u32, self.pos as u32, self.line),
                            ));
                        }
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
                            Some(b'0'..=b'9') => {
                                let num_kind = self.scan_number();
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
                            Some(b'\'') | Some(b'"') => {
                                // Skip for now - string key in interpolation
                            }
                            _ => {
                                // Bare identifier as key
                                if self
                                    .peek()
                                    .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                                {
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
            b"float" | b"double" | b"real" => TokenKind::FloatCast,
            b"string" | b"binary" => TokenKind::StringCast,
            b"bool" | b"boolean" => TokenKind::BoolCast,
            b"array" => TokenKind::ArrayCast,
            b"object" => TokenKind::ObjectCast,
            b"unset" => TokenKind::UnsetCast,
            _ => {
                self.pos = saved_pos;
                self.line = saved_line;
                return None;
            }
        };

        self.advance(); // consume )
        Some(kind)
    }

    pub fn next_token(&mut self) -> Token {
        // Return pending tokens first
        if let Some(token) = self.pending.first().cloned() {
            self.pending.remove(0);
            return token;
        }

        match self.mode() {
            LexerMode::Initial => self.scan_initial(),
            LexerMode::Scripting => self.scan_scripting(),
            LexerMode::DoubleQuote | LexerMode::Backtick | LexerMode::Heredoc => {
                // TODO: implement string interpolation scanning modes
                self.scan_scripting()
            }
        }
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

            if self.starts_with(b"<?php")
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
                self.push_mode(LexerMode::Scripting);
                return Token::new(
                    TokenKind::OpenTag,
                    Span::new(tag_start as u32, self.pos as u32, self.line),
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
            b'$' if self
                .peek_at(1)
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') =>
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

            b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xFF => {
                let ident = self.scan_identifier();

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
                                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != b'_')
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
                TokenKind::Backslash
            }

            _ => {
                // Unknown character - skip it and produce an identifier
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
        if self.peek() == Some(b'\r') {
            self.advance();
        }
        if self.peek() == Some(b'\n') {
            self.advance();
        }

        // Now scan until we find the closing label
        let mut content = Vec::new();
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
                if matches!(next_ch, None | Some(b';') | Some(b'\n') | Some(b'\r')) {
                    self.pos = after_label;
                    // Consume ; and newline if present
                    break;
                }
            }

            // Not the closing label, add the indent back and scan the line
            content.extend_from_slice(&indent);
            loop {
                match self.advance() {
                    Some(b'\n') => {
                        content.push(b'\n');
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

        // Remove trailing newline from content
        if content.last() == Some(&b'\n') {
            content.pop();
            if content.last() == Some(&b'\r') {
                content.pop();
            }
        }

        TokenKind::ConstantString(content)
    }

    /// Tokenize the entire source into a vector
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            if token.kind == TokenKind::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }
        tokens
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
