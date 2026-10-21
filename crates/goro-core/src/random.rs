//! Implementation of the PHP `Random\` namespace classes:
//!   * `Random\Engine\Mt19937`
//!   * `Random\Engine\Xoshiro256StarStar`
//!   * `Random\Engine\PcgOneseq128XslRr64`
//!   * `Random\Engine\Secure`
//!   * `Random\Randomizer`
//!
//! plus the engine-level helpers shared with the legacy ext/standard
//! functions (mt_rand(), rand(), shuffle(), str_shuffle(), array_rand(),
//! lcg_value(), random_int(), random_bytes()), ported from php-src
//! ext/random/{random,randomizer,gammasection,engine_*}.c and the
//! php_array_data_shuffle / php_array_pick_keys / php_binary_string_shuffle
//! helpers of ext/standard.
//!
//! All algorithms are hand-rolled in pure Rust; no external crates.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{NativeData, PhpObject};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{NativeObjectHandlers, Vm};

// ===== Mt19937 =====

const MT_N: usize = 624;
const MT_M: usize = 397;

#[derive(Clone)]
pub struct Mt19937State {
    pub state: [u32; MT_N],
    pub count: u32, // 0..=N
    pub mode: i64,  // 0 = MT_RAND_MT19937, 1 = MT_RAND_PHP
}

impl Mt19937State {
    pub fn new() -> Self {
        Self {
            state: [0u32; MT_N],
            count: MT_N as u32,
            mode: 0,
        }
    }

    pub fn seed32(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..MT_N {
            let prev = self.state[i - 1];
            self.state[i] = 1812433253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.count = MT_N as u32;
        self.reload();
    }

    fn reload(&mut self) {
        let mt19937 = self.mode == 0;
        let twist = |m: u32, u: u32, v: u32, mt: bool| -> u32 {
            let lo_v = v & 0x00000001;
            let lo_u = u & 0x00000001;
            let mix = (u & 0x80000000) | (v & 0x7FFFFFFF);
            let lo = if mt { lo_v } else { lo_u };
            let mag = (lo as i32).wrapping_neg() as u32 & 0x9908b0df;
            m ^ (mix >> 1) ^ mag
        };

        // p = state vector
        // for (i = N - M; i--; ++p) *p = twist(p[M], p[0], p[1])
        // First loop: indices 0..N-M
        for i in 0..(MT_N - MT_M) {
            let m = self.state[i + MT_M];
            let u = self.state[i];
            let v = self.state[i + 1];
            self.state[i] = twist(m, u, v, mt19937);
        }
        // Second loop: indices N-M..N-1
        for i in (MT_N - MT_M)..(MT_N - 1) {
            let m = self.state[i + MT_M - MT_N];
            let u = self.state[i];
            let v = self.state[i + 1];
            self.state[i] = twist(m, u, v, mt19937);
        }
        // Last: i = N - 1
        {
            let i = MT_N - 1;
            let m = self.state[i + MT_M - MT_N];
            let u = self.state[i];
            let v = self.state[0];
            self.state[i] = twist(m, u, v, mt19937);
        }

        self.count = 0;
    }

    /// Returns one 32-bit value.
    pub fn generate_u32(&mut self) -> u32 {
        if self.count >= MT_N as u32 {
            self.reload();
        }
        let mut s1 = self.state[self.count as usize];
        self.count += 1;
        s1 ^= s1 >> 11;
        s1 ^= (s1 << 7) & 0x9d2c5680;
        s1 ^= (s1 << 15) & 0xefc60000;
        s1 ^= s1 >> 18;
        s1
    }
}

impl Default for Mt19937State {
    fn default() -> Self {
        Self::new()
    }
}

// ===== Xoshiro256** =====

#[derive(Clone)]
pub struct Xoshiro256State {
    pub s: [u64; 4],
}

fn splitmix64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut r = *seed;
    r = (r ^ (r >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    r = (r ^ (r >> 27)).wrapping_mul(0x94d049bb133111eb);
    r ^ (r >> 31)
}

impl Xoshiro256State {
    pub fn new() -> Self {
        Self { s: [0; 4] }
    }
    pub fn seed64(&mut self, seed: u64) {
        let mut s = seed;
        self.s[0] = splitmix64(&mut s);
        self.s[1] = splitmix64(&mut s);
        self.s[2] = splitmix64(&mut s);
        self.s[3] = splitmix64(&mut s);
    }
    pub fn seed256(&mut self, s0: u64, s1: u64, s2: u64, s3: u64) {
        self.s = [s0, s1, s2, s3];
    }
    pub fn generate_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }
    #[allow(clippy::needless_range_loop)] // index loop mirrors the reference algorithm
    pub fn jump(&mut self, jmp: &[u64; 4]) {
        let mut t = [0u64; 4];
        for i in 0..4 {
            for b in 0..64 {
                if (jmp[i] >> b) & 1 != 0 {
                    t[0] ^= self.s[0];
                    t[1] ^= self.s[1];
                    t[2] ^= self.s[2];
                    t[3] ^= self.s[3];
                }
                self.generate_u64();
            }
        }
        self.s = t;
    }
    pub fn jump_short(&mut self) {
        let jmp: [u64; 4] = [
            0x180ec6d33cfd0aba,
            0xd5a61266f0c9392c,
            0xa9582618e03fc9aa,
            0x39abdc4529b1661c,
        ];
        self.jump(&jmp);
    }
    pub fn jump_long(&mut self) {
        let jmp: [u64; 4] = [
            0x76e15d3efefdcbbf,
            0xc5004e441c522fb3,
            0x77710069854ee241,
            0x39109bb02acbe635,
        ];
        self.jump(&jmp);
    }
}

impl Default for Xoshiro256State {
    fn default() -> Self {
        Self::new()
    }
}

// ===== PcgOneseq128XslRr64 =====

#[derive(Clone)]
pub struct PcgState {
    pub state: u128,
}

const PCG_MULT: u128 = ((2549297995355413924u128) << 64) | (4865540595714422341u128);
const PCG_INC: u128 = ((6364136223846793005u128) << 64) | (1442695040888963407u128);

impl PcgState {
    pub fn new() -> Self {
        Self { state: 0 }
    }
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(PCG_INC);
    }
    pub fn seed128(&mut self, hi: u64, lo: u64) {
        let seed: u128 = ((hi as u128) << 64) | (lo as u128);
        self.state = 0;
        self.step();
        self.state = self.state.wrapping_add(seed);
        self.step();
    }
    pub fn seed64(&mut self, seed: u64) {
        // PHP behavior: when called with int seed, hi=0, lo=seed
        self.seed128(0, seed);
    }
    pub fn generate_u64(&mut self) -> u64 {
        self.step();
        let hi = (self.state >> 64) as u64;
        let lo = self.state as u64;
        let v = hi ^ lo;
        let s = (hi >> 58) as u32;
        v.rotate_right(s)
    }
    pub fn advance(&mut self, mut delta: u64) {
        // Multi-step LCG advance via repeated squaring
        let mut cur_mult: u128 = PCG_MULT;
        let mut cur_plus: u128 = PCG_INC;
        let mut acc_mult: u128 = 1;
        let mut acc_plus: u128 = 0;
        while delta > 0 {
            if delta & 1 == 1 {
                acc_mult = acc_mult.wrapping_mul(cur_mult);
                acc_plus = acc_plus.wrapping_mul(cur_mult).wrapping_add(cur_plus);
            }
            cur_plus = cur_mult.wrapping_add(1).wrapping_mul(cur_plus);
            cur_mult = cur_mult.wrapping_mul(cur_mult);
            delta >>= 1;
        }
        self.state = acc_mult.wrapping_mul(self.state).wrapping_add(acc_plus);
    }
}

impl Default for PcgState {
    fn default() -> Self {
        Self::new()
    }
}

// ===== Secure (CSPRNG) =====
// Random\Engine\Secure, random_bytes() and random_int() read the OS CSPRNG.

fn os_random_seed() -> u64 {
    // Read 8 bytes from /dev/urandom; fall back to time.
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let mut buf = [0u8; 8];
        if f.read_exact(&mut buf).is_ok() {
            return u64::from_le_bytes(buf);
        }
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xdeadbeef)
}

fn os_random_bytes(out: &mut [u8]) {
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    // Fallback: deterministic but not zero
    let mut s = os_random_seed();
    for b in out.iter_mut() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *b = (s >> 33) as u8;
    }
}

// ===== Helpers: hex encode/decode little-endian =====

fn bin2hex_le<const N: usize>(bytes: [u8; N]) -> String {
    // PHP encodes the value as little-endian bytes -> hex.
    let mut s = String::with_capacity(N * 2);
    for b in bytes.iter() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn u32_to_hex_le(v: u32) -> String {
    bin2hex_le(v.to_le_bytes())
}
fn u64_to_hex_le(v: u64) -> String {
    bin2hex_le(v.to_le_bytes())
}

fn hex_to_u32_le(s: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(hex::<4>(s)?))
}
fn hex_to_u64_le(s: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(hex::<8>(s)?))
}

fn hex<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for i in 0..N {
        let hi = hex_digit(bytes[i * 2])?;
        let lo = hex_digit(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ===== Engine objects =====
//
// A native engine object carries its state as `PhpObject::native` data
// (php_random_engine.engine.state). A Randomizer built on a native engine
// shares that state, exactly like php-src copies the state pointer.

/// The state of a native engine (always held behind an `Rc`, so the size
/// difference between the variants does not matter).
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum EngineState {
    Mt(Mt19937State),
    Pcg(PcgState),
    Xoshiro(Xoshiro256State),
    Secure,
}

type StateRc = Rc<RefCell<EngineState>>;

const MT19937: &[u8] = b"random\\engine\\mt19937";
const PCG: &[u8] = b"random\\engine\\pcgoneseq128xslrr64";
const XOSHIRO: &[u8] = b"random\\engine\\xoshiro256starstar";
const SECURE: &[u8] = b"random\\engine\\secure";
const RANDOMIZER: &[u8] = b"random\\randomizer";

/// PHP_RANDOM_RANGE_ATTEMPTS
const RANGE_ATTEMPTS: u32 = 50;
/// PHP_MT_RAND_MAX
const MT_RAND_MAX: u32 = 0x7FFF_FFFF;

static ENGINE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    write_property: Some(no_dynamic_property),
    clone_obj: Some(engine_clone),
    ..NativeObjectHandlers::EMPTY
};

static RANDOMIZER_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    write_property: Some(randomizer_write_property),
    unset_property: Some(randomizer_unset_property),
    ..NativeObjectHandlers::EMPTY
};

fn class_lower(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
    obj.borrow().class_name.to_ascii_lowercase()
}

fn class_display(obj: &Rc<RefCell<PhpObject>>) -> String {
    crate::value::display_class_name(&obj.borrow().class_name)
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
}

/// EG(exception) after an engine call (exit() inside a user engine too).
fn failed(vm: &Vm) -> bool {
    vm.current_exception.is_some() || vm.exit_requested
}

fn throw_range_attempts(vm: &mut Vm) {
    throw(
        vm,
        b"Random\\BrokenRandomEngineError",
        &format!(
            "Failed to generate an acceptable random number in {} attempts",
            RANGE_ATTEMPTS
        ),
    );
}

/// `zend_throw_exception(NULL, "Invalid serialization data for X object")`,
/// chaining the pending exception (if any) as previous.
fn throw_invalid_serialization(vm: &mut Vm, class_name: &str) {
    let prev = vm.current_exception.take();
    let msg = format!("Invalid serialization data for {} object", class_name);
    let exc = vm.create_exception(b"Exception", &msg, vm.current_line);
    if let (Some(p), Value::Object(o)) = (prev, &exc) {
        o.borrow_mut().set_property(b"previous".to_vec(), p);
    }
    vm.current_exception = Some(exc);
}

fn new_engine_state(class_lower: &[u8]) -> Option<EngineState> {
    Some(match class_lower {
        MT19937 => EngineState::Mt(Mt19937State::new()),
        PCG => EngineState::Pcg(PcgState::new()),
        XOSHIRO => EngineState::Xoshiro(Xoshiro256State::new()),
        SECURE => EngineState::Secure,
        _ => return None,
    })
}

fn attach_engine_state(obj: &Rc<RefCell<PhpObject>>, st: EngineState) -> StateRc {
    let rc = Rc::new(RefCell::new(st));
    obj.borrow_mut().native = Some(NativeData {
        data: rc.clone() as Rc<dyn std::any::Any>,
        handlers: &ENGINE_HANDLERS,
    });
    rc
}

/// The state of a native engine object, allocated on first use (as
/// php_random_engine_common_init does when the object is created). None for
/// user engines.
fn engine_state(obj: &Rc<RefCell<PhpObject>>) -> Option<StateRc> {
    if let Some(n) = obj.borrow().native.as_ref() {
        if let Ok(rc) = n.data.clone().downcast::<RefCell<EngineState>>() {
            return Some(rc);
        }
    }
    let st = new_engine_state(&class_lower(obj))?;
    Some(attach_engine_state(obj, st))
}

/// php_random_engine_common_clone_object: the clone gets a copy of the state.
fn engine_clone(_vm: &mut Vm, src: &Rc<RefCell<PhpObject>>, dst: &Rc<RefCell<PhpObject>>) {
    if let Some(rc) = engine_state(src) {
        let st = rc.borrow().clone();
        attach_engine_state(dst, st);
    }
}

/// The engines are @strict-properties classes without properties.
fn no_dynamic_property(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    name: &[u8],
    _value: &Value,
) -> bool {
    let msg = format!(
        "Cannot create dynamic property {}::${}",
        class_display(obj),
        String::from_utf8_lossy(name)
    );
    throw(vm, b"Error", &msg);
    true
}

fn randomizer_engine_initialized(obj: &Rc<RefCell<PhpObject>>) -> bool {
    let o = obj.borrow();
    o.has_property(b"engine") && !matches!(o.get_property(b"engine"), Value::Undef)
}

fn scope_suffix(vm: &Vm) -> Option<String> {
    match vm.current_class_scope() {
        Some(s) if s.eq_ignore_ascii_case(RANDOMIZER) => None,
        Some(s) => Some(format!(
            " from scope {}",
            crate::value::display_class_name(&s)
        )),
        None => Some(" from global scope".to_string()),
    }
}

/// `public readonly Engine $engine` on a @strict-properties class.
fn randomizer_write_property(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    name: &[u8],
    value: &Value,
) -> bool {
    if name != b"engine" {
        return no_dynamic_property(vm, obj, name, value);
    }
    let msg = if randomizer_engine_initialized(obj) {
        "Cannot modify readonly property Random\\Randomizer::$engine".to_string()
    } else {
        match scope_suffix(vm) {
            Some(s) => {
                format!("Cannot initialize readonly property Random\\Randomizer::$engine{s}")
            }
            // Initialization from inside the class: default handling.
            None => return false,
        }
    };
    throw(vm, b"Error", &msg);
    true
}

fn randomizer_unset_property(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> bool {
    if name != b"engine" {
        return false;
    }
    let msg = if randomizer_engine_initialized(obj) {
        "Cannot unset readonly property Random\\Randomizer::$engine".to_string()
    } else {
        match scope_suffix(vm) {
            Some(s) => format!("Cannot unset readonly property Random\\Randomizer::$engine{s}"),
            None => return false,
        }
    };
    throw(vm, b"Error", &msg);
    true
}

fn attach_randomizer(obj: &Rc<RefCell<PhpObject>>) {
    if obj.borrow().native.is_none() {
        obj.borrow_mut().native = Some(NativeData {
            data: Rc::new(()) as Rc<dyn std::any::Any>,
            handlers: &RANDOMIZER_HANDLERS,
        });
    }
}

// ===== php_random_algo_with_state =====

/// An engine as used by the Randomizer and the legacy functions: a native
/// state, or a user object whose generate() method is called.
#[derive(Clone)]
pub enum Engine {
    Native(StateRc),
    User(Value),
}

/// php_random_result
#[derive(Clone, Copy)]
pub struct RandResult {
    pub size: usize,
    pub result: u64,
}

impl Engine {
    /// The engine behind an engine object (randomizer_common_init).
    pub fn from_value(v: &Value) -> Option<Engine> {
        let Value::Object(o) = v else {
            return None;
        };
        Some(match engine_state(o) {
            Some(rc) => Engine::Native(rc),
            None => Engine::User(v.clone()),
        })
    }

    /// A native Mt19937 engine in MT_RAND_PHP mode.
    fn is_mt_php_mode(&self) -> bool {
        match self {
            Engine::Native(rc) => matches!(&*rc.borrow(), EngineState::Mt(m) if m.mode != 0),
            Engine::User(_) => false,
        }
    }
}

/// algo->generate()
pub fn generate(vm: &mut Vm, e: &Engine) -> RandResult {
    match e {
        Engine::Native(rc) => match &mut *rc.borrow_mut() {
            EngineState::Mt(m) => RandResult {
                size: 4,
                result: m.generate_u32() as u64,
            },
            EngineState::Pcg(p) => RandResult {
                size: 8,
                result: p.generate_u64(),
            },
            EngineState::Xoshiro(x) => RandResult {
                size: 8,
                result: x.generate_u64(),
            },
            EngineState::Secure => {
                let mut b = [0u8; 8];
                os_random_bytes(&mut b);
                RandResult {
                    size: 8,
                    result: u64::from_le_bytes(b),
                }
            }
        },
        Engine::User(obj) => {
            // engine_user.c
            let fail = RandResult { size: 8, result: 0 };
            // zend_call_known_instance_method: an "[internal function]" frame.
            let r = vm.spl_call_method_at(obj, b"generate", &[], true);
            if failed(vm) {
                return fail;
            }
            let s = match r {
                Some(v) => v.deref().to_php_string(),
                None => PhpString::empty(),
            };
            let bytes = s.as_bytes();
            if bytes.is_empty() {
                throw(
                    vm,
                    b"Random\\BrokenRandomEngineError",
                    "A random engine must return a non-empty string",
                );
                return fail;
            }
            let size = bytes.len().min(8);
            let mut result = 0u64;
            for (i, b) in bytes[..size].iter().enumerate() {
                result |= (*b as u64) << (8 * i);
            }
            RandResult { size, result }
        }
    }
}

fn gen_u32(vm: &mut Vm, e: &Engine) -> Option<u32> {
    let mut result: u32 = 0;
    let mut total = 0usize;
    loop {
        let r = generate(vm, e);
        result |= (r.result as u32) << (total * 8);
        total += r.size;
        if failed(vm) {
            return None;
        }
        if total >= 4 {
            return Some(result);
        }
    }
}

fn gen_u64(vm: &mut Vm, e: &Engine) -> Option<u64> {
    let mut result: u64 = 0;
    let mut total = 0usize;
    loop {
        let r = generate(vm, e);
        result |= r.result << (total * 8);
        total += r.size;
        if failed(vm) {
            return None;
        }
        if total >= 8 {
            return Some(result);
        }
    }
}

/// php_random_range32()
pub fn range32(vm: &mut Vm, e: &Engine, umax: u32) -> u32 {
    let Some(mut result) = gen_u32(vm, e) else {
        return 0;
    };
    if umax == u32::MAX {
        return result;
    }
    let umax = umax + 1;
    if umax & (umax - 1) == 0 {
        return result & (umax - 1);
    }
    let limit = u32::MAX - (u32::MAX % umax) - 1;
    let mut count = 0;
    while result > limit {
        count += 1;
        if count > RANGE_ATTEMPTS {
            throw_range_attempts(vm);
            return 0;
        }
        match gen_u32(vm, e) {
            Some(r) => result = r,
            None => return 0,
        }
    }
    result % umax
}

/// php_random_range64()
pub fn range64(vm: &mut Vm, e: &Engine, umax: u64) -> u64 {
    let Some(mut result) = gen_u64(vm, e) else {
        return 0;
    };
    if umax == u64::MAX {
        return result;
    }
    let umax = umax + 1;
    if umax & (umax - 1) == 0 {
        return result & (umax - 1);
    }
    let limit = u64::MAX - (u64::MAX % umax) - 1;
    let mut count = 0;
    while result > limit {
        count += 1;
        if count > RANGE_ATTEMPTS {
            throw_range_attempts(vm);
            return 0;
        }
        match gen_u64(vm, e) {
            Some(r) => result = r,
            None => return 0,
        }
    }
    result % umax
}

/// php_random_range(): a value in [min, max].
pub fn range(vm: &mut Vm, e: &Engine, min: i64, max: i64) -> i64 {
    let umax = (max as u64).wrapping_sub(min as u64);
    if umax > u32::MAX as u64 {
        return range64(vm, e, umax).wrapping_add(min as u64) as i64;
    }
    (range32(vm, e, umax as u32) as u64).wrapping_add(min as u64) as i64
}

/// The RAND_RANGE_BADSCALING scaling of MT_RAND_PHP mode.
fn bad_scaling(r: u64, min: i64, max: i64) -> i64 {
    let offset = ((max as f64 - min as f64 + 1.0) * (r as f64 / (MT_RAND_MAX as f64 + 1.0))) as u64;
    offset.wrapping_add(min as u64) as i64
}

// ===== Legacy global state (RANDOM_G) =====

thread_local! {
    static DEFAULT_MT: StateRc = Rc::new(RefCell::new(EngineState::Mt(Mt19937State::new())));
    static MT_SEEDED: Cell<bool> = const { Cell::new(false) };
    static LCG_STATE: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

fn with_default_mt<R>(f: impl FnOnce(&mut Mt19937State) -> R) -> R {
    DEFAULT_MT.with(|rc| match &mut *rc.borrow_mut() {
        EngineState::Mt(m) => f(m),
        _ => unreachable!("the default engine is Mt19937"),
    })
}

/// php_random_default_engine(): the global Mt19937, seeded on first use.
pub fn default_engine() -> Engine {
    if !MT_SEEDED.get() {
        with_default_mt(|m| {
            m.mode = 0;
            m.seed32(os_random_seed() as u32);
        });
        MT_SEEDED.set(true);
    }
    Engine::Native(DEFAULT_MT.with(|rc| rc.clone()))
}

/// mt_srand() / srand(): `mode` other than MT_RAND_PHP selects MT_RAND_MT19937.
/// The MT_RAND_PHP deprecation is the caller's.
pub fn mt_srand(seed: Option<i64>, mode: i64) {
    with_default_mt(|m| {
        m.mode = if mode == 1 { 1 } else { 0 };
        let seed = match seed {
            Some(s) => s as u32,
            None => os_random_seed() as u32,
        };
        m.seed32(seed);
    });
    MT_SEEDED.set(true);
}

/// php_mt_rand(): the next raw 32-bit output of the global Mt19937.
pub fn mt_rand_u32() -> u32 {
    default_engine();
    with_default_mt(|m| m.generate_u32())
}

/// php_mt_rand_range()
pub fn mt_rand_range(vm: &mut Vm, min: i64, max: i64) -> i64 {
    let e = default_engine();
    range(vm, &e, min, max)
}

/// php_mt_rand_common(): mt_rand()/rand() with a range.
pub fn mt_rand_common(vm: &mut Vm, min: i64, max: i64) -> i64 {
    let e = default_engine();
    if !e.is_mt_php_mode() {
        return range(vm, &e, min, max);
    }
    let r = (with_default_mt(|m| m.generate_u32()) >> 1) as u64;
    bad_scaling(r, min, max)
}

/// php_combined_lcg(): lcg_value().
pub fn combined_lcg() -> f64 {
    fn modmult(a: i32, b: i32, c: i32, m: i32, s: i32) -> i32 {
        let q = s / a;
        let mut s = b
            .wrapping_mul(s.wrapping_sub(a.wrapping_mul(q)))
            .wrapping_sub(c.wrapping_mul(q));
        if s < 0 {
            s = s.wrapping_add(m);
        }
        s
    }
    let (s0, s1) = LCG_STATE.get().unwrap_or_else(|| {
        let seed = os_random_seed();
        (seed as u32 as i32, (seed >> 32) as u32 as i32)
    });
    let s0 = modmult(53668, 40014, 12211, 2147483563, s0);
    let s1 = modmult(52774, 40692, 3791, 2147483399, s1);
    LCG_STATE.set(Some((s0, s1)));
    let mut z = s0.wrapping_sub(s1);
    if z < 1 {
        z = z.wrapping_add(2147483562);
    }
    (z as u64) as f64 * 4.656613e-10
}

/// php_random_bytes(): fill `out` from the OS CSPRNG.
pub fn csprng_bytes(out: &mut [u8]) {
    os_random_bytes(out);
}

/// php_random_int(): a CSPRNG value in [min, max] (min <= max).
pub fn csprng_int(min: i64, max: i64) -> i64 {
    if min == max {
        return min;
    }
    let mut umax = (max as u64).wrapping_sub(min as u64);
    let next = || {
        let mut b = [0u8; 8];
        os_random_bytes(&mut b);
        u64::from_le_bytes(b)
    };
    let mut result = next();
    if umax == u64::MAX {
        return result as i64;
    }
    umax += 1;
    if umax & (umax - 1) != 0 {
        let limit = u64::MAX - (u64::MAX % umax) - 1;
        while result > limit {
            result = next();
        }
    }
    (result % umax).wrapping_add(min as u64) as i64
}

// ===== Shuffling and picking (ext/standard helpers) =====

/// php_array_data_shuffle() / php_binary_string_shuffle(): Fisher-Yates over
/// `items`. Returns false when the engine failed (exception pending).
pub fn shuffle_items<T>(vm: &mut Vm, e: &Engine, items: &mut [T]) -> bool {
    let mut n_left = items.len() as i64;
    if n_left <= 1 {
        return true;
    }
    loop {
        n_left -= 1;
        if n_left == 0 {
            return true;
        }
        let j = range(vm, e, 0, n_left);
        if failed(vm) {
            return false;
        }
        if j != n_left {
            items.swap(n_left as usize, j as usize);
        }
    }
}

fn key_value(k: &ArrayKey) -> Value {
    match k {
        ArrayKey::Int(n) => Value::Long(*n),
        ArrayKey::String(s) => Value::String(s.clone()),
    }
}

/// php_array_pick_keys(): `num_req` keys of an array (in array order), a
/// single key when `num_req` is 1. `fname` prefixes the argument errors.
/// None when an exception is pending.
pub fn pick_keys(
    vm: &mut Vm,
    e: &Engine,
    keys: &[ArrayKey],
    num_req: i64,
    fname: &str,
) -> Option<Value> {
    let num_avail = keys.len() as i64;
    if num_avail == 0 {
        throw(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($array) must not be empty", fname),
        );
        return None;
    }
    if num_req == 1 {
        // The arrays have no holes: sampling a bucket always hits.
        let r = range(vm, e, 0, num_avail - 1);
        if failed(vm) {
            return None;
        }
        return Some(key_value(&keys[r as usize]));
    }
    if num_req <= 0 || num_req > num_avail {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)",
                fname
            ),
        );
        return None;
    }
    let mut num_req = num_req;
    let negative = num_req > (num_avail >> 1);
    if negative {
        num_req = num_avail - num_req;
    }
    let mut bitset = vec![false; num_avail as usize];
    let mut i = num_req;
    let mut failures = 0;
    while i > 0 {
        let r = range(vm, e, 0, num_avail - 1);
        if failed(vm) {
            return None;
        }
        let r = r as usize;
        if bitset[r] {
            failures += 1;
            if failures > RANGE_ATTEMPTS {
                throw_range_attempts(vm);
                return None;
            }
        } else {
            bitset[r] = true;
            i -= 1;
            failures = 0;
        }
    }
    let mut out = PhpArray::new();
    for (k, picked) in keys.iter().zip(bitset) {
        if picked != negative {
            out.push(key_value(k));
        }
    }
    Some(Value::Array(Rc::new(RefCell::new(out))))
}

// ===== γ-section (gammasection.c) =====

fn next_after(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    if x == y {
        return y;
    }
    if x == 0.0 {
        let tiny = f64::from_bits(1);
        return if y > 0.0 { tiny } else { -tiny };
    }
    let bits = x.to_bits();
    if (y > x) == (x > 0.0) {
        f64::from_bits(bits + 1)
    } else {
        f64::from_bits(bits - 1)
    }
}

fn gamma_low(x: f64) -> f64 {
    x - next_after(x, -f64::MAX)
}

fn gamma_high(x: f64) -> f64 {
    next_after(x, f64::MAX) - x
}

fn gamma_max(x: f64, y: f64) -> f64 {
    if x.abs() > y.abs() {
        gamma_high(x)
    } else {
        gamma_low(y)
    }
}

fn split_int64(v: u64) -> (f64, f64) {
    ((v >> 2) as f64, (v & 0x3) as f64)
}

fn ceil_int(a: f64, b: f64, g: f64) -> u64 {
    let s = b / g - a / g;
    let e = if a.abs() <= b.abs() {
        -a / g - (s - b / g)
    } else {
        b / g - (s + a / g)
    };
    let si = s.ceil();
    if s != si {
        si as u64
    } else {
        (si as u64).wrapping_add((e > 0.0) as u64)
    }
}

fn from_max(max: f64, k: u64, g: f64) -> f64 {
    let (k_hi, k_lo) = split_int64(k);
    4.0 * (max * 0.25 - k_hi * g) - k_lo * g
}

fn from_min(min: f64, k: u64, g: f64) -> f64 {
    let (k_hi, k_lo) = split_int64(k);
    4.0 * (min * 0.25 + k_hi * g) + k_lo * g
}

#[derive(Clone, Copy, PartialEq)]
enum Boundary {
    ClosedOpen,
    ClosedClosed,
    OpenClosed,
    OpenOpen,
}

fn gammasection(vm: &mut Vm, e: &Engine, min: f64, max: f64, b: Boundary) -> f64 {
    let g = gamma_max(min, max);
    let hi = ceil_int(min, max, g);
    let near_min = min.abs() <= max.abs();
    match b {
        Boundary::ClosedOpen => {
            if max <= min || hi < 1 {
                return f64::NAN;
            }
            let k = 1 + range64(vm, e, hi - 1);
            if near_min {
                if k == hi { min } else { from_max(max, k, g) }
            } else {
                from_min(min, k - 1, g)
            }
        }
        Boundary::ClosedClosed => {
            if max < min {
                return f64::NAN;
            }
            let k = range64(vm, e, hi);
            if near_min {
                if k == hi { min } else { from_max(max, k, g) }
            } else if k == hi {
                max
            } else {
                from_min(min, k, g)
            }
        }
        Boundary::OpenClosed => {
            if max <= min || hi < 1 {
                return f64::NAN;
            }
            let k = range64(vm, e, hi - 1);
            if near_min {
                from_max(max, k, g)
            } else if k == hi - 1 {
                max
            } else {
                from_min(min, k + 1, g)
            }
        }
        Boundary::OpenOpen => {
            if max <= min || hi < 2 {
                return f64::NAN;
            }
            let k = 1 + range64(vm, e, hi - 2);
            if near_min {
                from_max(max, k, g)
            } else {
                from_min(min, k, g)
            }
        }
    }
}

// ===== Method dispatch =====
//
// Methods that don't need args are dispatched via dispatch_random_method().
// Methods that need args are dispatched via handle_random_docall().

pub fn is_random_class(class_lower: &[u8]) -> bool {
    matches!(class_lower, MT19937 | XOSHIRO | PCG | SECURE | RANDOMIZER)
}

pub fn is_random_engine_class(class_lower: &[u8]) -> bool {
    matches!(class_lower, MT19937 | XOSHIRO | PCG | SECURE)
}

pub fn is_random_args_method(class_lower: &[u8], method: &[u8]) -> bool {
    match class_lower {
        MT19937 => matches!(method, b"__construct" | b"__unserialize"),
        XOSHIRO => matches!(method, b"__construct" | b"__unserialize"),
        PCG => matches!(method, b"__construct" | b"__unserialize" | b"jump"),
        RANDOMIZER => matches!(
            method,
            b"__construct"
                | b"__unserialize"
                | b"getint"
                | b"getbytes"
                | b"getbytesfromstring"
                | b"getfloat"
                | b"shufflearray"
                | b"shufflebytes"
                | b"pickarraykeys"
        ),
        _ => false,
    }
}

fn str_value(bytes: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(bytes))
}

fn arr_value(arr: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(arr)))
}

fn hex_value(s: String) -> Value {
    Value::String(PhpString::from_string(s))
}

/// algo->serialize(): the engine state as hex strings (+ count/mode).
fn serialize_state(st: &EngineState) -> Option<PhpArray> {
    let mut a = PhpArray::new();
    match st {
        EngineState::Mt(m) => {
            for v in m.state.iter() {
                a.push(hex_value(u32_to_hex_le(*v)));
            }
            a.push(Value::Long(m.count as i64));
            a.push(Value::Long(m.mode));
        }
        EngineState::Pcg(p) => {
            a.push(hex_value(u64_to_hex_le((p.state >> 64) as u64)));
            a.push(hex_value(u64_to_hex_le(p.state as u64)));
        }
        EngineState::Xoshiro(x) => {
            for v in x.s.iter() {
                a.push(hex_value(u64_to_hex_le(*v)));
            }
        }
        EngineState::Secure => return None,
    }
    Some(a)
}

/// The hex string at `i` of a serialized state.
fn state_hex(a: &PhpArray, i: i64) -> Option<Vec<u8>> {
    match a.get_int(i).map(|v| v.deref()) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn state_long(a: &PhpArray, i: i64) -> Option<i64> {
    match a.get_int(i).map(|v| v.deref()) {
        Some(Value::Long(n)) => Some(n),
        _ => None,
    }
}

/// algo->unserialize()
fn unserialize_state(class_lower: &[u8], a: &PhpArray) -> Option<EngineState> {
    match class_lower {
        MT19937 => {
            if a.len() != MT_N + 2 {
                return None;
            }
            let mut m = Mt19937State::new();
            for i in 0..MT_N {
                m.state[i] = hex_to_u32_le(&state_hex(a, i as i64)?)?;
            }
            let count = state_long(a, MT_N as i64)?;
            if !(0..=MT_N as i64).contains(&count) {
                return None;
            }
            m.count = count as u32;
            let mode = state_long(a, MT_N as i64 + 1)?;
            if mode != 0 && mode != 1 {
                return None;
            }
            m.mode = mode;
            Some(EngineState::Mt(m))
        }
        PCG => {
            if a.len() != 2 {
                return None;
            }
            let hi = hex_to_u64_le(&state_hex(a, 0)?)?;
            let lo = hex_to_u64_le(&state_hex(a, 1)?)?;
            Some(EngineState::Pcg(PcgState {
                state: ((hi as u128) << 64) | lo as u128,
            }))
        }
        XOSHIRO => {
            if a.len() != 4 {
                return None;
            }
            let mut x = Xoshiro256State::new();
            for i in 0..4 {
                x.s[i] = hex_to_u64_le(&state_hex(a, i as i64)?)?;
            }
            Some(EngineState::Xoshiro(x))
        }
        _ => None,
    }
}

/// The first member that object_properties_load() would reject as a
/// dynamic property (the classes are @strict-properties).
fn first_dynamic_property(members: &PhpArray, allowed: &[u8]) -> Option<String> {
    members.iter().find_map(|(k, _)| match k {
        ArrayKey::String(s) if s.as_bytes() == allowed => None,
        ArrayKey::String(s) => Some(s.to_string_lossy()),
        ArrayKey::Int(n) => Some(n.to_string()),
    })
}

/// Dispatch no-arg methods (e.g. generate, nextInt, nextFloat, __serialize)
pub fn dispatch_random_method(
    vm: &mut Vm,
    class_lower: &[u8],
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    if is_random_engine_class(class_lower) {
        let st = engine_state(obj)?;
        return match method {
            b"generate" => {
                let e = Engine::Native(st);
                let r = generate(vm, &e);
                let bytes: Vec<u8> = (0..r.size).map(|i| (r.result >> (i * 8)) as u8).collect();
                Some(str_value(bytes))
            }
            b"__serialize" if class_lower != SECURE => {
                let state = serialize_state(&st.borrow())?;
                let mut out = PhpArray::new();
                out.push(arr_value(PhpArray::new()));
                out.push(arr_value(state));
                Some(arr_value(out))
            }
            b"__debuginfo" if class_lower != SECURE => {
                let state = serialize_state(&st.borrow())?;
                let mut out = PhpArray::new();
                out.set(
                    ArrayKey::String(PhpString::from_bytes(b"__states")),
                    arr_value(state),
                );
                Some(arr_value(out))
            }
            b"jump" if class_lower == XOSHIRO => {
                if let EngineState::Xoshiro(x) = &mut *st.borrow_mut() {
                    x.jump_short();
                }
                Some(Value::Null)
            }
            b"jumplong" if class_lower == XOSHIRO => {
                if let EngineState::Xoshiro(x) = &mut *st.borrow_mut() {
                    x.jump_long();
                }
                Some(Value::Null)
            }
            _ => None,
        };
    }
    if class_lower != RANDOMIZER {
        return None;
    }
    match method {
        b"nextint" => {
            let e = randomizer_engine(vm, obj)?;
            let r = generate(vm, &e);
            if failed(vm) {
                return Some(Value::Null);
            }
            Some(Value::Long((r.result >> 1) as i64))
        }
        b"nextfloat" => {
            let e = randomizer_engine(vm, obj)?;
            let Some(r) = gen_u64(vm, &e) else {
                return Some(Value::Null);
            };
            let step = 1.0 / (1u64 << 53) as f64;
            Some(Value::Double(step * (r >> 11) as f64))
        }
        b"__serialize" => {
            let mut props = PhpArray::new();
            if randomizer_engine_initialized(obj) {
                let engine = obj.borrow().get_property(b"engine");
                props.set(ArrayKey::String(PhpString::from_bytes(b"engine")), engine);
            }
            let mut out = PhpArray::new();
            out.push(arr_value(props));
            Some(arr_value(out))
        }
        _ => None,
    }
}

/// The Randomizer's engine (randomizer->engine).
fn randomizer_engine(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>) -> Option<Engine> {
    let v = obj.borrow().get_property(b"engine");
    let e = Engine::from_value(&v);
    if e.is_none() {
        throw(
            vm,
            b"Error",
            "Typed property Random\\Randomizer::$engine must not be accessed before initialization",
        );
    }
    e
}

fn is_engine_object(vm: &Vm, v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            is_random_engine_class(&class_lower(o)) || vm.obj_instanceof(v, b"random\\engine")
        }
        _ => false,
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).map(|v| v.deref()).unwrap_or(Value::Undef)
}

fn arg_is_null(args: &[Value], i: usize) -> bool {
    matches!(arg(args, i), Value::Null | Value::Undef)
}

fn le_word(b: &[u8], i: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[i * 8..i * 8 + 8]);
    u64::from_le_bytes(w)
}

/// Dispatch args methods. `args[0]` is `$this`, the rest are user args.
pub fn handle_random_docall(
    vm: &mut Vm,
    class_lower: &[u8],
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    if !is_random_class(class_lower) {
        return None;
    }
    let obj = match args.first()? {
        Value::Object(o) => o.clone(),
        _ => return None,
    };
    let null = Some(Value::Null);

    match (class_lower, method) {
        // ---- Engine __construct ----
        (MT19937, b"__construct") => {
            let mode = if arg_is_null(args, 2) {
                0
            } else {
                arg(args, 2).to_long()
            };
            match mode {
                0 => {}
                1 => vm.emit_deprecated_at(
                    "The MT_RAND_PHP variant of Mt19937 is deprecated",
                    vm.current_line,
                ),
                _ => {
                    throw(
                        vm,
                        b"ValueError",
                        "Random\\Engine\\Mt19937::__construct(): Argument #2 ($mode) must be either MT_RAND_MT19937 or MT_RAND_PHP",
                    );
                    return null;
                }
            }
            let seed = if arg_is_null(args, 1) {
                os_random_seed() as u32
            } else {
                arg(args, 1).to_long() as u32
            };
            let mut m = Mt19937State::new();
            m.mode = mode;
            m.seed32(seed);
            *engine_state(&obj)?.borrow_mut() = EngineState::Mt(m);
            null
        }

        (XOSHIRO, b"__construct") => {
            let mut x = Xoshiro256State::new();
            match arg(args, 1) {
                Value::Null | Value::Undef => x.seed64(os_random_seed()),
                Value::Long(n) => x.seed64(n as u64),
                Value::String(s) => {
                    let b = s.as_bytes();
                    if b.len() != 32 {
                        throw(
                            vm,
                            b"ValueError",
                            "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be a 32 byte (256 bit) string",
                        );
                        return null;
                    }
                    x.seed256(le_word(b, 0), le_word(b, 1), le_word(b, 2), le_word(b, 3));
                    if x.s == [0, 0, 0, 0] {
                        throw(
                            vm,
                            b"ValueError",
                            "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must not consist entirely of NUL bytes",
                        );
                        return null;
                    }
                }
                v => {
                    let msg = format!(
                        "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be of type string|int|null, {} given",
                        Vm::value_type_name(&v)
                    );
                    throw(vm, b"TypeError", &msg);
                    return null;
                }
            }
            *engine_state(&obj)?.borrow_mut() = EngineState::Xoshiro(x);
            null
        }

        (PCG, b"__construct") => {
            let mut p = PcgState::new();
            match arg(args, 1) {
                Value::Null | Value::Undef => p.seed128(os_random_seed(), os_random_seed()),
                Value::Long(n) => p.seed64(n as u64),
                Value::String(s) => {
                    let b = s.as_bytes();
                    if b.len() != 16 {
                        throw(
                            vm,
                            b"ValueError",
                            "Random\\Engine\\PcgOneseq128XslRr64::__construct(): Argument #1 ($seed) must be a 16 byte (128 bit) string",
                        );
                        return null;
                    }
                    p.seed128(le_word(b, 0), le_word(b, 1));
                }
                v => {
                    let msg = format!(
                        "Random\\Engine\\PcgOneseq128XslRr64::__construct(): Argument #1 ($seed) must be of type string|int|null, {} given",
                        Vm::value_type_name(&v)
                    );
                    throw(vm, b"TypeError", &msg);
                    return null;
                }
            }
            *engine_state(&obj)?.borrow_mut() = EngineState::Pcg(p);
            null
        }

        (PCG, b"jump") => {
            let advance = arg(args, 1).to_long();
            if advance < 0 {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Engine\\PcgOneseq128XslRr64::jump(): Argument #1 ($advance) must be greater than or equal to 0",
                );
                return null;
            }
            if let EngineState::Pcg(p) = &mut *engine_state(&obj)?.borrow_mut() {
                p.advance(advance as u64);
            }
            null
        }

        // ---- Engine __unserialize ----
        (MT19937 | XOSHIRO | PCG, b"__unserialize") => {
            let cname = class_display(&obj);
            let Value::Array(data) = arg(args, 1) else {
                throw_invalid_serialization(vm, &cname);
                return null;
            };
            let data = data.borrow();
            if data.len() != 2 {
                throw_invalid_serialization(vm, &cname);
                return null;
            }
            let Some(Value::Array(members)) = data.get_int(0).map(|v| v.deref()) else {
                throw_invalid_serialization(vm, &cname);
                return null;
            };
            let bad = first_dynamic_property(&members.borrow(), b"");
            if let Some(name) = bad {
                let msg = format!("Cannot create dynamic property {}::${}", cname, name);
                throw(vm, b"Error", &msg);
                throw_invalid_serialization(vm, &cname);
                return null;
            }
            let st = match data.get_int(1).map(|v| v.deref()) {
                Some(Value::Array(s)) => unserialize_state(class_lower, &s.borrow()),
                _ => None,
            };
            match st {
                Some(st) => *engine_state(&obj)?.borrow_mut() = st,
                None => throw_invalid_serialization(vm, &cname),
            }
            null
        }

        // ---- Randomizer ----
        (RANDOMIZER, b"__construct") => {
            let engine = if arg_is_null(args, 1) {
                let id = vm.alloc_object_id();
                let o = PhpObject::new(b"Random\\Engine\\Secure".to_vec(), id);
                Value::Object(Rc::new(RefCell::new(o)))
            } else {
                let v = arg(args, 1);
                if !is_engine_object(vm, &v) {
                    let msg = format!(
                        "Random\\Randomizer::__construct(): Argument #1 ($engine) must be of type ?Random\\Engine, {} given",
                        Vm::value_type_name(&v)
                    );
                    throw(vm, b"TypeError", &msg);
                    return null;
                }
                v
            };
            attach_randomizer(&obj);
            if randomizer_engine_initialized(&obj) {
                throw(
                    vm,
                    b"Error",
                    "Cannot modify readonly property Random\\Randomizer::$engine",
                );
                return null;
            }
            obj.borrow_mut().set_property(b"engine".to_vec(), engine);
            null
        }

        (RANDOMIZER, b"__unserialize") => {
            let cname = "Random\\Randomizer";
            attach_randomizer(&obj);
            let Value::Array(data) = arg(args, 1) else {
                throw_invalid_serialization(vm, cname);
                return null;
            };
            let members = {
                let d = data.borrow();
                match d.get_int(0).map(|v| v.deref()) {
                    Some(Value::Array(m)) if d.len() == 1 => Some(m),
                    _ => None,
                }
            };
            let Some(members) = members else {
                throw_invalid_serialization(vm, cname);
                return null;
            };
            let entries: Vec<(ArrayKey, Value)> = members
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.deref()))
                .collect();
            for (k, v) in entries {
                let is_engine = matches!(&k, ArrayKey::String(s) if s.as_bytes() == b"engine");
                let msg = if !is_engine {
                    let name = match &k {
                        ArrayKey::String(s) => s.to_string_lossy(),
                        ArrayKey::Int(n) => n.to_string(),
                    };
                    format!("Cannot create dynamic property {}::${}", cname, name)
                } else if randomizer_engine_initialized(&obj) {
                    "Cannot modify readonly property Random\\Randomizer::$engine".to_string()
                } else {
                    obj.borrow_mut().set_property(b"engine".to_vec(), v);
                    continue;
                };
                throw(vm, b"Error", &msg);
                throw_invalid_serialization(vm, cname);
                return null;
            }
            let engine = obj.borrow().get_property(b"engine");
            if !is_engine_object(vm, &engine) {
                throw_invalid_serialization(vm, cname);
            }
            null
        }

        (RANDOMIZER, _) => randomizer_call(vm, &obj, method, args),

        _ => None,
    }
}

fn randomizer_call(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let null = Some(Value::Null);
    match method {
        b"getint" => {
            let min = arg(args, 1).to_long();
            let max = arg(args, 2).to_long();
            if max < min {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getInt(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)",
                );
                return null;
            }
            let e = randomizer_engine(vm, obj)?;
            let v = if e.is_mt_php_mode() {
                let r = generate(vm, &e).result >> 1;
                bad_scaling(r, min, max)
            } else {
                range(vm, &e, min, max)
            };
            if failed(vm) {
                return null;
            }
            Some(Value::Long(v))
        }

        b"getbytes" => {
            let length = arg(args, 1).to_long();
            if length < 1 {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getBytes(): Argument #1 ($length) must be greater than 0",
                );
                return null;
            }
            let e = randomizer_engine(vm, obj)?;
            let length = length as usize;
            let mut out = Vec::with_capacity(length);
            while out.len() < length {
                let mut r = generate(vm, &e);
                if failed(vm) {
                    return null;
                }
                for _ in 0..r.size {
                    out.push(r.result as u8);
                    r.result >>= 8;
                    if out.len() >= length {
                        break;
                    }
                }
            }
            Some(str_value(out))
        }

        b"getbytesfromstring" => {
            let source = arg(args, 1).to_php_string();
            let source = source.as_bytes();
            let length = arg(args, 2).to_long();
            if source.is_empty() {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty",
                );
                return null;
            }
            if length < 1 {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getBytesFromString(): Argument #2 ($length) must be greater than 0",
                );
                return null;
            }
            let e = randomizer_engine(vm, obj)?;
            let length = length as usize;
            let max_offset = source.len() - 1;
            let mut out = Vec::with_capacity(length);
            if max_offset > 0xff {
                while out.len() < length {
                    let offset = range(vm, &e, 0, max_offset as i64);
                    if failed(vm) {
                        return null;
                    }
                    out.push(source[offset as usize]);
                }
            } else {
                let mut mask = max_offset as u64;
                mask |= mask >> 1;
                mask |= mask >> 2;
                mask |= mask >> 4;
                mask = mask.wrapping_mul(0x0101010101010101);
                let mut failures = 0;
                while out.len() < length {
                    let r = generate(vm, &e);
                    if failed(vm) {
                        return null;
                    }
                    let mut offsets = r.result & mask;
                    for _ in 0..r.size {
                        let offset = (offsets & 0xff) as usize;
                        offsets >>= 8;
                        if offset > max_offset {
                            failures += 1;
                            if failures > RANGE_ATTEMPTS {
                                throw_range_attempts(vm);
                                return null;
                            }
                            continue;
                        }
                        failures = 0;
                        out.push(source[offset]);
                        if out.len() >= length {
                            break;
                        }
                    }
                }
            }
            Some(str_value(out))
        }

        b"getfloat" => {
            let min = arg(args, 1).to_double();
            let max = arg(args, 2).to_double();
            let boundary = match arg(args, 3) {
                Value::Object(o) => {
                    let name = o.borrow().get_property(b"name").to_php_string();
                    match name.as_bytes() {
                        b"ClosedClosed" => Boundary::ClosedClosed,
                        b"OpenClosed" => Boundary::OpenClosed,
                        b"OpenOpen" => Boundary::OpenOpen,
                        _ => Boundary::ClosedOpen,
                    }
                }
                _ => Boundary::ClosedOpen,
            };
            if !min.is_finite() {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getFloat(): Argument #1 ($min) must be finite",
                );
                return null;
            }
            if !max.is_finite() {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getFloat(): Argument #2 ($max) must be finite",
                );
                return null;
            }
            if boundary == Boundary::ClosedClosed {
                if max < min {
                    throw(
                        vm,
                        b"ValueError",
                        "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)",
                    );
                    return null;
                }
            } else if max <= min {
                throw(
                    vm,
                    b"ValueError",
                    "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than argument #1 ($min)",
                );
                return null;
            }
            let e = randomizer_engine(vm, obj)?;
            let v = gammasection(vm, &e, min, max, boundary);
            if failed(vm) {
                return null;
            }
            if boundary == Boundary::OpenOpen && v.is_nan() {
                throw(
                    vm,
                    b"ValueError",
                    "The given interval is empty, there are no floats between argument #1 ($min) and argument #2 ($max)",
                );
                return null;
            }
            Some(Value::Double(v))
        }

        b"shufflearray" => {
            let Value::Array(arr) = arg(args, 1) else {
                return null;
            };
            let mut values: Vec<Value> = arr.borrow().values().cloned().collect();
            let e = randomizer_engine(vm, obj)?;
            if !shuffle_items(vm, &e, &mut values) {
                return null;
            }
            let mut out = PhpArray::with_capacity(values.len());
            for v in values {
                out.push(v);
            }
            Some(arr_value(out))
        }

        b"shufflebytes" => {
            let mut bytes = arg(args, 1).to_php_string().as_bytes().to_vec();
            if bytes.len() < 2 {
                return Some(str_value(bytes));
            }
            let e = randomizer_engine(vm, obj)?;
            if !shuffle_items(vm, &e, &mut bytes) {
                return null;
            }
            Some(str_value(bytes))
        }

        b"pickarraykeys" => {
            let Value::Array(arr) = arg(args, 1) else {
                return null;
            };
            let num = arg(args, 2).to_long();
            let keys: Vec<ArrayKey> = arr.borrow().keys().cloned().collect();
            let e = randomizer_engine(vm, obj)?;
            match pick_keys(vm, &e, &keys, num, "Random\\Randomizer::pickArrayKeys")? {
                Value::Array(a) => Some(Value::Array(a)),
                single => {
                    let mut out = PhpArray::new();
                    out.push(single);
                    Some(arr_value(out))
                }
            }
        }

        _ => None,
    }
}
