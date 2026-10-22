//! ICU's MessagePattern (ApostropheMode DOUBLE_OPTIONAL): the parsed form
//! of a MessageFormat pattern as a list of parts, as ICU4C builds it.

use crate::error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartType {
    MsgStart,
    MsgLimit,
    SkipSyntax,
    InsertChar,
    ReplaceNumber,
    ArgStart,
    ArgLimit,
    ArgNumber,
    ArgName,
    ArgType,
    ArgStyle,
    ArgSelector,
    ArgInt,
    ArgDouble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    None,
    Simple,
    Choice,
    Plural,
    Select,
    SelectOrdinal,
}

impl ArgType {
    pub fn has_plural_style(self) -> bool {
        matches!(self, ArgType::Plural | ArgType::SelectOrdinal)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub ty: PartType,
    /// Index in the pattern (code points).
    pub index: usize,
    pub length: usize,
    /// ARG_NUMBER: the number; ARG_INT: the value; MSG_*: nesting level.
    pub value: i32,
    pub arg_type: ArgType,
    /// For ARG_START / MSG_START: the index of the matching limit part.
    pub limit_part: usize,
}

impl Part {
    pub fn limit(&self) -> usize {
        self.index + self.length
    }
}

/// A syntax error: (UErrorCode, offset in code points).
#[derive(Clone, Debug)]
pub struct PatternError {
    pub code: i32,
    pub offset: usize,
}

#[derive(Clone, Debug, Default)]
pub struct MessagePattern {
    pub msg: Vec<char>,
    pub parts: Vec<Part>,
    pub numeric_values: Vec<f64>,
    pub has_arg_names: bool,
    pub has_arg_numbers: bool,
}

fn is_pattern_white_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{b}'
            | '\u{c}'
            | '\r'
            | ' '
            | '\u{85}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Pattern_Syntax
fn is_pattern_syntax(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x21..=0x2F | 0x3A..=0x40 | 0x5B..=0x5E | 0x60 | 0x7B..=0x7E
        | 0xA1..=0xA7 | 0xA9 | 0xAB | 0xAC | 0xAE | 0xB0 | 0xB1 | 0xB6 | 0xBB | 0xBF | 0xD7 | 0xF7
        | 0x2010..=0x2027 | 0x2030..=0x203E | 0x2041..=0x2053 | 0x2055..=0x205E
        | 0x2190..=0x245F | 0x2500..=0x2775 | 0x2794..=0x2BFF | 0x2E00..=0x2E7F
        | 0x3001..=0x3003 | 0x3008..=0x3020 | 0x3030 | 0xFD3E | 0xFD3F | 0xFE45 | 0xFE46)
}

const ARG_NAME_NOT_NUMBER: i32 = -1;
const ARG_NAME_NOT_VALID: i32 = -2;

impl MessagePattern {
    pub fn parse(pattern: &str) -> Result<MessagePattern, PatternError> {
        let mut mp = MessagePattern {
            msg: pattern.chars().collect(),
            ..Default::default()
        };
        mp.parse_message(0, 0, 0, ArgType::None)?;
        Ok(mp)
    }

    pub fn count(&self) -> usize {
        self.parts.len()
    }

    pub fn part(&self, i: usize) -> &Part {
        &self.parts[i]
    }

    pub fn substring(&self, p: &Part) -> String {
        self.msg[p.index..p.limit()].iter().collect()
    }

    /// The limit part index of the ARG_START / MSG_START at `i` (or `i`).
    pub fn limit_part_index(&self, i: usize) -> usize {
        let p = &self.parts[i];
        if matches!(p.ty, PartType::ArgStart | PartType::MsgStart) {
            p.limit_part
        } else {
            i
        }
    }

    pub fn numeric_value(&self, p: &Part) -> f64 {
        match p.ty {
            PartType::ArgInt => p.value as f64,
            PartType::ArgDouble => self.numeric_values[p.value as usize],
            _ => f64::NAN,
        }
    }

    /// The offset of a plural style starting at part `i` (the ARG_START).
    pub fn plural_offset(&self, i: usize) -> f64 {
        let p = &self.parts[i];
        if matches!(p.ty, PartType::ArgInt | PartType::ArgDouble) {
            self.numeric_value(p)
        } else {
            0.0
        }
    }

    pub fn utf16_offset(&self, i: usize) -> usize {
        self.msg[..i.min(self.msg.len())]
            .iter()
            .map(|c| c.len_utf16())
            .sum()
    }

    /// The pre- and post-context of an error offset (UParseError).
    pub fn error_context(&self, index: usize) -> (String, String) {
        let pre_start = index.saturating_sub(15);
        let pre: String = self.msg[pre_start..index.min(self.msg.len())]
            .iter()
            .collect();
        let end = (index + 15).min(self.msg.len());
        let post: String = self.msg[index.min(self.msg.len())..end].iter().collect();
        (pre, post)
    }

    fn add(&mut self, ty: PartType, index: usize, length: usize, value: i32) -> usize {
        self.parts.push(Part {
            ty,
            index,
            length,
            value,
            arg_type: ArgType::None,
            limit_part: 0,
        });
        self.parts.len() - 1
    }

    fn add_limit(&mut self, start: usize, ty: PartType, index: usize, length: usize, value: i32) {
        let i = self.add(ty, index, length, value);
        self.parts[start].limit_part = i;
    }

    fn err<T>(&self, code: i32, offset: usize) -> Result<T, PatternError> {
        Err(PatternError { code, offset })
    }

    fn char_at(&self, i: usize) -> char {
        self.msg.get(i).copied().unwrap_or('\0')
    }

    fn skip_white_space(&self, mut i: usize) -> usize {
        while i < self.msg.len() && is_pattern_white_space(self.msg[i]) {
            i += 1;
        }
        i
    }

    fn skip_identifier(&self, mut i: usize) -> usize {
        while i < self.msg.len()
            && !is_pattern_white_space(self.msg[i])
            && !is_pattern_syntax(self.msg[i])
        {
            i += 1;
        }
        i
    }

    fn skip_double(&self, mut i: usize) -> usize {
        while i < self.msg.len() {
            let c = self.msg[i];
            if (c < '0' && c != '+' && c != '-' && c != '.')
                || (c > '9' && c != 'e' && c != 'E' && c != '\u{221e}')
            {
                break;
            }
            i += 1;
        }
        i
    }

    fn parse_arg_number(&self, start: usize, limit: usize) -> i32 {
        if start >= limit {
            return ARG_NAME_NOT_VALID;
        }
        let mut s = start;
        let c = self.msg[s];
        s += 1;
        let mut number: i64;
        let mut bad;
        if c == '0' {
            if s == limit {
                return 0;
            }
            number = 0;
            bad = true;
        } else if ('1'..='9').contains(&c) {
            number = (c as u8 - b'0') as i64;
            bad = false;
        } else {
            return ARG_NAME_NOT_NUMBER;
        }
        while s < limit {
            let c = self.msg[s];
            s += 1;
            if c.is_ascii_digit() {
                if number >= (i32::MAX / 10) as i64 {
                    bad = true;
                }
                number = number * 10 + (c as u8 - b'0') as i64;
            } else {
                return ARG_NAME_NOT_NUMBER;
            }
        }
        if bad {
            ARG_NAME_NOT_VALID
        } else {
            number as i32
        }
    }

    fn parse_double(
        &mut self,
        start: usize,
        limit: usize,
        allow_infinity: bool,
    ) -> Result<(), PatternError> {
        let s: String = self.msg[start..limit].iter().collect();
        let (neg, body) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s.strip_prefix('+').unwrap_or(&s)),
        };
        if body == "\u{221e}" {
            if !allow_infinity {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
            }
            let v = if neg {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
            self.numeric_values.push(v);
            let idx = self.numeric_values.len() as i32 - 1;
            self.add(PartType::ArgDouble, start, limit - start, idx);
            return Ok(());
        }
        // a small integer
        if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) && body.len() <= 5 {
            let v: i32 = body.parse().unwrap_or(0);
            let v = if neg { -v } else { v };
            if v.abs() <= 32767 {
                self.add(PartType::ArgInt, start, limit - start, v);
                return Ok(());
            }
        }
        match s.parse::<f64>() {
            Ok(v) if !body.is_empty() && !body.starts_with(['+', '-']) => {
                self.numeric_values.push(v);
                let idx = self.numeric_values.len() as i32 - 1;
                self.add(PartType::ArgDouble, start, limit - start, idx);
                Ok(())
            }
            _ => self.err(error::U_PATTERN_SYNTAX_ERROR, start),
        }
    }

    fn in_message_format_pattern(&self, nesting: i32) -> bool {
        nesting > 0
            || self
                .parts
                .first()
                .is_some_and(|p| p.ty == PartType::MsgStart)
    }

    fn in_top_level_choice_message(&self, nesting: i32, parent: ArgType) -> bool {
        nesting == 1
            && parent == ArgType::Choice
            && self
                .parts
                .first()
                .is_none_or(|p| p.ty != PartType::MsgStart)
    }

    fn parse_message(
        &mut self,
        mut index: usize,
        start_len: usize,
        nesting: i32,
        parent: ArgType,
    ) -> Result<usize, PatternError> {
        let msg_start = self.add(PartType::MsgStart, index, start_len, nesting);
        index += start_len;
        loop {
            if index >= self.msg.len() {
                break;
            }
            let c = self.msg[index];
            index += 1;
            if c == '\'' {
                if index == self.msg.len() {
                    self.add(PartType::InsertChar, index, 0, '\'' as i32);
                } else {
                    let c = self.msg[index];
                    if c == '\'' {
                        self.add(PartType::SkipSyntax, index, 1, 0);
                        index += 1;
                    } else if c == '{'
                        || c == '}'
                        || (parent == ArgType::Choice && c == '|')
                        || (parent.has_plural_style() && c == '#')
                    {
                        self.add(PartType::SkipSyntax, index - 1, 1, 0);
                        loop {
                            let next = self.msg[index + 1..].iter().position(|&x| x == '\'');
                            match next {
                                Some(p) => {
                                    index = index + 1 + p;
                                    if self.char_at(index + 1) == '\'' {
                                        index += 1;
                                        self.add(PartType::SkipSyntax, index, 1, 0);
                                    } else {
                                        self.add(PartType::SkipSyntax, index, 1, 0);
                                        index += 1;
                                        break;
                                    }
                                }
                                None => {
                                    index = self.msg.len();
                                    self.add(PartType::InsertChar, index, 0, '\'' as i32);
                                    break;
                                }
                            }
                        }
                    } else {
                        self.add(PartType::InsertChar, index, 0, '\'' as i32);
                    }
                }
            } else if parent.has_plural_style() && c == '#' {
                self.add(PartType::ReplaceNumber, index - 1, 1, 0);
            } else if c == '{' {
                index = self.parse_arg(index - 1, 1, nesting)?;
            } else if (nesting > 0 && c == '}') || (parent == ArgType::Choice && c == '|') {
                let limit_len = if parent == ArgType::Choice && c == '}' {
                    0
                } else {
                    1
                };
                self.add_limit(msg_start, PartType::MsgLimit, index - 1, limit_len, nesting);
                if parent == ArgType::Choice {
                    return Ok(index - 1);
                }
                return Ok(index);
            }
        }
        if nesting > 0 && !self.in_top_level_choice_message(nesting, parent) {
            return self.err(error::U_UNMATCHED_BRACES, 0);
        }
        self.add_limit(msg_start, PartType::MsgLimit, index, 0, nesting);
        Ok(index)
    }

    fn parse_arg(
        &mut self,
        index: usize,
        start_len: usize,
        nesting: i32,
    ) -> Result<usize, PatternError> {
        let arg_start = self.add(PartType::ArgStart, index, start_len, 0);
        let mut arg_type = ArgType::None;
        let name_index = self.skip_white_space(index + start_len);
        let mut index = name_index;
        if index == self.msg.len() {
            return self.err(error::U_UNMATCHED_BRACES, 0);
        }
        index = self.skip_identifier(index);
        let number = self.parse_arg_number(name_index, index);
        if number >= 0 {
            self.has_arg_numbers = true;
            self.add(PartType::ArgNumber, name_index, index - name_index, number);
        } else if number == ARG_NAME_NOT_NUMBER {
            self.has_arg_names = true;
            self.add(PartType::ArgName, name_index, index - name_index, 0);
        } else {
            return self.err(error::U_PATTERN_SYNTAX_ERROR, name_index);
        }
        index = self.skip_white_space(index);
        if index == self.msg.len() {
            return self.err(error::U_UNMATCHED_BRACES, 0);
        }
        let mut c = self.msg[index];
        if c == '}' {
            // done
        } else if c != ',' {
            return self.err(error::U_PATTERN_SYNTAX_ERROR, name_index);
        } else {
            let type_index = self.skip_white_space(index + 1);
            index = type_index;
            while index < self.msg.len() && self.msg[index].is_ascii_alphabetic() {
                index += 1;
            }
            let length = index - type_index;
            index = self.skip_white_space(index);
            if index == self.msg.len() {
                return self.err(error::U_UNMATCHED_BRACES, 0);
            }
            c = self.msg[index];
            if length == 0 || (c != ',' && c != '}') {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, name_index);
            }
            let tname: String = self.msg[type_index..type_index + length]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            arg_type = match tname.as_str() {
                "choice" => ArgType::Choice,
                "plural" => ArgType::Plural,
                "select" => ArgType::Select,
                "selectordinal" => ArgType::SelectOrdinal,
                _ => ArgType::Simple,
            };
            self.parts[arg_start].arg_type = arg_type;
            if arg_type == ArgType::Simple {
                self.add(PartType::ArgType, type_index, length, 0);
            }
            if c == '}' {
                if arg_type != ArgType::Simple {
                    return self.err(error::U_PATTERN_SYNTAX_ERROR, name_index);
                }
            } else {
                index += 1;
                index = match arg_type {
                    ArgType::Simple => self.parse_simple_style(index)?,
                    ArgType::Choice => self.parse_choice_style(index, nesting)?,
                    _ => self.parse_plural_or_select_style(arg_type, index, nesting)?,
                };
            }
        }
        self.add_limit(arg_start, PartType::ArgLimit, index, 1, 0);
        let li = self.parts.len() - 1;
        self.parts[li].arg_type = arg_type;
        Ok(index + 1)
    }

    fn parse_simple_style(&mut self, mut index: usize) -> Result<usize, PatternError> {
        let start = index;
        let mut nested = 0;
        while index < self.msg.len() {
            let c = self.msg[index];
            index += 1;
            if c == '\'' {
                match self.msg[index..].iter().position(|&x| x == '\'') {
                    Some(p) => index = index + p + 1,
                    None => return self.err(error::U_PATTERN_SYNTAX_ERROR, start),
                }
            } else if c == '{' {
                nested += 1;
            } else if c == '}' {
                if nested > 0 {
                    nested -= 1;
                } else {
                    index -= 1;
                    self.add(PartType::ArgStyle, start, index - start, 0);
                    return Ok(index);
                }
            }
        }
        self.err(error::U_UNMATCHED_BRACES, 0)
    }

    fn parse_choice_style(&mut self, index: usize, nesting: i32) -> Result<usize, PatternError> {
        let start = index;
        let mut index = self.skip_white_space(index);
        if index == self.msg.len() || self.msg[index] == '}' {
            return self.err(error::U_PATTERN_SYNTAX_ERROR, 0);
        }
        loop {
            let number_index = index;
            index = self.skip_double(index);
            if index == number_index {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
            }
            self.parse_double(number_index, index, true)?;
            index = self.skip_white_space(index);
            if index == self.msg.len() {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
            }
            let c = self.msg[index];
            if !(c == '#' || c == '<' || c == '\u{2264}') {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
            }
            self.add(PartType::ArgSelector, index, 1, 0);
            index = self.parse_message(index + 1, 0, nesting + 1, ArgType::Choice)?;
            if index == self.msg.len() {
                return Ok(index);
            }
            if self.msg[index] == '}' {
                if !self.in_message_format_pattern(nesting) {
                    return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                }
                return Ok(index);
            }
            index = self.skip_white_space(index + 1);
        }
    }

    fn parse_plural_or_select_style(
        &mut self,
        arg_type: ArgType,
        index: usize,
        nesting: i32,
    ) -> Result<usize, PatternError> {
        let start = index;
        let mut index = index;
        let mut is_empty = true;
        let mut has_other = false;
        loop {
            index = self.skip_white_space(index);
            let eos = index == self.msg.len();
            if eos || self.msg[index] == '}' {
                if eos == self.in_message_format_pattern(nesting) {
                    return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                }
                if !has_other {
                    return self.err(error::U_DEFAULT_KEYWORD_MISSING, 0);
                }
                return Ok(index);
            }
            let selector_index = index;
            if arg_type.has_plural_style() && self.msg[selector_index] == '=' {
                index = self.skip_double(index + 1);
                let length = index - selector_index;
                if length == 1 {
                    return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                }
                self.add(PartType::ArgSelector, selector_index, length, 0);
                self.parse_double(selector_index + 1, index, false)?;
            } else {
                index = self.skip_identifier(index);
                let length = index - selector_index;
                if length == 0 {
                    return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                }
                let word: String = self.msg[selector_index..index].iter().collect();
                if arg_type.has_plural_style()
                    && length == 6
                    && index < self.msg.len()
                    && word == "offset"
                    && self.msg[index] == ':'
                {
                    if !is_empty {
                        return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                    }
                    let value_index = self.skip_white_space(index + 1);
                    index = self.skip_double(value_index);
                    if index == value_index {
                        return self.err(error::U_PATTERN_SYNTAX_ERROR, start);
                    }
                    self.parse_double(value_index, index, false)?;
                    is_empty = false;
                    continue;
                }
                self.add(PartType::ArgSelector, selector_index, length, 0);
                if word == "other" {
                    has_other = true;
                }
            }
            index = self.skip_white_space(index);
            if index == self.msg.len() || self.msg[index] != '{' {
                return self.err(error::U_PATTERN_SYNTAX_ERROR, selector_index);
            }
            index = self.parse_message(index, 1, nesting + 1, arg_type)?;
            is_empty = false;
        }
    }
}
