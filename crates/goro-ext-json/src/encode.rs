//! `json_encode`: an iterative encoder (no Rust recursion per nesting
//! level) with PHP's error semantics.
//!
//! Errors (observed on PHP 8.5, see the fixture):
//!
//! - The last error raised is the one reported.
//! - With `JSON_PARTIAL_OUTPUT_ON_ERROR` encoding always goes on, with a
//!   stand-in for the bad value: `null` for malformed strings, resources
//!   and recursion, `0` for INF/NAN and pure enums, `""` for a malformed
//!   key (tests ext/json/tests/bug43941.phpt, bug68567.phpt,
//!   inf_nan_error.phpt).
//! - Without it, INF/NAN and malformed keys still let encoding go on (so
//!   later `jsonSerialize()` calls happen and later errors replace the
//!   code); every other error stops it.
//! - The depth is checked when an array or object is closed, after its
//!   contents (`[[NAN]]` with depth 1 reports the depth error, `[["\xff"]]`
//!   the UTF-8 one): nesting level `n` fails when `n > $depth` (test
//!   ext/json/tests/bug81532.phpt).
//!
//! Recursion: arrays (by identity of their shared storage), objects and
//! `JsonSerializable` objects have separate thread-local guards that span
//! nested `json_encode()` calls. A `jsonSerialize()` that returns `$this`
//! has its properties encoded as a plain object, after its own guard is
//! released (tests ext/json/tests/bug61978.phpt,
//! json_encode_recursion_01.phpt, serialize.phpt).

use std::cell::RefCell;
use std::collections::HashSet;

use goro_runtime::numeric::{Number, numeric_value};
use goro_runtime::{KeyRef, ObjRef, PhpArray, PhpStr, Value, convert, float_fmt};

use crate::consts::*;
use crate::{JsonError, JsonHost, ObjectKind, Utf8Mode};

#[derive(Default)]
struct Guards {
    /// Array storage addresses.
    arrays: GuardStack,
    /// Object handles.
    objects: GuardStack,
    serializable: GuardStack,
}

/// The containers being encoded, innermost last: a linear scan for the
/// usual shallow nesting, a hash set for the levels beyond, so that very
/// deep documents stay linear.
#[derive(Default)]
struct GuardStack {
    items: Vec<usize>,
    deep: HashSet<usize>,
}

const SHALLOW: usize = 16;

impl GuardStack {
    #[inline]
    fn contains(&self, x: &usize) -> bool {
        let n = self.items.len();
        self.items[..n.min(SHALLOW)].contains(x) || (n > SHALLOW && self.deep.contains(x))
    }

    #[inline]
    fn push(&mut self, x: usize) {
        if self.items.len() >= SHALLOW {
            self.deep.insert(x);
        }
        self.items.push(x);
    }

    #[inline]
    fn pop(&mut self) {
        if let Some(x) = self.items.pop()
            && self.items.len() >= SHALLOW
        {
            self.deep.remove(&x);
        }
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn truncate(&mut self, n: usize) {
        while self.items.len() > n {
            self.pop();
        }
    }
}

thread_local! {
    static GUARDS: RefCell<Guards> = RefCell::new(Guards::default());
}

/// Encodes `value`. Returns the output and the error (the output is
/// meaningful with `JSON_PARTIAL_OUTPUT_ON_ERROR` or when there is no
/// error). `depth` is `json_encode`'s `$depth` as a 32-bit int.
pub fn encode<H: JsonHost + ?Sized>(
    host: &mut H,
    value: &Value,
    flags: i64,
    depth: i32,
) -> Result<(Vec<u8>, JsonError), H::Exception> {
    let guards = GUARDS.with(|g| std::mem::take(&mut *g.borrow_mut()));
    let base = (
        guards.arrays.len(),
        guards.objects.len(),
        guards.serializable.len(),
    );
    let mut e = Encoder {
        precision: host.serialize_precision(),
        max_nesting: host.max_encode_nesting().unwrap_or(usize::MAX),
        host,
        out: Vec::new(),
        flags,
        partial: flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0,
        pretty: flags & JSON_PRETTY_PRINT != 0,
        utf8: Utf8Mode::from_encode_flags(flags),
        max_depth: depth,
        err: JsonError::None,
        frames: Vec::new(),
        guards,
        base,
        esc: escape_table(flags),
        simple_ascii: flags & (JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT) == 0,
    };
    match e.run(value) {
        Ok(()) | Err(Halt::Stop) => Ok((std::mem::take(&mut e.out), e.err)),
        Err(Halt::Exception(x)) => Err(x),
    }
}

enum Halt<E> {
    /// An error that ends encoding (output is discarded).
    Stop,
    /// An exception from the host.
    Exception(E),
}

struct Frame {
    arr: PhpArray,
    pos: u32,
    /// Encoded as `{...}`.
    map: bool,
    /// Nothing written yet.
    first: bool,
    /// An object's property table: skip mangled (NUL-prefixed) names.
    props: bool,
    /// `JsonSerializable` guards to release when this frame closes.
    js_pops: u32,
}

struct Encoder<'h, H: JsonHost + ?Sized> {
    host: &'h mut H,
    out: Vec<u8>,
    flags: i64,
    partial: bool,
    pretty: bool,
    utf8: Utf8Mode,
    precision: i32,
    max_depth: i32,
    max_nesting: usize,
    err: JsonError,
    frames: Vec<Frame>,
    guards: Guards,
    base: (usize, usize, usize),
    /// Per ASCII byte: 0 when it is copied as is, else the escape kind.
    esc: [u8; 128],
    /// No `JSON_HEX_*` flag: the SIMD scan's notion of "plain" is exact.
    simple_ascii: bool,
}

impl<H: JsonHost + ?Sized> Drop for Encoder<'_, H> {
    fn drop(&mut self) {
        let mut g = std::mem::take(&mut self.guards);
        g.arrays.truncate(self.base.0);
        g.objects.truncate(self.base.1);
        g.serializable.truncate(self.base.2);
        GUARDS.with(|cell| *cell.borrow_mut() = g);
    }
}

/// Escape kinds in the table.
const E_COPY: u8 = 0;
const E_SHORT: u8 = 1; // \" \\ \/ \b \f \n \r \t
const E_HEX: u8 = 2; // \u00XX

fn escape_table(flags: i64) -> [u8; 128] {
    let mut t = [E_COPY; 128];
    t[..0x20].fill(E_HEX);
    for c in [0x08u8, 0x0c, b'\n', b'\r', b'\t', b'\\', b'"'] {
        t[c as usize] = E_SHORT;
    }
    if flags & JSON_UNESCAPED_SLASHES == 0 {
        t[b'/' as usize] = E_SHORT;
    }
    if flags & JSON_HEX_QUOT != 0 {
        t[b'"' as usize] = E_HEX;
    }
    if flags & JSON_HEX_TAG != 0 {
        t[b'<' as usize] = E_HEX;
        t[b'>' as usize] = E_HEX;
    }
    if flags & JSON_HEX_AMP != 0 {
        t[b'&' as usize] = E_HEX;
    }
    if flags & JSON_HEX_APOS != 0 {
        t[b'\'' as usize] = E_HEX;
    }
    t
}

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

impl<H: JsonHost + ?Sized> Encoder<'_, H> {
    fn run(&mut self, value: &Value) -> Result<(), Halt<H::Exception>> {
        self.value(value)?;
        while !self.frames.is_empty() {
            self.step()?;
        }
        Ok(())
    }

    /// Records an error; with partial output writes `stand_in` and goes
    /// on, else stops.
    #[cold]
    fn fail(&mut self, e: JsonError, stand_in: &[u8]) -> Result<(), Halt<H::Exception>> {
        self.err = e;
        if self.partial {
            self.out.extend_from_slice(stand_in);
            Ok(())
        } else {
            Err(Halt::Stop)
        }
    }

    fn indent(&mut self, level: usize) {
        self.out.push(b'\n');
        self.out.resize(self.out.len() + 4 * level, b' ');
    }

    /// Writes the next element of the innermost container, or closes it.
    fn step(&mut self) -> Result<(), Halt<H::Exception>> {
        let level = self.frames.len();
        let f = self.frames.last_mut().expect("frame");
        let next = f
            .arr
            .data()
            .next_from(f.pos)
            .map(|(p, k, v)| (p, k.to_owned(), v.clone()));
        let Some((p, k, v)) = next else {
            return self.close();
        };
        f.pos = p + 1;
        if f.props
            && let goro_runtime::Key::Str(s) = &k
            && s.first() == Some(&0)
        {
            return Ok(());
        }
        let map = f.map;
        if !std::mem::replace(&mut f.first, false) {
            self.out.push(b',');
        }
        if self.pretty {
            self.indent(level);
        }
        if map {
            self.key(k.as_ref());
            self.out.push(b':');
            if self.pretty {
                self.out.push(b' ');
            }
        }
        self.value(&v)
    }

    fn close(&mut self) -> Result<(), Halt<H::Exception>> {
        let level = self.frames.len();
        let f = self.frames.pop().expect("frame");
        if self.pretty && !f.first {
            self.indent(level - 1);
        }
        self.out.push(if f.map { b'}' } else { b']' });
        if f.props {
            self.guards.objects.pop();
        } else {
            self.guards.arrays.pop();
        }
        for _ in 0..f.js_pops {
            self.guards.serializable.pop();
        }
        self.depth_check(level)
    }

    #[inline]
    fn depth_check(&mut self, level: usize) -> Result<(), Halt<H::Exception>> {
        if level as i64 > self.max_depth as i64 {
            self.err = JsonError::Depth;
            if !self.partial {
                return Err(Halt::Stop);
            }
        }
        Ok(())
    }

    /// Opens a container frame (`arr` non-empty).
    fn open(&mut self, arr: PhpArray, map: bool, props: bool) -> Result<(), Halt<H::Exception>> {
        if self.frames.len() >= self.max_nesting {
            self.err = JsonError::Depth;
            return Err(Halt::Stop);
        }
        self.out.push(if map { b'{' } else { b'[' });
        self.frames.push(Frame {
            arr,
            pos: 0,
            map,
            first: true,
            props,
            js_pops: 0,
        });
        Ok(())
    }

    /// An empty array or object.
    fn empty(&mut self, map: bool) -> Result<(), Halt<H::Exception>> {
        self.out.extend_from_slice(if map { b"{}" } else { b"[]" });
        self.depth_check(self.frames.len() + 1)
    }

    fn value(&mut self, v: &Value) -> Result<(), Halt<H::Exception>> {
        match v {
            Value::Null => self.out.extend_from_slice(b"null"),
            Value::Bool(true) => self.out.extend_from_slice(b"true"),
            Value::Bool(false) => self.out.extend_from_slice(b"false"),
            Value::Int(i) => convert::push_int(&mut self.out, *i),
            Value::Float(f) => self.float(*f),
            Value::Str(s) => return self.string_value(s),
            Value::Array(a) => return self.array(a),
            Value::Object(o) => return self.object(o),
            Value::Resource(_) => return self.fail(JsonError::UnsupportedType, b"null"),
            Value::Ref(r) => {
                let inner = r.get();
                return self.value(&inner);
            }
        }
        Ok(())
    }

    fn array(&mut self, a: &PhpArray) -> Result<(), Halt<H::Exception>> {
        let force = self.flags & JSON_FORCE_OBJECT != 0;
        if a.is_empty() {
            return self.empty(force);
        }
        let id = a.data() as *const _ as usize;
        if self.guards.arrays.contains(&id) {
            return self.fail(JsonError::Recursion, b"null");
        }
        let map = force || !a.data().is_list();
        self.guards.arrays.push(id);
        self.open(a.clone(), map, false)
    }

    fn object(&mut self, o: &ObjRef) -> Result<(), Halt<H::Exception>> {
        match self.host.object_kind(o) {
            ObjectKind::Plain => self.plain_object(o),
            ObjectKind::Serializable => self.serializable(o),
            ObjectKind::UnitEnum => self.fail(JsonError::NonBackedEnum, b"0"),
            ObjectKind::BackedEnum(v) => self.value(&v),
        }
    }

    /// Runs a host callback with the guards visible to nested calls.
    fn call_host<R>(
        &mut self,
        f: impl FnOnce(&mut H) -> Result<R, H::Exception>,
    ) -> Result<R, Halt<H::Exception>> {
        GUARDS.with(|g| std::mem::swap(&mut *g.borrow_mut(), &mut self.guards));
        let r = f(self.host);
        GUARDS.with(|g| std::mem::swap(&mut *g.borrow_mut(), &mut self.guards));
        r.map_err(Halt::Exception)
    }

    fn serializable(&mut self, o: &ObjRef) -> Result<(), Halt<H::Exception>> {
        let h = o.handle();
        if self.guards.serializable.contains(&(h as usize)) {
            return self.fail(JsonError::Recursion, b"null");
        }
        self.guards.serializable.push(h as usize);
        let r = self.call_host(|host| host.json_serialize(o))?;
        if let Value::Object(ro) = &r
            && ObjRef::ptr_eq(ro, o)
        {
            self.guards.serializable.pop();
            return self.plain_object(o);
        }
        let before = self.frames.len();
        self.value(&r)?;
        if self.frames.len() > before {
            // The returned container is encoded by the main loop; release
            // the guard when it is done.
            if let Some(f) = self.frames.last_mut() {
                f.js_pops += 1;
            }
        } else {
            self.guards.serializable.pop();
        }
        Ok(())
    }

    fn plain_object(&mut self, o: &ObjRef) -> Result<(), Halt<H::Exception>> {
        let h = o.handle();
        if self.guards.objects.contains(&(h as usize)) {
            return self.fail(JsonError::Recursion, b"null");
        }
        let props = self.call_host(|host| host.object_properties(o))?;
        if props.is_empty() {
            return self.empty(true);
        }
        self.guards.objects.push(h as usize);
        self.open(props, true, true)
    }

    fn float(&mut self, f: f64) {
        if !f.is_finite() {
            // Never stops encoding (observed on PHP 8.5).
            self.err = JsonError::InfOrNan;
            self.out.push(b'0');
            return;
        }
        let start = self.out.len();
        if self.precision == 0 {
            format_precision0(f, &mut self.out);
        } else {
            float_fmt::format_php_repr(f, self.precision, b'e', &mut self.out);
        }
        if self.flags & JSON_PRESERVE_ZERO_FRACTION != 0
            && self.out[start..]
                .iter()
                .all(|&c| c.is_ascii_digit() || c == b'-')
        {
            self.out.extend_from_slice(b".0");
        }
    }

    fn string_value(&mut self, s: &PhpStr) -> Result<(), Halt<H::Exception>> {
        if self.flags & JSON_NUMERIC_CHECK != 0 {
            // PHP manual: "Encodes numeric strings as numbers"; observed on
            // PHP 8.5: one that overflows to INF stays a string.
            match numeric_value(s.as_bytes()) {
                Some(Number::Int(i)) => {
                    convert::push_int(&mut self.out, i);
                    return Ok(());
                }
                Some(Number::Float(f)) if f.is_finite() => {
                    self.float(f);
                    return Ok(());
                }
                _ => {}
            }
        }
        let start = self.out.len();
        if !self.string(s.as_bytes()) {
            self.out.truncate(start);
            return self.fail(JsonError::Utf8, b"null");
        }
        Ok(())
    }

    fn key(&mut self, k: KeyRef<'_>) {
        match k {
            KeyRef::Int(i) => {
                self.out.push(b'"');
                convert::push_int(&mut self.out, i);
                self.out.push(b'"');
            }
            KeyRef::Str(s) => {
                let start = self.out.len();
                if !self.string(s.as_bytes()) {
                    // Observed on PHP 8.5: a malformed key does not stop
                    // encoding; partial output shows it as "".
                    self.out.truncate(start);
                    self.out.extend_from_slice(b"\"\"");
                    self.err = JsonError::Utf8;
                }
            }
        }
    }

    /// Writes `b` as a JSON string. Returns false for malformed UTF-8 when
    /// that is an error.
    fn string(&mut self, b: &[u8]) -> bool {
        let out = &mut self.out;
        out.reserve(b.len() + 2);
        out.push(b'"');
        let unescaped = self.flags & JSON_UNESCAPED_UNICODE != 0;
        let line_terms = self.flags & JSON_UNESCAPED_LINE_TERMINATORS != 0;
        let mut run = 0;
        let mut i = 0;
        while i < b.len() {
            if self.simple_ascii {
                i = scan_plain(b, i, self.esc[b'/' as usize] == E_COPY);
                if i >= b.len() {
                    break;
                }
            }
            let c = b[i];
            if c < 0x80 {
                match self.esc[c as usize] {
                    E_COPY => i += 1,
                    E_SHORT => {
                        out.extend_from_slice(&b[run..i]);
                        let e = match c {
                            0x08 => b'b',
                            0x0c => b'f',
                            b'\n' => b'n',
                            b'\r' => b'r',
                            b'\t' => b't',
                            c => c,
                        };
                        out.extend_from_slice(&[b'\\', e]);
                        i += 1;
                        run = i;
                    }
                    _ => {
                        out.extend_from_slice(&b[run..i]);
                        // Observed on PHP 8.5: "\u001f" (lower case) for
                        // controls, "<" (upper case) for JSON_HEX_*.
                        let hex = if c < 0x20 { HEX_LOWER } else { HEX_UPPER };
                        out.extend_from_slice(&[
                            b'\\',
                            b'u',
                            b'0',
                            b'0',
                            hex[(c >> 4) as usize],
                            hex[(c & 15) as usize],
                        ]);
                        i += 1;
                        run = i;
                    }
                }
                continue;
            }
            match next_char(b, i) {
                Ok((cp, n)) => {
                    if unescaped && (line_terms || (cp != 0x2028 && cp != 0x2029)) {
                        i += n;
                        continue;
                    }
                    out.extend_from_slice(&b[run..i]);
                    if cp >= 0x10000 {
                        let v = cp - 0x10000;
                        push_u16_escape(out, 0xD800 | (v >> 10));
                        push_u16_escape(out, 0xDC00 | (v & 0x3FF));
                    } else {
                        push_u16_escape(out, cp);
                    }
                    i += n;
                    run = i;
                }
                Err(n) => {
                    out.extend_from_slice(&b[run..i]);
                    match self.utf8 {
                        Utf8Mode::Error => return false,
                        Utf8Mode::Ignore => {}
                        Utf8Mode::Substitute => {
                            if unescaped {
                                out.extend_from_slice("\u{fffd}".as_bytes());
                            } else {
                                out.extend_from_slice(b"\\ufffd");
                            }
                        }
                    }
                    i += n;
                    run = i;
                }
            }
        }
        out.extend_from_slice(&b[run..]);
        out.push(b'"');
        true
    }
}

fn push_u16_escape(out: &mut Vec<u8>, u: u32) {
    out.extend_from_slice(&[
        b'\\',
        b'u',
        HEX_LOWER[(u >> 12 & 15) as usize],
        HEX_LOWER[(u >> 8 & 15) as usize],
        HEX_LOWER[(u >> 4 & 15) as usize],
        HEX_LOWER[(u & 15) as usize],
    ]);
}

/// Decodes the character at `b[i]` (non-ASCII) as `json_encode` does:
/// `Ok((code point, length))`, or `Err(bytes to skip)` for a malformed
/// sequence.
///
/// Observed on PHP 8.5 (fixture): a malformed sequence counts as one bad
/// character whose length is that of the lead byte's sequence, cut short
/// before the first byte that is not a continuation byte and could start a
/// character itself (ASCII or `C2..F4`): "\xf0\x9f\x98A" is one bad
/// character then "A", "\xc2\xc0" one bad character, "\xc2\xc2" two.
/// Overlong forms, surrogates and code points above U+10FFFF made of
/// continuation bytes are one bad character of full length.
#[inline]
fn next_char(b: &[u8], i: usize) -> Result<(u32, usize), usize> {
    let c = b[i];
    let avail = b.len() - i;
    let trail = |j: usize| j < avail && b[i + j] & 0xC0 == 0x80;
    let lead = |j: usize| {
        let x = b[i + j];
        x < 0x80 || (0xC2..=0xF4).contains(&x)
    };
    let cut = |need: usize| -> usize {
        for j in 1..need {
            if j >= avail || lead(j) {
                return j;
            }
        }
        need
    };
    let t = |j: usize| (b[i + j] & 0x3F) as u32;
    match c {
        0xC2..=0xDF => {
            if !trail(1) {
                return Err(cut(2));
            }
            Ok(((c as u32 & 0x1F) << 6 | t(1), 2))
        }
        0xE0..=0xEF => {
            if !(trail(1) && trail(2)) {
                return Err(cut(3));
            }
            let cp = (c as u32 & 0x0F) << 12 | t(1) << 6 | t(2);
            if cp < 0x800 || (0xD800..=0xDFFF).contains(&cp) {
                return Err(3);
            }
            Ok((cp, 3))
        }
        0xF0..=0xF4 => {
            if !(trail(1) && trail(2) && trail(3)) {
                return Err(cut(4));
            }
            let cp = (c as u32 & 0x07) << 18 | t(1) << 12 | t(2) << 6 | t(3);
            if !(0x10000..=0x10FFFF).contains(&cp) {
                return Err(4);
            }
            Ok((cp, 4))
        }
        _ => Err(1),
    }
}

/// Index of the first byte at or after `i` that is not copied as is
/// (control, `"`, `\`, `/` unless unescaped, non-ASCII), or `b.len()`.
#[inline]
fn scan_plain(b: &[u8], mut i: usize, slash_plain: bool) -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline; loads are unaligned
        // and within bounds.
        unsafe {
            use std::arch::x86_64::*;
            let q = _mm_set1_epi8(b'"' as i8);
            let bs = _mm_set1_epi8(b'\\' as i8);
            let sl = _mm_set1_epi8(if slash_plain { b'"' } else { b'/' } as i8);
            let sp = _mm_set1_epi8(0x20);
            while i + 16 <= b.len() {
                let x = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
                let m = _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(x, q), _mm_cmpeq_epi8(x, bs)),
                    _mm_or_si128(_mm_cmpeq_epi8(x, sl), _mm_cmplt_epi8(x, sp)),
                );
                let mask = _mm_movemask_epi8(m) as u32;
                if mask != 0 {
                    return i + mask.trailing_zeros() as usize;
                }
                i += 16;
            }
        }
    }
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\\' || (c == b'/' && !slash_plain) || !(0x20..0x80).contains(&c) {
            return i;
        }
        i += 1;
    }
    i
}

/// `serialize_precision = 0`: one significant digit, and (observed on PHP
/// 8.5, unlike `var_dump`) the exponential form from exponent 0 on:
/// 1.5 gives "2.0e+0", 0.1 gives "0.1", 0.0 gives "0.0e+0".
fn format_precision0(f: f64, out: &mut Vec<u8>) {
    if f.is_sign_negative() {
        out.push(b'-');
    }
    let a = f.abs();
    if a == 0.0 {
        out.extend_from_slice(b"0.0e+0");
        return;
    }
    let (d, x) = float_fmt::fixed_digits(a, 1);
    let d = d.as_bytes();
    if !(-4..0).contains(&x) {
        out.push(d[0]);
        out.extend_from_slice(b".0e");
        out.push(if x < 0 { b'-' } else { b'+' });
        convert::push_int(out, x.unsigned_abs() as i64);
    } else {
        out.extend_from_slice(b"0.");
        out.resize(out.len() + (-x - 1) as usize, b'0');
        out.push(d[0]);
    }
}
