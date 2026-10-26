//! ext/intl/msgformat: MessageFormatter and the msgfmt_*() functions, over
//! an implementation of ICU4C's MessageFormat (MessagePattern parts,
//! cached argument formats, plural / selectordinal / select / choice
//! sub-messages, parsing) on the number formatters of formatter.rs and the
//! date formatting of IntlDateFormatter.

use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::ArrayKey;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::datecore::{self, CalInfo};
use crate::decfmt::Dq;
use crate::error::{self, IntlError};
use crate::formatter::{self, Kind, Nf};
use crate::msgdate;
use crate::msgpattern::{ArgType, MessagePattern, PartType, PatternError};
use crate::{Call, Obj, obj, s, sv};

crate::intl_functions! { pub FUNCTIONS:
    "msgfmt_create" => create,
    "msgfmt_format" => format,
    "msgfmt_format_message" => format_message,
    "msgfmt_parse" => parse,
    "msgfmt_parse_message" => parse_message,
    "msgfmt_set_pattern" => set_pattern,
    "msgfmt_get_pattern" => get_pattern,
    "msgfmt_get_locale" => get_locale,
    "msgfmt_get_error_code" => get_error_code,
    "msgfmt_get_error_message" => get_error_message,
}

pub const METHODS: &[(&str, crate::Imp)] = &[("MessageFormatter::__construct", construct)];

/// The functions that reset the global error before parsing their
/// arguments (msgfmt_ctor()'s intl_error_reset(NULL)).
pub const RESET_BEFORE_ZPP: &[&str] = &[
    "msgfmt_create",
    "MessageFormatter::create",
    "MessageFormatter::__construct",
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("MessageFormatter", Some(create_object))];

/// Formattable::Type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FType {
    Object,
    Str,
    Double,
    Long,
    Int64,
    Date,
}

/// A Formattable.
#[derive(Clone, Debug)]
pub enum Fv {
    Str(String),
    Double(f64),
    Long(i64),
    Int64(i64),
    /// UDate (milliseconds)
    Date(f64),
}

impl Fv {
    fn is_numeric(&self) -> bool {
        matches!(self, Fv::Double(_) | Fv::Long(_) | Fv::Int64(_))
    }
    fn as_f64(&self) -> f64 {
        match self {
            Fv::Double(d) => *d,
            Fv::Long(n) | Fv::Int64(n) => *n as f64,
            Fv::Date(d) => *d,
            Fv::Str(_) => 0.0,
        }
    }
    fn to_dq(&self) -> Dq {
        match self {
            Fv::Long(n) | Fv::Int64(n) => Dq::from_i64(*n),
            v => Dq::from_f64(v.as_f64()),
        }
    }
}

/// A cached argument format.
#[derive(Clone, Debug)]
enum Sub {
    Number(Box<Nf>),
    Date(String),
}

/// A MessageFormat.
#[derive(Clone, Debug)]
pub struct MsgFmt {
    /// The locale (umsg_getLocale()).
    pub locale: String,
    mp: MessagePattern,
    formats: HashMap<usize, Sub>,
    /// argTypes (by argument number).
    arg_types: Vec<FType>,
    default_nf: Box<Nf>,
}

/// An error of umsg_open(): the code and, for syntax errors, the position.
pub struct OpenError {
    pub code: i32,
    pub syntax: Option<PatternError>,
}

/// The locale name a MessageFormat reports (Locale::getName()).
fn locale_name(id: &str) -> String {
    let (base, kw) = match id.find('@') {
        Some(i) => (&id[..i], &id[i..]),
        None => (id, ""),
    };
    let mut parts: Vec<String> = Vec::new();
    for (i, p) in base.split(['_', '-']).enumerate() {
        if i == 0 {
            parts.push(p.to_ascii_lowercase());
        } else if i == 1 && p.len() == 4 {
            let mut t = p[..1].to_ascii_uppercase();
            t.push_str(&p[1..].to_ascii_lowercase());
            parts.push(t);
        } else {
            parts.push(p.to_ascii_uppercase());
        }
    }
    let mut s = parts.join("_");
    s.push_str(kw);
    s
}

fn find_keyword(s: &str, list: &[&str]) -> Option<usize> {
    let t = s.trim().to_lowercase();
    list.iter().position(|k| *k == t)
}

impl MsgFmt {
    /// umsg_open()
    pub fn open(vm: &Vm, locale: &str, pattern: &str) -> Result<MsgFmt, OpenError> {
        let mp = MessagePattern::parse(pattern).map_err(|e| OpenError {
            code: e.code,
            syntax: (e.code == error::U_PATTERN_SYNTAX_ERROR).then_some(e),
        })?;
        let default_nf = Nf::open(vm, locale, formatter::DECIMAL, None).map_err(|code| OpenError {
            code,
            syntax: None,
        })?;
        let mut mf = MsgFmt {
            locale: locale_name(locale),
            mp,
            formats: HashMap::new(),
            arg_types: Vec::new(),
            default_nf: Box::new(default_nf),
        };
        mf.cache_explicit_formats(vm, locale)
            .map_err(|code| OpenError { code, syntax: None })?;
        Ok(mf)
    }

    /// MessageFormat::cacheExplicitFormats()
    fn cache_explicit_formats(&mut self, vm: &Vm, locale: &str) -> Result<(), i32> {
        let n = self.mp.count();
        let limit = n.saturating_sub(2);
        let mut count = 0usize;
        for i in 2..limit {
            let p = self.mp.part(i);
            if p.ty == PartType::ArgNumber {
                count = count.max(p.value as usize + 1);
            }
        }
        self.arg_types = vec![FType::Object; count];
        let mut conflicts = false;
        let mut i = 1;
        while i < limit {
            let p = *self.mp.part(i);
            if p.ty != PartType::ArgStart {
                i += 1;
                continue;
            }
            let np = *self.mp.part(i + 1);
            let arg_number = (np.ty == PartType::ArgNumber).then_some(np.value as usize);
            let ft = match p.arg_type {
                ArgType::None => FType::Str,
                ArgType::Simple => {
                    let index = i;
                    let ty = self.mp.substring(self.mp.part(i + 2));
                    let mut style = String::new();
                    let mut j = i + 3;
                    if self.mp.part(j).ty == PartType::ArgStyle {
                        style = self.mp.substring(self.mp.part(j));
                        j += 1;
                    }
                    let (sub, ft) = self.create_format(vm, locale, &ty, &style)?;
                    self.formats.insert(index, sub);
                    i = j - 1;
                    ft
                }
                ArgType::Choice | ArgType::Plural | ArgType::SelectOrdinal => FType::Double,
                ArgType::Select => FType::Str,
            };
            if let Some(a) = arg_number {
                if self.arg_types[a] != FType::Object && self.arg_types[a] != ft {
                    conflicts = true;
                }
                self.arg_types[a] = ft;
            }
            i += 1;
        }
        // umsg_open(): hasArgTypeConflicts
        if conflicts {
            return Err(error::U_ARGUMENT_TYPE_MISMATCH);
        }
        Ok(())
    }

    /// MessageFormat::createAppropriateFormat()
    fn create_format(&self, vm: &Vm, locale: &str, ty: &str, style: &str) -> Result<(Sub, FType), i32> {
        let tid = find_keyword(ty, &["number", "date", "time", "spellout", "ordinal", "duration"]);
        match tid {
            Some(0) => {
                let sid = find_keyword(style, &["", "currency", "percent", "integer"]);
                let (nf, ft) = match sid {
                    Some(0) => (Nf::open(vm, locale, formatter::DECIMAL, None)?, FType::Double),
                    Some(1) => (Nf::open(vm, locale, formatter::CURRENCY, None)?, FType::Double),
                    Some(2) => (Nf::open(vm, locale, formatter::PERCENT, None)?, FType::Double),
                    Some(3) => {
                        let mut nf = Nf::open(vm, locale, formatter::DECIMAL, None)?;
                        if let Some(d) = nf.df_mut() {
                            d.set_attribute(6, 0);
                            d.props.decimal_always = false;
                            d.props.parse_int_only = true;
                        }
                        (nf, FType::Long)
                    }
                    _ => {
                        let st = style.trim_start_matches(|c: char| c.is_whitespace());
                        let mut nf = Nf::open(vm, locale, formatter::DECIMAL, None)?;
                        if let Some(sk) = st.strip_prefix("::") {
                            let data = nf.data.clone();
                            let df = nf.df_mut().unwrap();
                            let extra = crate::decfmt::apply_skeleton(df, &data, sk)?;
                            if let Some(long) = extra.compact {
                                let d = df.clone();
                                nf.kind = Kind::Compact(long, d);
                            }
                        } else if let Some(df) = nf.df_mut() {
                            crate::decfmt::apply_pattern(&mut df.props, style, 0)?;
                        }
                        (nf, FType::Double)
                    }
                };
                Ok((Sub::Number(Box::new(nf)), ft))
            }
            Some(1) | Some(2) => {
                let st = style.trim();
                let sid = find_keyword(st, &["", "short", "medium", "long", "full"]);
                let level = match sid {
                    Some(0) => 2,
                    Some(1) => 3,
                    Some(2) => 2,
                    Some(3) => 1,
                    Some(4) => 0,
                    _ => -2,
                };
                let sub = if level >= 0 {
                    let p = if tid == Some(1) {
                        msgdate::style_pattern(locale, level, -1)
                    } else {
                        msgdate::style_pattern(locale, -1, level)
                    };
                    Sub::Date(p)
                } else if let Some(sk) = st.strip_prefix("::") {
                    // (DateFormat::createInstanceForSkeleton(): the
                    // generator's best pattern)
                    Sub::Date(crate::datepatterngen::best_pattern(locale, sk))
                } else {
                    // (the pattern as written, white space included)
                    Sub::Date(style.to_string())
                };
                Ok((sub, FType::Date))
            }
            Some(t @ 3..=5) => {
                // makeRBNF(): the (trimmed) style names the default rule
                // set; one it is not leaves the default
                let style_k = [formatter::SPELLOUT, formatter::ORDINAL, formatter::DURATION][t - 3];
                let mut nf = Nf::open(vm, locale, style_k, None)?;
                if !style.trim().is_empty() {
                    nf.set_default_ruleset(style.trim());
                }
                Ok((Sub::Number(Box::new(nf)), FType::Double))
            }
            _ => Err(error::U_ILLEGAL_ARGUMENT_ERROR),
        }
    }

    pub fn uses_named_arguments(&self) -> bool {
        self.mp.has_arg_names
    }

    fn format_date(&self, vm: &mut Vm, sub: &Sub, ms: f64) -> String {
        let tz = datecore::default_tz_id(vm);
        let info = CalInfo::gregorian(ms, &tz, &self.locale);
        match sub {
            Sub::Date(p) => msgdate::format_date_pattern(vm, &self.locale, p, &info),
            Sub::Number(_) => String::new(),
        }
    }

    /// Format a Formattable with a cached format.
    fn format_with(&self, vm: &mut Vm, sub: &Sub, v: &Fv) -> Result<String, i32> {
        match sub {
            Sub::Number(nf) => match v {
                Fv::Double(d) => nf.format_f64(*d),
                Fv::Long(n) | Fv::Int64(n) => nf.format_i64(*n),
                _ => Err(error::U_ILLEGAL_ARGUMENT_ERROR),
            },
            _ => match v {
                Fv::Date(ms) | Fv::Double(ms) => Ok(self.format_date(vm, sub, *ms)),
                Fv::Long(n) | Fv::Int64(n) => Ok(self.format_date(vm, sub, *n as f64)),
                Fv::Str(_) => Err(error::U_ILLEGAL_ARGUMENT_ERROR),
            },
        }
    }

    fn default_format(&self, vm: &mut Vm, v: &Fv) -> Result<String, i32> {
        match v {
            Fv::Str(s) => Ok(s.clone()),
            Fv::Date(ms) => {
                let p = msgdate::style_pattern(&self.locale, 3, 3);
                Ok(self.format_date(vm, &Sub::Date(p), *ms))
            }
            Fv::Double(d) => self.default_nf.format_f64(*d),
            Fv::Long(n) | Fv::Int64(n) => self.default_nf.format_i64(*n),
        }
    }

    /// MessageFormat::format() with named arguments.
    pub fn format(&self, vm: &mut Vm, args: &[(String, Fv)]) -> Result<String, i32> {
        let mut out = String::new();
        self.format_part(vm, 0, None, args, &mut out)?;
        Ok(out)
    }

    fn arg_by_name<'a>(args: &'a [(String, Fv)], name: &str) -> Option<&'a Fv> {
        args.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    fn format_part(
        &self,
        vm: &mut Vm,
        msg_start: usize,
        pl: Option<&PluralCtx>,
        args: &[(String, Fv)],
        out: &mut String,
    ) -> Result<(), i32> {
        let mp = &self.mp;
        let mut prev = mp.part(msg_start).limit();
        let mut i = msg_start + 1;
        loop {
            let part = *mp.part(i);
            out.extend(mp.msg[prev..part.index].iter());
            if part.ty == PartType::MsgLimit {
                return Ok(());
            }
            prev = part.limit();
            if part.ty == PartType::ReplaceNumber {
                if let Some(ctx) = pl {
                    out.push_str(&ctx.number_string);
                }
                i += 1;
                continue;
            }
            if part.ty != PartType::ArgStart {
                i += 1;
                continue;
            }
            let arg_limit = mp.limit_part_index(i);
            let arg_type = part.arg_type;
            let name_part = *mp.part(i + 1);
            let arg_name = mp.substring(&name_part);
            let start_index = i;
            i += 2;
            match Self::arg_by_name(args, &arg_name) {
                None => {
                    out.push('{');
                    out.push_str(&arg_name);
                    out.push('}');
                }
                Some(arg) => {
                    if let Some(sub) = self.formats.get(&start_index) {
                        let s = self.format_with(vm, sub, arg)?;
                        out.push_str(&s);
                    } else if arg_type == ArgType::None {
                        let s = self.default_format(vm, arg)?;
                        out.push_str(&s);
                    } else if arg_type == ArgType::Choice {
                        if !arg.is_numeric() {
                            return Err(error::U_ILLEGAL_ARGUMENT_ERROR);
                        }
                        let sub = self.choice_find(i, arg.as_f64());
                        self.format_part(vm, sub, None, args, out)?;
                    } else if arg_type.has_plural_style() {
                        if !arg.is_numeric() {
                            return Err(error::U_ILLEGAL_ARGUMENT_ERROR);
                        }
                        let number = arg.as_f64();
                        let offset = mp.plural_offset(i);
                        let ctx = self.plural_context(start_index, &arg_name, arg, offset, arg_type);
                        let sub = self.plural_find(i, &ctx, number);
                        self.format_part(vm, sub, Some(&ctx), args, out)?;
                    } else if arg_type == ArgType::Select {
                        let s = match arg {
                            Fv::Str(s) => s.clone(),
                            _ => return Err(error::U_ILLEGAL_ARGUMENT_ERROR),
                        };
                        let sub = self.select_find(i, &s);
                        self.format_part(vm, sub, None, args, out)?;
                    } else {
                        return Err(error::U_INTERNAL_PROGRAM_ERROR);
                    }
                }
            }
            prev = mp.part(arg_limit).limit();
            i = arg_limit + 1;
        }
    }

    /// ChoiceFormat::findSubMessage()
    fn choice_find(&self, mut i: usize, number: f64) -> usize {
        let mp = &self.mp;
        let count = mp.count();
        i += 2;
        loop {
            let msg_start = i;
            i = mp.limit_part_index(i);
            i += 1;
            if i >= count {
                return msg_start;
            }
            let part = *mp.part(i);
            i += 1;
            if part.ty == PartType::ArgLimit {
                return msg_start;
            }
            let boundary = mp.numeric_value(&part);
            let sel = *mp.part(i);
            i += 1;
            let c = mp.msg[sel.index];
            // (the negated comparisons catch NaN)
            let stop = if c == '<' {
                number.partial_cmp(&boundary) != Some(std::cmp::Ordering::Greater)
            } else {
                !matches!(
                    number.partial_cmp(&boundary),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                )
            };
            if stop {
                return msg_start;
            }
        }
    }

    /// SelectFormat::findSubMessage()
    fn select_find(&self, mut i: usize, keyword: &str) -> usize {
        let mp = &self.mp;
        let count = mp.count();
        let mut msg_start = 0;
        loop {
            let part = *mp.part(i);
            i += 1;
            if part.ty == PartType::ArgLimit {
                break;
            }
            let sel = mp.substring(&part);
            if sel == keyword {
                return i;
            } else if msg_start == 0 && sel == "other" {
                msg_start = i;
            }
            i = mp.limit_part_index(i);
            i += 1;
            if i >= count {
                break;
            }
        }
        msg_start
    }

    fn plural_context(&self, start: usize, arg_name: &str, arg: &Fv, offset: f64, ty: ArgType) -> PluralCtx {
        let number = if offset == 0.0 {
            arg.clone()
        } else {
            Fv::Double(arg.as_f64() - offset)
        };
        // the formatter of the first {arg,number,...} in the "other" message
        let mut nf: &Nf = &self.default_nf;
        if let Some(other) = self.find_other_sub_message(start + 2) {
            if let Some(idx) = self.find_first_plural_number_arg(other, arg_name) {
                if let Some(Sub::Number(n)) = self.formats.get(&idx) {
                    nf = n;
                }
            }
        }
        let q = number.to_dq();
        let number_string = nf.format_dq(q.clone()).unwrap_or_default();
        let (rq, mfd) = match nf.df() {
            Some(d) => d.rounded(q),
            None => (q, 0),
        };
        let mut keyword_q = rq;
        keyword_q.neg = false;
        let plain = keyword_q.plain(mfd);
        PluralCtx {
            number_string,
            operands: intl::plural::PluralOperands::parse(&plain),
            ordinal: ty == ArgType::SelectOrdinal,
            lang: crate::numdata::language(&self.locale),
        }
    }

    fn find_other_sub_message(&self, mut i: usize) -> Option<usize> {
        let mp = &self.mp;
        let count = mp.count();
        let p = mp.part(i);
        if matches!(p.ty, PartType::ArgInt | PartType::ArgDouble) {
            i += 1;
        }
        loop {
            let part = *mp.part(i);
            i += 1;
            if part.ty == PartType::ArgLimit {
                return None;
            }
            if matches!(mp.part(i).ty, PartType::ArgInt | PartType::ArgDouble) {
                i += 1;
            }
            if mp.substring(&part) == "other" {
                return Some(i);
            }
            i = mp.limit_part_index(i);
            i += 1;
            if i >= count {
                return None;
            }
        }
    }

    fn find_first_plural_number_arg(&self, msg_start: usize, arg_name: &str) -> Option<usize> {
        let mp = &self.mp;
        let mut i = msg_start + 1;
        loop {
            let part = *mp.part(i);
            match part.ty {
                PartType::MsgLimit => return None,
                PartType::ReplaceNumber => return None,
                PartType::ArgStart => {
                    if part.arg_type == ArgType::Simple {
                        let np = mp.part(i + 1);
                        if mp.substring(np) == arg_name
                            && mp.substring(mp.part(i + 2)).trim().eq_ignore_ascii_case("number")
                        {
                            return Some(i);
                        }
                    }
                    i = mp.limit_part_index(i);
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// PluralFormat::findSubMessage()
    fn plural_find(&self, mut i: usize, ctx: &PluralCtx, n: f64) -> usize {
        let mp = &self.mp;
        let count = mp.count();
        let p = *mp.part(i);
        if matches!(p.ty, PartType::ArgInt | PartType::ArgDouble) {
            i += 1;
        }
        let mut keyword: Option<&'static str> = None;
        let mut have_match = false;
        let mut msg_start = 0;
        loop {
            let part = *mp.part(i);
            i += 1;
            if part.ty == PartType::ArgLimit {
                break;
            }
            if matches!(mp.part(i).ty, PartType::ArgInt | PartType::ArgDouble) {
                let vp = *mp.part(i);
                i += 1;
                if n == mp.numeric_value(&vp) {
                    return i;
                }
            } else if !have_match {
                let sel = mp.substring(&part);
                if sel == "other" {
                    if msg_start == 0 {
                        msg_start = i;
                        if keyword == Some("other") {
                            have_match = true;
                        }
                    }
                } else {
                    if keyword.is_none() {
                        let k = ctx.select();
                        keyword = Some(k);
                        if msg_start != 0 && k == "other" {
                            have_match = true;
                        }
                    }
                    if !have_match && Some(sel.as_str()) == keyword {
                        msg_start = i;
                        have_match = true;
                    }
                }
            }
            i = mp.limit_part_index(i);
            i += 1;
            if i >= count {
                break;
            }
        }
        msg_start
    }

    /// The type of every argument (umsg_get_types()): by number when the
    /// pattern has only numbered arguments (MessageFormat's argTypes), else
    /// by walking the parts as php-src does. Err((code, message)).
    fn php_types(&self) -> Result<(HashMap<u64, FType>, HashMap<String, FType>), (i32, &'static str)> {
        let mut nums: HashMap<u64, FType> = HashMap::new();
        let mut names: HashMap<String, FType> = HashMap::new();
        if !self.mp.has_arg_names {
            for (i, t) in self.arg_types.iter().enumerate() {
                nums.insert(i as u64, *t);
            }
            return Ok((nums, names));
        }
        let mp = &self.mp;
        let n = mp.count();
        let mut i = 0;
        while i + 2 < n {
            let p = *mp.part(i);
            if p.ty != PartType::ArgStart {
                i += 1;
                continue;
            }
            i += 1;
            let np = *mp.part(i);
            let (key_name, key_num) = match np.ty {
                PartType::ArgName => (Some(mp.substring(&np)), None),
                PartType::ArgNumber => (None, Some(np.value as u64)),
                _ => return Err((error::U_INVALID_FORMAT_ERROR, "Invalid part type encountered")),
            };
            let ty = match p.arg_type {
                ArgType::None => FType::Str,
                ArgType::Simple => {
                    i += 1;
                    let tp = *mp.part(i);
                    if tp.ty != PartType::ArgType {
                        return Err((
                            error::U_PARSE_ERROR,
                            "Expected UMSGPAT_PART_TYPE_ARG_TYPE part following UMSGPAT_ARG_TYPE_SIMPLE part",
                        ));
                    }
                    let ts = mp.substring(&tp);
                    match ts.as_str() {
                        "number" => {
                            let sp = *mp.part(i + 1);
                            if sp.ty == PartType::ArgStyle && mp.substring(&sp) == "integer" {
                                FType::Int64
                            } else {
                                FType::Double
                            }
                        }
                        "date" | "time" => FType::Date,
                        "spellout" | "ordinal" | "duration" => FType::Double,
                        _ => FType::Object,
                    }
                }
                ArgType::Plural | ArgType::Choice | ArgType::SelectOrdinal => FType::Double,
                ArgType::Select => FType::Str,
            };
            let stored = match (&key_name, key_num) {
                (Some(k), _) => names.entry(k.clone()).or_insert(FType::Object),
                (None, Some(k)) => nums.entry(k).or_insert(FType::Object),
                _ => unreachable!(),
            };
            if *stored != FType::Object && *stored != ty {
                return Err((error::U_ARGUMENT_TYPE_MISMATCH, "Inconsistent types declared for an argument"));
            }
            *stored = ty;
            i += 1;
        }
        Ok((nums, names))
    }

    /// MessageFormat::parse(): the arguments, or Err(UErrorCode).
    pub fn parse(&self, source: &str, tz: &str) -> Result<Vec<Fv>, i32> {
        if self.mp.has_arg_names {
            return Err(error::U_ARGUMENT_TYPE_MISMATCH);
        }
        let src: Vec<char> = source.chars().collect();
        let mut result: Vec<Option<Fv>> = vec![None; self.arg_types.len().max(1)];
        let mut count = 0usize;
        let mp = &self.mp;
        let mut prev = mp.part(0).limit();
        let mut off = 0usize;
        let mut i = 1;
        loop {
            let part = *mp.part(i);
            let len = part.index - prev;
            if len == 0 || src.get(off..off + len) == Some(&mp.msg[prev..part.index]) {
                off += len;
            } else {
                return Err(error::U_MESSAGE_PARSE_ERROR);
            }
            if part.ty == PartType::MsgLimit {
                break;
            }
            if matches!(part.ty, PartType::SkipSyntax | PartType::InsertChar) {
                prev = part.limit();
                i += 1;
                continue;
            }
            let arg_limit = mp.limit_part_index(i);
            let arg_type = part.arg_type;
            let np = *mp.part(i + 1);
            let arg_number = np.value.max(0) as usize;
            let start_index = i;
            if result.len() <= arg_number {
                result.resize(arg_number + 1, None);
            }
            let mut have = false;
            if let Some(sub) = self.formats.get(&start_index) {
                match sub {
                    Sub::Number(nf) => match nf.parse(&src, off) {
                        Ok((q, end)) if end > off => {
                            result[arg_number] = Some(dq_to_fv(&q));
                            off = end;
                            have = true;
                        }
                        _ => return Err(error::U_MESSAGE_PARSE_ERROR),
                    },
                    Sub::Date(p) => match msgdate::parse_date_pattern(p, &src, off, tz) {
                        Some((ms, end)) if end > off => {
                            result[arg_number] = Some(Fv::Date(ms));
                            off = end;
                            have = true;
                        }
                        _ => return Err(error::U_MESSAGE_PARSE_ERROR),
                    },
                }
            } else if arg_type == ArgType::None {
                let after = self.literal_until_next_argument(arg_limit);
                let next = if after.is_empty() {
                    Some(src.len())
                } else {
                    let a: Vec<char> = after.chars().collect();
                    (off..=src.len().saturating_sub(a.len())).find(|&k| src[k..].starts_with(&a))
                };
                let Some(next) = next else {
                    return Err(error::U_MESSAGE_PARSE_ERROR);
                };
                let val: String = src[off..next].iter().collect();
                if val != format!("{{{}}}", arg_number) {
                    result[arg_number] = Some(Fv::Str(val));
                    have = true;
                }
                off = next;
            } else if arg_type == ArgType::Choice {
                match self.choice_parse(i + 2, &src, off) {
                    Some((v, end)) => {
                        result[arg_number] = Some(Fv::Double(v));
                        off = end;
                        have = true;
                    }
                    None => return Err(error::U_MESSAGE_PARSE_ERROR),
                }
            } else {
                // (U_UNSUPPORTED_ERROR, which the index 0 turns into this)
                return Err(error::U_MESSAGE_PARSE_ERROR);
            }
            if have && count <= arg_number {
                count = arg_number + 1;
            }
            prev = mp.part(arg_limit).limit();
            i = arg_limit + 1;
        }
        if off == 0 && !src.is_empty() || off == 0 && mp.count() > 2 {
            return Err(error::U_MESSAGE_PARSE_ERROR);
        }
        Ok(result
            .into_iter()
            .take(count)
            .map(|v| v.unwrap_or(Fv::Long(0)))
            .collect())
    }

    fn literal_until_next_argument(&self, from: usize) -> String {
        let mp = &self.mp;
        let mut out = String::new();
        let mut prev = mp.part(from).limit();
        let mut i = from + 1;
        loop {
            let part = *mp.part(i);
            out.extend(mp.msg[prev..part.index].iter());
            if matches!(part.ty, PartType::ArgStart | PartType::MsgLimit) {
                return out;
            }
            prev = part.limit();
            i += 1;
        }
    }

    /// ChoiceFormat::parseArgument()
    fn choice_parse(&self, mut i: usize, src: &[char], off: usize) -> Option<(f64, usize)> {
        let mp = &self.mp;
        let mut best: Option<(f64, usize)> = None;
        while i < mp.count() {
            let part = *mp.part(i);
            if part.ty == PartType::ArgLimit {
                break;
            }
            let v = mp.numeric_value(&part);
            i += 2;
            let lim = mp.limit_part_index(i);
            // the sub-message text (without syntax)
            let mut text = String::new();
            let mut prev = mp.part(i).limit();
            let mut k = i + 1;
            while k <= lim {
                let p = *mp.part(k);
                text.extend(mp.msg[prev..p.index].iter());
                prev = p.limit();
                k += 1;
            }
            let t: Vec<char> = text.chars().collect();
            if !t.is_empty() && src[off..].starts_with(&t) {
                let end = off + t.len();
                if best.is_none_or(|b| end > b.1) {
                    best = Some((v, end));
                }
            }
            i = lim + 1;
        }
        best
    }
}

/// A Formattable from a parsed number.
fn dq_to_fv(q: &Dq) -> Fv {
    if let Some(n) = q.to_i64() {
        if q.neg && q.is_zero() {
            return Fv::Double(-0.0);
        }
        if (i32::MIN as i64..=i32::MAX as i64).contains(&n) {
            return Fv::Long(n);
        }
        return Fv::Int64(n);
    }
    Fv::Double(q.to_f64())
}

struct PluralCtx {
    number_string: String,
    operands: Option<intl::plural::PluralOperands>,
    ordinal: bool,
    /// The language the plural rules are those of.
    lang: String,
}

impl PluralCtx {
    fn select(&self) -> &'static str {
        let Some(ops) = self.operands else {
            return "other";
        };
        let cat = if self.ordinal {
            intl::plural::ordinal_category(&self.lang, &ops)
        } else {
            intl::plural::plural_category(&self.lang, &ops)
        };
        use intl::plural::PluralCategory as C;
        match cat {
            C::Zero => "zero",
            C::One => "one",
            C::Two => "two",
            C::Few => "few",
            C::Many => "many",
            C::Other => "other",
        }
    }
}

// ---------------------------------------------------------------------------
// The object

#[derive(Clone, Debug, Default)]
pub struct MfState {
    pub mf: Option<MsgFmt>,
    pub orig: Option<Vec<u8>>,
    pub err: IntlError,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_object),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, MfState::default(), &HANDLERS);
}

fn clone_object(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let Some(st) = obj::state::<MfState>(src) else {
        return;
    };
    let constructed = st.borrow().mf.is_some();
    obj::clone_state::<MfState>(src, dst, |s| s.clone());
    if !constructed {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized MessageFormatter");
    }
}

type St = Rc<std::cell::RefCell<MfState>>;

fn state(o: &Obj) -> Option<St> {
    obj::state::<MfState>(o)
}

fn fetch(vm: &mut Vm, c: &Call) -> Option<St> {
    let st = state(&c.obj())?;
    st.borrow_mut().err.reset();
    if st.borrow().mf.is_none() {
        crate::zpp::throw(vm, b"Error", "Found unconstructed MessageFormatter");
        return None;
    }
    Some(st)
}

/// intl_errors_set(): the object's error and the global one.
fn errors_set(vm: &mut Vm, c: &Call, st: &St, code: i32, msg: &str) {
    let mut e = st.borrow().err.clone();
    error::set_both(vm, &c.name, &mut e, code, msg);
    st.borrow_mut().err = e;
}

/// intl_errors_set() with a message quoting raw (not UTF-8) bytes: the
/// IntlException gets the bytes as they are.
fn errors_set_raw(vm: &mut Vm, c: &Call, st: &St, code: i32, pre: &str, raw: &[u8]) {
    let had = vm.current_exception.is_some();
    let lossy = format!("{}{}'", pre, String::from_utf8_lossy(raw));
    errors_set(vm, c, st, code, &lossy);
    if had {
        return;
    }
    if let Some(Value::Object(e)) = &vm.current_exception {
        let mut m = format!("{}(): {}", c.name, pre).into_bytes();
        m.extend_from_slice(raw);
        m.push(b'\'');
        e.borrow_mut().set_property(b"message".to_vec(), sv(m));
    }
}

/// INTL_METHOD_CHECK_STATUS
fn check_status(vm: &mut Vm, c: &Call, st: &St, msg: &str) -> bool {
    let code = st.borrow().err.code;
    error::set_global_code(code);
    if error::failure(code) {
        errors_set(vm, c, st, code, msg);
        return true;
    }
    false
}

fn syntax_message(mp_src: &str, e: &PatternError) -> String {
    let mp = MessagePattern {
        msg: mp_src.chars().collect(),
        ..Default::default()
    };
    let (pre, post) = mp.error_context(e.offset);
    let pe = error::parse_error_string(-1, mp.utf16_offset(e.offset) as i32, &pre, &post);
    format!("pattern syntax error ({})", pe)
}

/// msgfmt_ctor()
fn ctor(vm: &mut Vm, c: &mut Call, o: &Obj) -> bool {
    error::reset_global();
    let locale = c.str(0);
    let pattern = c.str(1);
    if locale.len() > crate::common::MAX_LOCALE_LEN {
        let msg = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
        return false;
    }
    let Some(st) = state(o) else {
        return false;
    };
    st.borrow_mut().err.reset();
    let pat = match std::str::from_utf8(&pattern) {
        Ok(p) => p.to_string(),
        Err(_) => {
            st.borrow_mut().err.code = error::U_INVALID_CHAR_FOUND;
            check_status(vm, c, &st, "error converting pattern to UTF-16");
            return false;
        }
    };
    let locale = crate::common::locale_or_default(vm, &locale);
    st.borrow_mut().orig = Some(pattern.clone());
    if pat.is_empty() {
        st.borrow_mut().err.code = error::U_ILLEGAL_ARGUMENT_ERROR;
        check_status(vm, c, &st, "message formatter creation failed");
        return false;
    }
    match MsgFmt::open(vm, &locale, &pat) {
        Ok(mf) => {
            st.borrow_mut().mf = Some(mf);
            error::set_global_code(error::U_ZERO_ERROR);
            true
        }
        Err(e) => {
            st.borrow_mut().err.code = e.code;
            if let Some(se) = &e.syntax {
                let msg = syntax_message(&pat, se);
                error::set_global_code(e.code);
                errors_set(vm, c, &st, e.code, &msg);
            } else {
                check_status(vm, c, &st, "message formatter creation failed");
            }
            false
        }
    }
}

/// msgfmt_create() / MessageFormatter::create()
fn create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = obj::object_init_ex(vm, b"messageformatter") else {
        return Value::Null;
    };
    if ctor(vm, c, &o) {
        Value::Object(o)
    } else {
        Value::Null
    }
}

/// MessageFormatter::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    formatter::with_ctor_errors(vm, |vm| ctor(vm, c, &o));
    Value::Null
}

/// intl_zval_to_millis(): Err(message) on failure.
fn zval_to_millis(vm: &mut Vm, v: &Value) -> Result<f64, String> {
    match v {
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Double(d), _)) => Ok(d * 1000.0),
            Some((goro_core::value::NumStr::Long(n), _)) => Ok(n as f64 * 1000.0),
            None => Err(format!(
                "string '{}' is not numeric, which would be required for it to be a valid date",
                s.to_string_lossy()
            )),
        },
        Value::Long(n) => Ok(*n as f64 * 1000.0),
        Value::Double(d) => Ok(d * 1000.0),
        Value::Object(o) => {
            if crate::zpp::obj_instanceof(vm, o, "DateTimeInterface") {
                let r = vm.call_object_method(v, b"format", &[s(b"U.u")]);
                match r {
                    Some(Value::String(t)) => {
                        let t = t.to_string_lossy();
                        let (sec, us) = t.split_once('.').unwrap_or((&t, "0"));
                        let sec: f64 = sec.parse().unwrap_or(0.0);
                        let us: f64 = us.parse().unwrap_or(0.0);
                        let neg = t.starts_with('-');
                        let ms = sec * 1000.0 + if neg { -us } else { us } / 1000.0;
                        Ok(ms.trunc())
                    }
                    _ => Err("call to DateTime::format() failed".to_string()),
                }
            } else if crate::zpp::obj_instanceof(vm, o, "IntlCalendar") {
                match datecore::cal_object_info(o) {
                    Some(info) => Ok(info.millis),
                    None => Err("IntlCalendar object is not properly constructed".to_string()),
                }
            } else {
                Err("invalid object type for date/time (only IntlCalendar and DateTimeInterface permitted)".to_string())
            }
        }
        _ => Err("invalid PHP type for date".to_string()),
    }
}

/// zval_get_string() with __toString(); None if it threw.
fn zval_string(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    let s = vm.value_to_string_checked(v).ok()?;
    if vm.current_exception.is_some() {
        return None;
    }
    Some(s.as_bytes().to_vec())
}

/// umsg_format_helper(): the formatted message, or None (error set).
fn format_helper(vm: &mut Vm, c: &Call, st: &St, args: &Value) -> Option<String> {
    let Value::Array(arr) = args.deref() else {
        return None;
    };
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mf = st.borrow().mf.clone()?;
    let (nums, names) = match mf.php_types() {
        Ok(t) => t,
        Err((code, msg)) => {
            errors_set(vm, c, st, code, msg);
            return None;
        }
    };
    let mut fargs: Vec<(String, Fv)> = Vec::new();
    for (k, v) in entries {
        if error::failure(st.borrow().err.code) {
            break;
        }
        let v = v.deref();
        let (key, ty) = match &k {
            ArrayKey::Int(n) => {
                if *n < 0 || *n > i32::MAX as i64 {
                    errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, "Found negative or too large array key");
                    continue;
                }
                (n.to_string(), nums.get(&(*n as u64)).copied())
            }
            ArrayKey::String(s) => match std::str::from_utf8(s.as_bytes()) {
                Ok(t) => (t.to_string(), names.get(t).copied()),
                Err(_) => {
                    errors_set_raw(
                        vm,
                        c,
                        st,
                        error::U_INVALID_CHAR_FOUND,
                        "Invalid UTF-8 data in argument key: '",
                        s.as_bytes(),
                    );
                    continue;
                }
            },
        };
        let ty = ty.unwrap_or(FType::Object);
        let fv = match ty {
            FType::Str => match string_arg(vm, c, st, &v) {
                Some(f) => f,
                None => continue,
            },
            FType::Double => Fv::Double(v.to_double()),
            FType::Long => {
                let n = match &v {
                    Value::Double(d) => {
                        if *d > i32::MAX as f64 || *d < i32::MIN as f64 {
                            errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, "Found PHP float with absolute value too large for 32 bit integer argument");
                            0
                        } else {
                            *d as i64
                        }
                    }
                    Value::Long(n) => {
                        if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                            errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, "Found PHP integer with absolute value too large for 32 bit integer argument");
                            0
                        } else {
                            *n
                        }
                    }
                    v => v.to_long() as i32 as i64,
                };
                Fv::Long(n)
            }
            FType::Int64 => {
                let n = match &v {
                    Value::Double(d) => {
                        if *d > i64::MAX as f64 || *d < i64::MIN as f64 {
                            errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, "Found PHP float with absolute value too large for 64 bit integer argument");
                            0
                        } else {
                            *d as i64
                        }
                    }
                    v => v.to_long(),
                };
                Fv::Int64(n)
            }
            FType::Date => match zval_to_millis(vm, &v) {
                Ok(ms) => Fv::Date(ms),
                Err(_) => {
                    let msg = format!("The argument for key '{}' cannot be used as a date or time", key);
                    errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
                    continue;
                }
            },
            FType::Object => match &v {
                Value::Double(d) => Fv::Double(*d),
                Value::Long(n) => Fv::Int64(*n),
                Value::Null | Value::Undef | Value::False => Fv::Int64(0),
                Value::True => Fv::Int64(1),
                Value::String(_) | Value::Object(_) => match string_arg(vm, c, st, &v) {
                    Some(f) => f,
                    None => continue,
                },
                _ => {
                    let msg = format!(
                        "No strategy to convert the value given for the argument with key '{}' is available",
                        key
                    );
                    errors_set(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
                    Fv::Int64(0)
                }
            },
        };
        fargs.push((key, fv));
    }
    if error::failure(st.borrow().err.code) || vm.current_exception.is_some() && !error::use_exceptions(vm) {
        return None;
    }
    if error::failure(st.borrow().err.code) {
        return None;
    }
    match mf.format(vm, &fargs) {
        Ok(s) => Some(s),
        Err(code) => {
            errors_set(vm, c, st, code, "Call to ICU MessageFormat::format() has failed");
            None
        }
    }
}

/// A string argument (kString): None when an error was set.
fn string_arg(vm: &mut Vm, c: &Call, st: &St, v: &Value) -> Option<Fv> {
    let bytes = zval_string(vm, v)?;
    match String::from_utf8(bytes) {
        Ok(t) => Some(Fv::Str(t)),
        Err(e) => {
            let raw = e.into_bytes();
            errors_set_raw(
                vm,
                c,
                st,
                error::U_INVALID_CHAR_FOUND,
                "Invalid UTF-8 data in string argument: '",
                &raw,
            );
            None
        }
    }
}

/// msgfmt_format()
fn format(vm: &mut Vm, c: &mut Call) -> Value {
    let args = c.arg(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match format_helper(vm, c, &st, &args) {
        Some(s) => {
            error::set_global_code(error::U_ZERO_ERROR);
            sv(s.into_bytes())
        }
        None => Value::False,
    }
}

/// msgfmt_format_message() / MessageFormatter::formatMessage()
fn format_message(vm: &mut Vm, c: &mut Call) -> Value {
    let locale = c.str(0);
    let pattern = c.str(1);
    let args = c.arg(2);
    if locale.len() > crate::common::MAX_LOCALE_LEN {
        let msg = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
        return Value::Null;
    }
    let Ok(pat) = std::str::from_utf8(&pattern) else {
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, "error converting pattern to UTF-16");
        return Value::False;
    };
    let locale = crate::common::locale_or_default(vm, &locale);
    let r = if pat.is_empty() {
        Err(OpenError {
            code: error::U_ILLEGAL_ARGUMENT_ERROR,
            syntax: None,
        })
    } else {
        MsgFmt::open(vm, &locale, pat)
    };
    let mf = match r {
        Ok(mf) => mf,
        Err(e) => {
            if let Some(se) = &e.syntax {
                let msg = syntax_message(pat, se);
                error::set_global(vm, &c.name, e.code, &msg);
            } else {
                let code = error::global_code();
                error::set_global(vm, &c.name, code, "Creating message formatter failed");
            }
            return Value::False;
        }
    };
    let st: St = Rc::new(std::cell::RefCell::new(MfState {
        mf: Some(mf),
        orig: None,
        err: IntlError::default(),
    }));
    match format_helper(vm, c, &st, &args) {
        Some(s) => {
            error::set_global_code(error::U_ZERO_ERROR);
            sv(s.into_bytes())
        }
        None => Value::False,
    }
}

fn fv_to_value(v: &Fv) -> Value {
    match v {
        Fv::Date(ms) => Value::Double(ms / 1000.0),
        Fv::Double(d) => Value::Double(*d),
        Fv::Long(n) => Value::Long(*n),
        Fv::Int64(n) => Value::Long(*n),
        Fv::Str(s) => crate::s(s.as_bytes()),
    }
}

/// msgfmt_do_parse()
fn do_parse(vm: &mut Vm, c: &Call, st: &St, source: &[u8]) -> Value {
    let Ok(src) = std::str::from_utf8(source) else {
        st.borrow_mut().err.code = error::U_INVALID_CHAR_FOUND;
        check_status(vm, c, st, "Converting parse string failed");
        return Value::False;
    };
    let tz = datecore::default_tz_id(vm);
    let r = st.borrow().mf.as_ref().unwrap().parse(src, &tz);
    match r {
        Ok(vals) => {
            error::set_global_code(error::U_ZERO_ERROR);
            let mut a = goro_core::array::PhpArray::new();
            for v in &vals {
                a.push(fv_to_value(v));
            }
            crate::arr(a)
        }
        Err(code) => {
            st.borrow_mut().err.code = code;
            check_status(vm, c, st, "Parsing failed");
            Value::False
        }
    }
}

/// msgfmt_parse()
fn parse(vm: &mut Vm, c: &mut Call) -> Value {
    let source = c.str(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    do_parse(vm, c, &st, &source)
}

/// msgfmt_parse_message() / MessageFormatter::parseMessage()
fn parse_message(vm: &mut Vm, c: &mut Call) -> Value {
    let locale = c.str(0);
    let pattern = c.str(1);
    let source = c.str(2);
    if locale.len() > crate::common::MAX_LOCALE_LEN {
        let msg = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
        return Value::Null;
    }
    let Ok(pat) = std::str::from_utf8(&pattern) else {
        error::set_global(vm, &c.name, error::U_ILLEGAL_ARGUMENT_ERROR, "error converting pattern to UTF-16");
        return Value::False;
    };
    let locale = crate::common::locale_or_default(vm, &locale);
    let st: St = Rc::new(std::cell::RefCell::new(MfState::default()));
    let r = if pat.is_empty() {
        Err(OpenError {
            code: error::U_ILLEGAL_ARGUMENT_ERROR,
            syntax: None,
        })
    } else {
        MsgFmt::open(vm, &locale, pat)
    };
    match r {
        Ok(mf) => st.borrow_mut().mf = Some(mf),
        Err(e) => {
            st.borrow_mut().err.code = e.code;
            check_status(vm, c, &st, "Creating message formatter failed");
            return Value::False;
        }
    }
    do_parse(vm, c, &st, &source)
}

/// msgfmt_set_pattern()
fn set_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let pattern = c.str(0);
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Ok(pat) = std::str::from_utf8(&pattern) else {
        st.borrow_mut().err.code = error::U_INVALID_CHAR_FOUND;
        check_status(vm, c, &st, "Error converting pattern to UTF-16");
        return Value::False;
    };
    let locale = st.borrow().mf.as_ref().unwrap().locale.clone();
    let r = if pat.is_empty() {
        Err(OpenError {
            code: error::U_ILLEGAL_ARGUMENT_ERROR,
            syntax: None,
        })
    } else {
        MsgFmt::open(vm, &locale, pat)
    };
    match r {
        Ok(mf) => {
            let mut b = st.borrow_mut();
            b.mf = Some(mf);
            b.orig = Some(pattern.clone());
            Value::True
        }
        Err(e) => {
            st.borrow_mut().err.code = e.code;
            let offset = e
                .syntax
                .as_ref()
                .map(|se| {
                    let mp = MessagePattern {
                        msg: pat.chars().collect(),
                        ..Default::default()
                    };
                    mp.utf16_offset(se.offset)
                })
                .unwrap_or(0);
            let msg = format!("Error setting symbol value at line 0, offset {}", offset);
            errors_set(vm, c, &st, e.code, &msg);
            Value::False
        }
    }
}

/// msgfmt_get_pattern()
fn get_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match st.borrow().orig.clone() {
        Some(p) => sv(p),
        None => Value::False,
    }
}

/// msgfmt_get_locale()
fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let l = st.borrow().mf.as_ref().unwrap().locale.clone();
    s(l.as_bytes())
}

/// msgfmt_get_error_code()
fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(&c.obj()) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

/// msgfmt_get_error_message()
fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(&c.obj()) {
        Some(st) => s(st.borrow().err.message().as_bytes()),
        None => s(b"U_ZERO_ERROR"),
    }
}
