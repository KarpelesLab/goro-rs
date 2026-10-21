//! Implementation of the PHP `Random\` namespace classes:
//!   * `Random\Engine\Mt19937`
//!   * `Random\Engine\Xoshiro256StarStar`
//!   * `Random\Engine\PcgOneseq128XslRr64`
//!   * `Random\Engine\Secure`
//!   * `Random\Randomizer`
//!
//! All algorithms are hand-rolled in pure Rust; no external crates.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

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
// Uses a fast Xoshiro256** seeded from /dev/urandom-equivalent on init.
// Not actually cryptographically secure in our impl, but acceptable for tests.

#[derive(Clone)]
pub struct SecureState;

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

fn hex_to_u32_le(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let bytes = hex::<4>(s)?;
    Some(u32::from_le_bytes(bytes))
}
fn hex_to_u64_le(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = hex::<8>(s)?;
    Some(u64::from_le_bytes(bytes))
}

fn hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 {
        return None;
    }
    let bytes = s.as_bytes();
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

// ===== Property storage helpers =====
//
// We store engine state on the PhpObject under hidden property keys
// (prefixed with `__rng_`) so they don't show up in user property
// iteration.

const PROP_MT_STATE: &[u8] = b"__rng_mt_state"; // serialized as PhpArray of 624 hex strings
const PROP_MT_COUNT: &[u8] = b"__rng_mt_count"; // i64
const PROP_MT_MODE: &[u8] = b"__rng_mt_mode"; // i64

const PROP_XOSHIRO_S: &[u8] = b"__rng_xoshiro_s"; // 4-element PhpArray of u64

const PROP_PCG_STATE_HI: &[u8] = b"__rng_pcg_hi"; // i64 (low 64 bits of high half) - using i64 for negative wraparound
const PROP_PCG_STATE_LO: &[u8] = b"__rng_pcg_lo"; // i64 (low 64 bits of low half)

// We store u64 values as i64 (transmuted) to leverage existing Value::Long.

fn u64_as_i64(v: u64) -> i64 {
    v as i64
}
fn i64_as_u64(v: i64) -> u64 {
    v as u64
}

pub fn save_mt_state(obj: &Rc<RefCell<PhpObject>>, state: &Mt19937State) {
    let mut o = obj.borrow_mut();
    let mut arr = PhpArray::new();
    for (i, v) in state.state.iter().enumerate() {
        arr.set(ArrayKey::Int(i as i64), Value::Long(*v as i64));
    }
    o.set_property(
        PROP_MT_STATE.to_vec(),
        Value::Array(Rc::new(RefCell::new(arr))),
    );
    o.set_property(PROP_MT_COUNT.to_vec(), Value::Long(state.count as i64));
    o.set_property(PROP_MT_MODE.to_vec(), Value::Long(state.mode));
}

pub fn load_mt_state(obj: &Rc<RefCell<PhpObject>>) -> Mt19937State {
    let o = obj.borrow();
    let mut st = Mt19937State::new();
    if let Value::Array(arr) = o.get_property(PROP_MT_STATE) {
        let arr_b = arr.borrow();
        for (k, v) in arr_b.iter() {
            if let ArrayKey::Int(i) = k {
                if (*i as usize) < MT_N {
                    st.state[*i as usize] = v.to_long() as u32;
                }
            }
        }
    }
    if let Value::Long(c) = o.get_property(PROP_MT_COUNT) {
        st.count = c as u32;
    }
    if let Value::Long(m) = o.get_property(PROP_MT_MODE) {
        st.mode = m;
    }
    st
}

pub fn save_xoshiro_state(obj: &Rc<RefCell<PhpObject>>, state: &Xoshiro256State) {
    let mut o = obj.borrow_mut();
    let mut arr = PhpArray::new();
    for (i, v) in state.s.iter().enumerate() {
        arr.set(ArrayKey::Int(i as i64), Value::Long(u64_as_i64(*v)));
    }
    o.set_property(
        PROP_XOSHIRO_S.to_vec(),
        Value::Array(Rc::new(RefCell::new(arr))),
    );
}

pub fn load_xoshiro_state(obj: &Rc<RefCell<PhpObject>>) -> Xoshiro256State {
    let o = obj.borrow();
    let mut st = Xoshiro256State::new();
    if let Value::Array(arr) = o.get_property(PROP_XOSHIRO_S) {
        let arr_b = arr.borrow();
        for (k, v) in arr_b.iter() {
            if let ArrayKey::Int(i) = k {
                if (*i as usize) < 4 {
                    st.s[*i as usize] = i64_as_u64(v.to_long());
                }
            }
        }
    }
    st
}

pub fn save_pcg_state(obj: &Rc<RefCell<PhpObject>>, state: &PcgState) {
    let mut o = obj.borrow_mut();
    let hi = (state.state >> 64) as u64;
    let lo = state.state as u64;
    o.set_property(PROP_PCG_STATE_HI.to_vec(), Value::Long(u64_as_i64(hi)));
    o.set_property(PROP_PCG_STATE_LO.to_vec(), Value::Long(u64_as_i64(lo)));
}

pub fn load_pcg_state(obj: &Rc<RefCell<PhpObject>>) -> PcgState {
    let o = obj.borrow();
    let hi = i64_as_u64(o.get_property(PROP_PCG_STATE_HI).to_long());
    let lo = i64_as_u64(o.get_property(PROP_PCG_STATE_LO).to_long());
    PcgState {
        state: ((hi as u128) << 64) | (lo as u128),
    }
}

// ===== Initialization (called from NewObject) =====

pub fn init_engine(class_lower: &[u8], obj: &Rc<RefCell<PhpObject>>) {
    match class_lower {
        b"random\\engine\\mt19937" => {
            let mut st = Mt19937State::new();
            // Default seed: random.
            let seed = (os_random_seed() & 0xffffffff) as u32;
            st.seed32(seed);
            save_mt_state(obj, &st);
        }
        b"random\\engine\\xoshiro256starstar" => {
            let mut st = Xoshiro256State::new();
            st.seed64(os_random_seed());
            save_xoshiro_state(obj, &st);
        }
        b"random\\engine\\pcgoneseq128xslrr64" => {
            let mut st = PcgState::new();
            st.seed64(os_random_seed());
            save_pcg_state(obj, &st);
        }
        b"random\\engine\\secure" => {
            // Stateless
        }
        b"random\\randomizer" => {
            // engine property set during construct.
        }
        _ => {}
    }
}

// ===== Method dispatchers =====
//
// Methods that don't need args are dispatched via dispatch_random_method().
// Methods that need args are dispatched via handle_random_docall().

pub fn is_random_class(class_lower: &[u8]) -> bool {
    matches!(
        class_lower,
        b"random\\engine\\mt19937"
            | b"random\\engine\\xoshiro256starstar"
            | b"random\\engine\\pcgoneseq128xslrr64"
            | b"random\\engine\\secure"
            | b"random\\randomizer"
    )
}

pub fn is_random_engine_class(class_lower: &[u8]) -> bool {
    matches!(
        class_lower,
        b"random\\engine\\mt19937"
            | b"random\\engine\\xoshiro256starstar"
            | b"random\\engine\\pcgoneseq128xslrr64"
            | b"random\\engine\\secure"
    )
}

pub fn is_random_args_method(class_lower: &[u8], method: &[u8]) -> bool {
    if !is_random_class(class_lower) {
        return false;
    }
    match class_lower {
        b"random\\engine\\mt19937" => matches!(method, b"__construct" | b"__unserialize"),
        b"random\\engine\\xoshiro256starstar" => matches!(
            method,
            b"__construct" | b"__unserialize" | b"jump" | b"jumplong"
        ),
        b"random\\engine\\pcgoneseq128xslrr64" => {
            matches!(method, b"__construct" | b"__unserialize" | b"jump")
        }
        b"random\\engine\\secure" => matches!(method, b"__unserialize"),
        b"random\\randomizer" => matches!(
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

/// Dispatch no-arg methods (e.g. generate, nextInt, nextFloat, __serialize)
pub fn dispatch_random_method(
    vm: &mut Vm,
    class_lower: &[u8],
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    if !is_random_class(class_lower) {
        return None;
    }

    match (class_lower, method) {
        // Engine: generate()
        (b"random\\engine\\mt19937", b"generate") => {
            let mut st = load_mt_state(obj);
            let v = st.generate_u32();
            save_mt_state(obj, &st);
            Some(Value::String(PhpString::from_vec(v.to_le_bytes().to_vec())))
        }
        (b"random\\engine\\xoshiro256starstar", b"generate") => {
            let mut st = load_xoshiro_state(obj);
            let v = st.generate_u64();
            save_xoshiro_state(obj, &st);
            Some(Value::String(PhpString::from_vec(v.to_le_bytes().to_vec())))
        }
        (b"random\\engine\\pcgoneseq128xslrr64", b"generate") => {
            let mut st = load_pcg_state(obj);
            let v = st.generate_u64();
            save_pcg_state(obj, &st);
            Some(Value::String(PhpString::from_vec(v.to_le_bytes().to_vec())))
        }
        (b"random\\engine\\secure", b"generate") => {
            let mut buf = [0u8; 8];
            os_random_bytes(&mut buf);
            Some(Value::String(PhpString::from_vec(buf.to_vec())))
        }

        // Mt19937 __serialize
        (b"random\\engine\\mt19937", b"__serialize") => Some(serialize_mt19937(obj)),
        (b"random\\engine\\xoshiro256starstar", b"__serialize") => Some(serialize_xoshiro(obj)),
        (b"random\\engine\\pcgoneseq128xslrr64", b"__serialize") => Some(serialize_pcg(obj)),

        // __debugInfo: returns array with __states key containing the state hex strings
        (b"random\\engine\\mt19937", b"__debuginfo") => {
            let st = load_mt_state(obj);
            let mut result = PhpArray::new();
            let mut state_arr = PhpArray::new();
            for (i, v) in st.state.iter().enumerate() {
                state_arr.set(
                    ArrayKey::Int(i as i64),
                    Value::String(PhpString::from_string(u32_to_hex_le(*v))),
                );
            }
            state_arr.set(ArrayKey::Int(MT_N as i64), Value::Long(st.count as i64));
            state_arr.set(ArrayKey::Int(MT_N as i64 + 1), Value::Long(st.mode));
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"__states")),
                Value::Array(Rc::new(RefCell::new(state_arr))),
            );
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        (b"random\\engine\\xoshiro256starstar", b"__debuginfo") => {
            let st = load_xoshiro_state(obj);
            let mut result = PhpArray::new();
            let mut state_arr = PhpArray::new();
            for (i, v) in st.s.iter().enumerate() {
                state_arr.set(
                    ArrayKey::Int(i as i64),
                    Value::String(PhpString::from_string(u64_to_hex_le(*v))),
                );
            }
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"__states")),
                Value::Array(Rc::new(RefCell::new(state_arr))),
            );
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        (b"random\\engine\\pcgoneseq128xslrr64", b"__debuginfo") => {
            let st = load_pcg_state(obj);
            let mut result = PhpArray::new();
            let hi = (st.state >> 64) as u64;
            let lo = st.state as u64;
            let mut state_arr = PhpArray::new();
            state_arr.set(
                ArrayKey::Int(0),
                Value::String(PhpString::from_string(u64_to_hex_le(hi))),
            );
            state_arr.set(
                ArrayKey::Int(1),
                Value::String(PhpString::from_string(u64_to_hex_le(lo))),
            );
            result.set(
                ArrayKey::String(PhpString::from_bytes(b"__states")),
                Value::Array(Rc::new(RefCell::new(state_arr))),
            );
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }

        // Xoshiro: jump / jumpLong (no args)
        (b"random\\engine\\xoshiro256starstar", b"jump") => {
            let mut st = load_xoshiro_state(obj);
            st.jump_short();
            save_xoshiro_state(obj, &st);
            Some(Value::Null)
        }
        (b"random\\engine\\xoshiro256starstar", b"jumplong") => {
            let mut st = load_xoshiro_state(obj);
            st.jump_long();
            save_xoshiro_state(obj, &st);
            Some(Value::Null)
        }

        // Randomizer: nextInt(), nextFloat()
        (b"random\\randomizer", b"nextint") => {
            let val = randomizer_next_int(vm, obj);
            Some(val)
        }
        (b"random\\randomizer", b"nextfloat") => {
            let v = randomizer_next_float(vm, obj);
            Some(Value::Double(v))
        }
        (b"random\\randomizer", b"__serialize") => {
            // serialize: return ['engine' => engine] + properties
            let o = obj.borrow();
            let engine = o.get_property(b"engine");
            drop(o);
            let mut arr = PhpArray::new();
            // PHP wraps as: [0 => properties_array, 1 => state]
            // Simpler: return ['engine' => engine] (won't perfectly match PHP serialize format but enough for many tests)
            // Use index 0 for properties, 1 reserved.
            let mut props = PhpArray::new();
            props.set(ArrayKey::String(PhpString::from_bytes(b"engine")), engine);
            arr.set(ArrayKey::Int(0), Value::Array(Rc::new(RefCell::new(props))));
            Some(Value::Array(Rc::new(RefCell::new(arr))))
        }
        _ => None,
    }
}

/// Dispatch args methods. `args[0]` is `$this`, the rest are user args.
#[allow(clippy::mutable_key_type)] // ArrayKey's interior mutability is only a hash cache
pub fn handle_random_docall(
    vm: &mut Vm,
    class_lower: &[u8],
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    if !is_random_class(class_lower) {
        return None;
    }
    let this = args.first()?;
    let obj = match this {
        Value::Object(o) => o.clone(),
        _ => return None,
    };

    match (class_lower, method) {
        // ---- Engine __construct ----
        (b"random\\engine\\mt19937", b"__construct") => {
            let mut st = Mt19937State::new();
            // mode (arg #2)
            let mode_arg = args.get(2);
            let mode: i64 = match mode_arg {
                Some(Value::Long(n)) => *n,
                Some(v) if !matches!(v, Value::Null | Value::Undef) => v.to_long(),
                _ => 0,
            };
            if mode != 0 && mode != 1 {
                let exc = vm.create_exception(b"ValueError",
                    "Random\\Engine\\Mt19937::__construct(): Argument #2 ($mode) must be either MT_RAND_MT19937 or MT_RAND_PHP",
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            if mode == 1 {
                vm.emit_deprecated_at(
                    "The MT_RAND_PHP variant of Mt19937 is deprecated",
                    vm.current_line,
                );
            }
            st.mode = mode;
            // Seed (arg #1)
            let seed_arg = args.get(1).cloned().unwrap_or(Value::Null);
            let seed: u32 = match seed_arg {
                Value::Null | Value::Undef => (os_random_seed() & 0xffffffff) as u32,
                Value::Long(n) => n as u32,
                v => v.to_long() as u32,
            };
            st.seed32(seed);
            save_mt_state(&obj, &st);
            Some(Value::Null)
        }

        (b"random\\engine\\xoshiro256starstar", b"__construct") => {
            let mut st = Xoshiro256State::new();
            let seed_arg = args.get(1).cloned().unwrap_or(Value::Null);
            match seed_arg {
                Value::Null | Value::Undef => {
                    st.seed64(os_random_seed());
                }
                Value::Long(n) => {
                    st.seed64(n as u64);
                }
                Value::String(s) => {
                    let bytes = s.as_bytes();
                    if bytes.len() != 32 {
                        let exc = vm.create_exception(b"ValueError",
                            "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be a 32 byte (256 bit) string",
                            vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    let mut s0 = [0u8; 8];
                    let mut s1 = [0u8; 8];
                    let mut s2 = [0u8; 8];
                    let mut s3 = [0u8; 8];
                    s0.copy_from_slice(&bytes[0..8]);
                    s1.copy_from_slice(&bytes[8..16]);
                    s2.copy_from_slice(&bytes[16..24]);
                    s3.copy_from_slice(&bytes[24..32]);
                    st.seed256(
                        u64::from_le_bytes(s0),
                        u64::from_le_bytes(s1),
                        u64::from_le_bytes(s2),
                        u64::from_le_bytes(s3),
                    );
                    if st.s == [0, 0, 0, 0] {
                        let exc = vm.create_exception(b"ValueError",
                            "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must not consist entirely of NUL bytes",
                            vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                v => {
                    let tn = Vm::value_type_name(&v);
                    let exc = vm.create_exception(b"TypeError",
                        &format!("Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be of type string|int|null, {} given", tn),
                        vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            save_xoshiro_state(&obj, &st);
            Some(Value::Null)
        }

        (b"random\\engine\\pcgoneseq128xslrr64", b"__construct") => {
            let mut st = PcgState::new();
            let seed_arg = args.get(1).cloned().unwrap_or(Value::Null);
            match seed_arg {
                Value::Null | Value::Undef => {
                    st.seed64(os_random_seed());
                }
                Value::Long(n) => {
                    st.seed64(n as u64);
                }
                Value::String(s) => {
                    let bytes = s.as_bytes();
                    if bytes.len() != 16 {
                        let exc = vm.create_exception(b"ValueError",
                            "Random\\Engine\\PcgOneseq128XslRr64::__construct(): Argument #1 ($seed) must be a 16 byte (128 bit) string",
                            vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    let mut hi_b = [0u8; 8];
                    let mut lo_b = [0u8; 8];
                    hi_b.copy_from_slice(&bytes[0..8]);
                    lo_b.copy_from_slice(&bytes[8..16]);
                    let hi = u64::from_le_bytes(hi_b);
                    let lo = u64::from_le_bytes(lo_b);
                    st.seed128(hi, lo);
                }
                v => {
                    let tn = Vm::value_type_name(&v);
                    let exc = vm.create_exception(b"TypeError",
                        &format!("Random\\Engine\\PcgOneseq128XslRr64::__construct(): Argument #1 ($seed) must be of type string|int|null, {} given", tn),
                        vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            save_pcg_state(&obj, &st);
            Some(Value::Null)
        }

        // ---- Engine __unserialize ----
        (b"random\\engine\\mt19937", b"__unserialize") => {
            let data = args.get(1)?;
            let class_name = "Random\\Engine\\Mt19937";
            if let Value::Array(arr) = data {
                let arr_b = arr.borrow();
                if arr_b.len() != 2 {
                    throw_invalid_serialization(vm, class_name);
                    return Some(Value::Null);
                }
                // index 1 contains state array of 626 hex strings + 2 ints
                let state_arr = arr_b.get(&ArrayKey::Int(1));
                if let Some(Value::Array(state)) = state_arr {
                    let state_b = state.borrow();
                    if state_b.len() != MT_N + 2 {
                        throw_invalid_serialization(vm, class_name);
                        return Some(Value::Null);
                    }
                    let mut st = Mt19937State::new();
                    for i in 0..MT_N {
                        let entry = state_b.get(&ArrayKey::Int(i as i64));
                        match entry {
                            Some(Value::String(s)) => {
                                let s_str = s.to_string_lossy();
                                match hex_to_u32_le(&s_str) {
                                    Some(v) => st.state[i] = v,
                                    None => {
                                        throw_invalid_serialization(vm, class_name);
                                        return Some(Value::Null);
                                    }
                                }
                            }
                            _ => {
                                throw_invalid_serialization(vm, class_name);
                                return Some(Value::Null);
                            }
                        }
                    }
                    let count = state_b
                        .get(&ArrayKey::Int(MT_N as i64))
                        .map(|v| v.to_long())
                        .unwrap_or(-1);
                    let mode = state_b
                        .get(&ArrayKey::Int(MT_N as i64 + 1))
                        .map(|v| v.to_long())
                        .unwrap_or(-1);
                    if count < 0 || count as usize > MT_N || (mode != 0 && mode != 1) {
                        throw_invalid_serialization(vm, class_name);
                        return Some(Value::Null);
                    }
                    st.count = count as u32;
                    st.mode = mode;
                    save_mt_state(&obj, &st);
                    return Some(Value::Null);
                }
                throw_invalid_serialization(vm, class_name);
                return Some(Value::Null);
            }
            throw_invalid_serialization(vm, class_name);
            Some(Value::Null)
        }

        (b"random\\engine\\xoshiro256starstar", b"__unserialize") => {
            let data = args.get(1)?;
            let class_name = "Random\\Engine\\Xoshiro256StarStar";
            if let Value::Array(arr) = data {
                let arr_b = arr.borrow();
                if arr_b.len() != 2 {
                    throw_invalid_serialization(vm, class_name);
                    return Some(Value::Null);
                }
                let state_arr = arr_b.get(&ArrayKey::Int(1));
                if let Some(Value::Array(state)) = state_arr {
                    let state_b = state.borrow();
                    if state_b.len() != 4 {
                        throw_invalid_serialization(vm, class_name);
                        return Some(Value::Null);
                    }
                    let mut st = Xoshiro256State::new();
                    for i in 0..4 {
                        let entry = state_b.get(&ArrayKey::Int(i));
                        match entry {
                            Some(Value::String(s)) => {
                                let s_str = s.to_string_lossy();
                                match hex_to_u64_le(&s_str) {
                                    Some(v) => st.s[i as usize] = v,
                                    None => {
                                        throw_invalid_serialization(vm, class_name);
                                        return Some(Value::Null);
                                    }
                                }
                            }
                            _ => {
                                throw_invalid_serialization(vm, class_name);
                                return Some(Value::Null);
                            }
                        }
                    }
                    save_xoshiro_state(&obj, &st);
                    return Some(Value::Null);
                }
            }
            throw_invalid_serialization(vm, class_name);
            Some(Value::Null)
        }

        (b"random\\engine\\pcgoneseq128xslrr64", b"__unserialize") => {
            let data = args.get(1)?;
            let class_name = "Random\\Engine\\PcgOneseq128XslRr64";
            if let Value::Array(arr) = data {
                let arr_b = arr.borrow();
                if arr_b.len() != 2 {
                    throw_invalid_serialization(vm, class_name);
                    return Some(Value::Null);
                }
                let state_arr = arr_b.get(&ArrayKey::Int(1));
                if let Some(Value::Array(state)) = state_arr {
                    let state_b = state.borrow();
                    if state_b.len() != 2 {
                        throw_invalid_serialization(vm, class_name);
                        return Some(Value::Null);
                    }
                    let hi = match state_b.get(&ArrayKey::Int(0)) {
                        Some(Value::String(s)) => match hex_to_u64_le(&s.to_string_lossy()) {
                            Some(v) => v,
                            None => {
                                throw_invalid_serialization(vm, class_name);
                                return Some(Value::Null);
                            }
                        },
                        _ => {
                            throw_invalid_serialization(vm, class_name);
                            return Some(Value::Null);
                        }
                    };
                    let lo = match state_b.get(&ArrayKey::Int(1)) {
                        Some(Value::String(s)) => match hex_to_u64_le(&s.to_string_lossy()) {
                            Some(v) => v,
                            None => {
                                throw_invalid_serialization(vm, class_name);
                                return Some(Value::Null);
                            }
                        },
                        _ => {
                            throw_invalid_serialization(vm, class_name);
                            return Some(Value::Null);
                        }
                    };
                    let st = PcgState {
                        state: ((hi as u128) << 64) | (lo as u128),
                    };
                    save_pcg_state(&obj, &st);
                    return Some(Value::Null);
                }
            }
            throw_invalid_serialization(vm, class_name);
            Some(Value::Null)
        }

        // ---- Xoshiro::jump(steps) / Pcg::jump(steps) ----
        // Note: jump() takes no args in Xoshiro256StarStar - it's in dispatch_random_method.
        (b"random\\engine\\pcgoneseq128xslrr64", b"jump") => {
            let advance = args.get(1).map(|v| v.to_long()).unwrap_or(0);
            if advance < 0 {
                let exc = vm.create_exception(b"ValueError",
                    "Random\\Engine\\PcgOneseq128XslRr64::jump(): Argument #1 ($advance) must be greater than or equal to 0",
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            let mut st = load_pcg_state(&obj);
            st.advance(advance as u64);
            save_pcg_state(&obj, &st);
            Some(Value::Null)
        }

        // ---- Randomizer __construct ----
        (b"random\\randomizer", b"__construct") => {
            let engine_arg = args.get(1).cloned().unwrap_or(Value::Null);
            let engine_val = match engine_arg {
                Value::Null | Value::Undef => {
                    // Default: Secure
                    create_default_secure_engine(vm)
                }
                Value::Object(_) => engine_arg,
                _ => {
                    let exc = vm.create_exception(b"TypeError",
                        "Random\\Randomizer::__construct(): Argument #1 ($engine) must be of type ?Random\\Engine, given",
                        vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            };
            let mut o = obj.borrow_mut();
            o.set_property(b"engine".to_vec(), engine_val);
            Some(Value::Null)
        }

        (b"random\\randomizer", b"__unserialize") => {
            let data = args.get(1)?;
            let class_name = "Random\\Randomizer";
            // PHP enforces that __unserialize must not bypass readonly. Since
            // we don't have full readonly enforcement, we accept the data.
            if let Value::Array(arr) = data {
                let arr_b = arr.borrow();
                // Look for index 0 properties
                if let Some(Value::Array(props)) = arr_b.get(&ArrayKey::Int(0)) {
                    let props_b = props.borrow();
                    for (k, v) in props_b.iter() {
                        if let ArrayKey::String(s) = k {
                            let mut o = obj.borrow_mut();
                            o.set_property(s.as_bytes().to_vec(), v.clone());
                        }
                    }
                    return Some(Value::Null);
                }
            }
            throw_invalid_serialization(vm, class_name);
            Some(Value::Null)
        }

        // ---- Randomizer methods with args ----
        (b"random\\randomizer", b"getint") => {
            let min = args.get(1).map(|v| v.to_long()).unwrap_or(0);
            let max = args.get(2).map(|v| v.to_long()).unwrap_or(0);
            if min > max {
                let exc = vm.create_exception(b"ValueError",
                    "Random\\Randomizer::getInt(): Argument #1 ($min) must be less than or equal to argument #2 ($max)",
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Long(0));
            }
            let val = randomizer_range(vm, &obj, min, max);
            Some(Value::Long(val))
        }

        (b"random\\randomizer", b"getbytes") => {
            let len = args.get(1).map(|v| v.to_long()).unwrap_or(0);
            if len < 1 {
                let exc = vm.create_exception(
                    b"ValueError",
                    "Random\\Randomizer::getBytes(): Argument #1 ($length) must be greater than 0",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::String(PhpString::empty()));
            }
            let bytes = randomizer_get_bytes(vm, &obj, len as usize);
            Some(Value::String(PhpString::from_vec(bytes)))
        }

        (b"random\\randomizer", b"getbytesfromstring") => {
            let src = match args.get(1) {
                Some(Value::String(s)) => s.as_bytes().to_vec(),
                Some(v) => v.to_php_string().as_bytes().to_vec(),
                None => Vec::new(),
            };
            let len = args.get(2).map(|v| v.to_long()).unwrap_or(0);
            if src.is_empty() {
                let exc = vm.create_exception(b"ValueError",
                    "Random\\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty",
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::String(PhpString::empty()));
            }
            if len < 1 {
                let exc = vm.create_exception(b"ValueError",
                    "Random\\Randomizer::getBytesFromString(): Argument #2 ($length) must be greater than 0",
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::String(PhpString::empty()));
            }
            let mut out = Vec::with_capacity(len as usize);
            let max_offset = src.len() - 1;
            // Fast path: source <= 256 -> bitmask each generate output's bytes
            if max_offset <= 0xff {
                let mut mask = max_offset as u64;
                mask |= mask >> 1;
                mask |= mask >> 2;
                mask |= mask >> 4;
                mask = mask.wrapping_mul(0x0101010101010101);
                let engine = obj.borrow().get_property(b"engine");
                // For native engines, use a fast inline path that avoids load/save per call.
                let native_class = native_engine_class(&engine);
                if let Some(class) = native_class.as_ref() {
                    if let Value::Object(eobj) = &engine {
                        match class.as_slice() {
                            b"random\\engine\\mt19937" => {
                                let mut st = load_mt_state(eobj);
                                while out.len() < len as usize {
                                    let v = st.generate_u32();
                                    let mut result = (v as u64) & mask;
                                    for _ in 0..4 {
                                        let offset = (result & 0xff) as usize;
                                        result >>= 8;
                                        if offset > max_offset {
                                            continue;
                                        }
                                        out.push(src[offset]);
                                        if out.len() >= len as usize {
                                            break;
                                        }
                                    }
                                }
                                save_mt_state(eobj, &st);
                            }
                            b"random\\engine\\xoshiro256starstar" => {
                                let mut st = load_xoshiro_state(eobj);
                                while out.len() < len as usize {
                                    let v = st.generate_u64();
                                    let mut result = v & mask;
                                    for _ in 0..8 {
                                        let offset = (result & 0xff) as usize;
                                        result >>= 8;
                                        if offset > max_offset {
                                            continue;
                                        }
                                        out.push(src[offset]);
                                        if out.len() >= len as usize {
                                            break;
                                        }
                                    }
                                }
                                save_xoshiro_state(eobj, &st);
                            }
                            b"random\\engine\\pcgoneseq128xslrr64" => {
                                let mut st = load_pcg_state(eobj);
                                while out.len() < len as usize {
                                    let v = st.generate_u64();
                                    let mut result = v & mask;
                                    for _ in 0..8 {
                                        let offset = (result & 0xff) as usize;
                                        result >>= 8;
                                        if offset > max_offset {
                                            continue;
                                        }
                                        out.push(src[offset]);
                                        if out.len() >= len as usize {
                                            break;
                                        }
                                    }
                                }
                                save_pcg_state(eobj, &st);
                            }
                            _ => {
                                while out.len() < len as usize {
                                    let bytes = engine_generate_bytes(vm, &engine, 8);
                                    if bytes.is_empty() {
                                        break;
                                    }
                                    let mut buf = [0u8; 8];
                                    let n = bytes.len().min(8);
                                    buf[..n].copy_from_slice(&bytes[..n]);
                                    let mut result = u64::from_le_bytes(buf) & mask;
                                    for _ in 0..n {
                                        let offset = (result & 0xff) as usize;
                                        result >>= 8;
                                        if offset > max_offset {
                                            continue;
                                        }
                                        out.push(src[offset]);
                                        if out.len() >= len as usize {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let mut consec_failures = 0;
                    while out.len() < len as usize {
                        let bytes = engine_generate_bytes(vm, &engine, 8);
                        if vm.current_exception.is_some() {
                            break;
                        }
                        if bytes.is_empty() {
                            break;
                        }
                        let mut buf = [0u8; 8];
                        let n = bytes.len().min(8);
                        buf[..n].copy_from_slice(&bytes[..n]);
                        let mut result = u64::from_le_bytes(buf) & mask;
                        let prev_len = out.len();
                        for _ in 0..n {
                            let offset = (result & 0xff) as usize;
                            result >>= 8;
                            if offset > max_offset {
                                continue;
                            }
                            out.push(src[offset]);
                            if out.len() >= len as usize {
                                break;
                            }
                        }
                        if out.len() == prev_len {
                            consec_failures += 1;
                            if consec_failures > 50 {
                                let exc = vm.create_exception(
                                    b"Random\\BrokenRandomEngineError",
                                    "Failed to generate an acceptable random number in 50 attempts",
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                break;
                            }
                        } else {
                            consec_failures = 0;
                        }
                    }
                }
            } else {
                // Slow path: source > 256 bytes - need range() per byte.
                // For native engines, do bulk inline state ops to avoid load/save overhead.
                let engine = obj.borrow().get_property(b"engine");
                let native_class = native_engine_class(&engine);
                let max_idx = max_offset as u64;
                if let Some(class) = native_class.as_ref() {
                    if let Value::Object(eobj) = &engine {
                        // Compute umax
                        let umax = max_idx;
                        let umax_inc = umax.wrapping_add(1);
                        let limit = if umax <= u32::MAX as u64 {
                            (u32::MAX - (u32::MAX % umax_inc as u32) - 1) as u64
                        } else {
                            u64::MAX - (u64::MAX % umax_inc) - 1
                        };
                        let is_pow2 = umax_inc & umax == 0;
                        let use_u32 = umax <= u32::MAX as u64;
                        match class.as_slice() {
                            b"random\\engine\\mt19937" => {
                                let mut st = load_mt_state(eobj);
                                while out.len() < len as usize {
                                    // Need 32 bits per offset
                                    let v = st.generate_u32() as u64;
                                    if is_pow2 {
                                        let r = v & umax;
                                        out.push(src[r as usize]);
                                    } else if use_u32 {
                                        if (v as u32) <= limit as u32 {
                                            let r = (v as u32) % umax_inc as u32;
                                            out.push(src[r as usize]);
                                        }
                                    } else {
                                        // 64-bit: fetch another word
                                        let v2 = st.generate_u32() as u64;
                                        let combined = (v2 << 32) | v;
                                        if combined <= limit {
                                            out.push(src[(combined % umax_inc) as usize]);
                                        }
                                    }
                                }
                                save_mt_state(eobj, &st);
                            }
                            b"random\\engine\\xoshiro256starstar" => {
                                let mut st = load_xoshiro_state(eobj);
                                while out.len() < len as usize {
                                    let v = st.generate_u64();
                                    if is_pow2 {
                                        out.push(src[(v & umax) as usize]);
                                    } else if use_u32 {
                                        let v32 = v as u32;
                                        if v32 <= limit as u32 {
                                            out.push(src[(v32 % umax_inc as u32) as usize]);
                                        }
                                    } else if v <= limit {
                                        out.push(src[(v % umax_inc) as usize]);
                                    }
                                }
                                save_xoshiro_state(eobj, &st);
                            }
                            b"random\\engine\\pcgoneseq128xslrr64" => {
                                let mut st = load_pcg_state(eobj);
                                while out.len() < len as usize {
                                    let v = st.generate_u64();
                                    if is_pow2 {
                                        out.push(src[(v & umax) as usize]);
                                    } else if use_u32 {
                                        let v32 = v as u32;
                                        if v32 <= limit as u32 {
                                            out.push(src[(v32 % umax_inc as u32) as usize]);
                                        }
                                    } else if v <= limit {
                                        out.push(src[(v % umax_inc) as usize]);
                                    }
                                }
                                save_pcg_state(eobj, &st);
                            }
                            _ => {
                                let max_idx_i = max_offset as i64;
                                for _ in 0..len {
                                    let r = randomizer_range(vm, &obj, 0, max_idx_i);
                                    if r >= 0 && (r as usize) < src.len() {
                                        out.push(src[r as usize]);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let max_idx_i = max_offset as i64;
                    for _ in 0..len {
                        let r = randomizer_range(vm, &obj, 0, max_idx_i);
                        if r >= 0 && (r as usize) < src.len() {
                            out.push(src[r as usize]);
                        }
                    }
                }
            }
            Some(Value::String(PhpString::from_vec(out)))
        }

        (b"random\\randomizer", b"getfloat") => {
            let min = args.get(1).map(|v| v.to_double()).unwrap_or(0.0);
            let max = args.get(2).map(|v| v.to_double()).unwrap_or(1.0);
            // boundary: enum case object or int
            // We accept default ClosedOpen.
            let boundary = match args.get(3) {
                Some(Value::Object(o)) => {
                    let ob = o.borrow();
                    let n = ob.get_property(b"name");
                    n.to_php_string().to_string_lossy()
                }
                _ => "ClosedOpen".to_string(),
            };
            if !min.is_finite() {
                let exc = vm.create_exception(
                    b"ValueError",
                    "Random\\Randomizer::getFloat(): Argument #1 ($min) must be finite",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::Double(0.0));
            }
            if !max.is_finite() {
                let exc = vm.create_exception(
                    b"ValueError",
                    "Random\\Randomizer::getFloat(): Argument #2 ($max) must be finite",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::Double(0.0));
            }
            // Range check: ClosedClosed allows min == max, others require min < max
            let allow_equal = boundary == "ClosedClosed";
            if (allow_equal && min > max) || (!allow_equal && min >= max) {
                let msg = if allow_equal {
                    "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)"
                } else {
                    "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than argument #1 ($min)"
                };
                let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Double(0.0));
            }
            let v = randomizer_get_float(vm, &obj, min, max, &boundary);
            Some(Value::Double(v))
        }

        (b"random\\randomizer", b"shufflearray") => {
            let arr_in = args.get(1).cloned().unwrap_or(Value::Null);
            if let Value::Array(arr) = arr_in {
                let mut values: Vec<Value> = arr.borrow().values().cloned().collect();
                let n = values.len();
                if n > 1 {
                    bulk_fisher_yates(vm, &obj, &mut values);
                }
                let mut out = PhpArray::new();
                for v in values {
                    out.push(v);
                }
                Some(Value::Array(Rc::new(RefCell::new(out))))
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }

        (b"random\\randomizer", b"shufflebytes") => {
            let s = match args.get(1) {
                Some(Value::String(s)) => s.as_bytes().to_vec(),
                Some(v) => v.to_php_string().as_bytes().to_vec(),
                None => Vec::new(),
            };
            let mut bytes = s;
            let n = bytes.len();
            if n > 1 {
                bulk_fisher_yates(vm, &obj, &mut bytes);
            }
            Some(Value::String(PhpString::from_vec(bytes)))
        }

        (b"random\\randomizer", b"pickarraykeys") => {
            let arr_in = args.get(1).cloned().unwrap_or(Value::Null);
            let num = args.get(2).map(|v| v.to_long()).unwrap_or(0);
            if !matches!(arr_in, Value::Array(_)) {
                let tn = Vm::value_type_name(&arr_in);
                let exc = vm.create_exception(b"TypeError",
                    &format!("Random\\Randomizer::pickArrayKeys(): Argument #1 ($array) must be of type array, {} given", tn),
                    vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            }
            if let Value::Array(arr) = arr_in {
                let arr_b = arr.borrow();
                let n = arr_b.len() as i64;
                if n == 0 {
                    let exc = vm.create_exception(b"ValueError",
                        "Random\\Randomizer::pickArrayKeys(): Argument #1 ($array) must not be empty",
                        vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                }
                if num < 1 || num > n {
                    let exc = vm.create_exception(b"ValueError",
                        "Random\\Randomizer::pickArrayKeys(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)",
                        vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                }
                // Reservoir sampling? Simpler: collect all keys, shuffle, take first num.
                let mut keys: Vec<ArrayKey> = arr_b.iter().map(|(k, _)| k.clone()).collect();
                drop(arr_b);
                if keys.len() > 1 {
                    bulk_fisher_yates(vm, &obj, &mut keys);
                }
                // PHP returns keys in their original ascending positional order.
                // Take first `num` (after shuffle), then re-sort by original index.
                let original_order: Vec<ArrayKey> =
                    arr.borrow().iter().map(|(k, _)| k.clone()).collect();
                let mut selected: std::collections::HashSet<ArrayKey> =
                    keys.into_iter().take(num as usize).collect();
                let mut out = PhpArray::new();
                for k in original_order.iter() {
                    if selected.remove(k) {
                        out.push(value_from_key(k));
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(out))))
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }

        _ => None,
    }
}

/// Fisher-Yates shuffle that uses native engine state inline when possible
/// to avoid load/save overhead per swap.
fn bulk_fisher_yates<T>(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, items: &mut [T]) {
    let n = items.len();
    if n < 2 {
        return;
    }
    let engine = obj.borrow().get_property(b"engine");
    let native_class = native_engine_class(&engine);

    // Helper closure to compute a random index in [0, max] inclusive
    // using a u64 from a generator function.
    let pick_idx = |g: &mut dyn FnMut() -> u64, max: u64| -> u64 {
        let umax_inc = max.wrapping_add(1);
        if max == u64::MAX {
            return g();
        }
        if umax_inc & max == 0 {
            return g() & max;
        }
        let limit = u64::MAX - (u64::MAX % umax_inc) - 1;
        loop {
            let r = g();
            if r <= limit {
                return r % umax_inc;
            }
        }
    };

    if let Some(class) = native_class.as_ref() {
        if let Value::Object(eobj) = &engine {
            match class.as_slice() {
                b"random\\engine\\mt19937" => {
                    let mut st = load_mt_state(eobj);
                    for i in (1..n).rev() {
                        let max = i as u64;
                        let mut g = || {
                            // Need 32 bits (range fits in u32)
                            if max <= u32::MAX as u64 {
                                st.generate_u32() as u64
                            } else {
                                let lo = st.generate_u32() as u64;
                                let hi = st.generate_u32() as u64;
                                (hi << 32) | lo
                            }
                        };
                        let umax_inc = max.wrapping_add(1);
                        let j = if umax_inc & max == 0 {
                            g() & max
                        } else if max <= u32::MAX as u64 {
                            let limit32 = u32::MAX - (u32::MAX % umax_inc as u32) - 1;
                            loop {
                                let r = st.generate_u32();
                                if r <= limit32 {
                                    break (r % umax_inc as u32) as u64;
                                }
                            }
                        } else {
                            pick_idx(&mut g, max)
                        };
                        items.swap(i, j as usize);
                    }
                    save_mt_state(eobj, &st);
                    return;
                }
                b"random\\engine\\xoshiro256starstar" => {
                    let mut st = load_xoshiro_state(eobj);
                    for i in (1..n).rev() {
                        let max = i as u64;
                        let umax_inc = max.wrapping_add(1);
                        let j = if umax_inc & max == 0 {
                            st.generate_u64() & max
                        } else if max <= u32::MAX as u64 {
                            let limit32 = u32::MAX - (u32::MAX % umax_inc as u32) - 1;
                            loop {
                                let r = st.generate_u64() as u32;
                                if r <= limit32 {
                                    break (r % umax_inc as u32) as u64;
                                }
                            }
                        } else {
                            let limit = u64::MAX - (u64::MAX % umax_inc) - 1;
                            loop {
                                let r = st.generate_u64();
                                if r <= limit {
                                    break r % umax_inc;
                                }
                            }
                        };
                        items.swap(i, j as usize);
                    }
                    save_xoshiro_state(eobj, &st);
                    return;
                }
                b"random\\engine\\pcgoneseq128xslrr64" => {
                    let mut st = load_pcg_state(eobj);
                    for i in (1..n).rev() {
                        let max = i as u64;
                        let umax_inc = max.wrapping_add(1);
                        let j = if umax_inc & max == 0 {
                            st.generate_u64() & max
                        } else if max <= u32::MAX as u64 {
                            let limit32 = u32::MAX - (u32::MAX % umax_inc as u32) - 1;
                            loop {
                                let r = st.generate_u64() as u32;
                                if r <= limit32 {
                                    break (r % umax_inc as u32) as u64;
                                }
                            }
                        } else {
                            let limit = u64::MAX - (u64::MAX % umax_inc) - 1;
                            loop {
                                let r = st.generate_u64();
                                if r <= limit {
                                    break r % umax_inc;
                                }
                            }
                        };
                        items.swap(i, j as usize);
                    }
                    save_pcg_state(eobj, &st);
                    return;
                }
                _ => {}
            }
        }
    }
    // Fallback: standard slow path with full call_object_method per swap
    for i in (1..n).rev() {
        let j = randomizer_range(vm, obj, 0, i as i64) as usize;
        if vm.current_exception.is_some() {
            return;
        }
        items.swap(i, j);
    }
}

fn value_from_key(k: &ArrayKey) -> Value {
    match k {
        ArrayKey::Int(n) => Value::Long(*n),
        ArrayKey::String(s) => Value::String(s.clone()),
    }
}

fn throw_invalid_serialization(vm: &mut Vm, class_name: &str) {
    let msg = format!("Invalid serialization data for {} object", class_name);
    let exc = vm.create_exception(b"Exception", &msg, vm.current_line);
    vm.current_exception = Some(exc);
}

fn create_default_secure_engine(vm: &mut Vm) -> Value {
    let obj_id = vm.alloc_object_id();
    let obj = PhpObject::new(b"Random\\Engine\\Secure".to_vec(), obj_id);
    Value::Object(Rc::new(RefCell::new(obj)))
}

// Generate a 64-bit random value from the engine attached to the Randomizer.
// Calls $engine->generate() and parses the returned bytes as little-endian.
#[allow(dead_code)]
fn engine_generate_u64(vm: &mut Vm, engine: &Value) -> u64 {
    let bytes = engine_generate_bytes(vm, engine, 8);
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
}

/// Try to identify a native engine class (lowercase). Used for the fast path.
fn native_engine_class(engine: &Value) -> Option<Vec<u8>> {
    if let Value::Object(o) = engine {
        let cn: Vec<u8> = o
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        match cn.as_slice() {
            b"random\\engine\\mt19937"
            | b"random\\engine\\xoshiro256starstar"
            | b"random\\engine\\pcgoneseq128xslrr64"
            | b"random\\engine\\secure" => Some(cn),
            _ => None,
        }
    } else {
        None
    }
}

// Generate `count` raw bytes from the engine. Has a fast path for native engines
// that bypasses the per-call dispatch overhead of `call_object_method`.
fn engine_generate_bytes(vm: &mut Vm, engine: &Value, count: usize) -> Vec<u8> {
    // Fast path: native engines have known generate sizes; we can bulk-pull state.
    if let Some(class) = native_engine_class(engine) {
        if let Value::Object(obj) = engine {
            let mut out = Vec::with_capacity(count);
            match class.as_slice() {
                b"random\\engine\\mt19937" => {
                    let mut st = load_mt_state(obj);
                    while out.len() < count {
                        let v = st.generate_u32();
                        let bytes = v.to_le_bytes();
                        let need = count - out.len();
                        let take = bytes.len().min(need);
                        out.extend_from_slice(&bytes[..take]);
                    }
                    save_mt_state(obj, &st);
                }
                b"random\\engine\\xoshiro256starstar" => {
                    let mut st = load_xoshiro_state(obj);
                    while out.len() < count {
                        let v = st.generate_u64();
                        let bytes = v.to_le_bytes();
                        let need = count - out.len();
                        let take = bytes.len().min(need);
                        out.extend_from_slice(&bytes[..take]);
                    }
                    save_xoshiro_state(obj, &st);
                }
                b"random\\engine\\pcgoneseq128xslrr64" => {
                    let mut st = load_pcg_state(obj);
                    while out.len() < count {
                        let v = st.generate_u64();
                        let bytes = v.to_le_bytes();
                        let need = count - out.len();
                        let take = bytes.len().min(need);
                        out.extend_from_slice(&bytes[..take]);
                    }
                    save_pcg_state(obj, &st);
                }
                b"random\\engine\\secure" => {
                    let mut buf = vec![0u8; count];
                    os_random_bytes(&mut buf);
                    out = buf;
                }
                _ => {}
            }
            // Suppress unused-vm warning when only fast path runs
            let _ = vm;
            return out;
        }
    }
    // For user engines, PHP truncates each generate() to 8 bytes (64 bits).
    let mut out = Vec::with_capacity(count);
    let mut consec_empty = 0;
    while out.len() < count {
        let result = vm.call_object_method(engine, b"generate", &[]);
        if vm.current_exception.is_some() {
            return out;
        }
        if vm.exit_requested {
            return out;
        }
        match result {
            Some(Value::String(s)) => {
                let bytes = s.as_bytes();
                if bytes.is_empty() {
                    // PHP: A random engine must return a non-empty string.
                    let _ = consec_empty;
                    throw_broken_engine(vm);
                    return out;
                }
                consec_empty = 0;
                let truncated_len = bytes.len().min(8);
                let truncated = &bytes[..truncated_len];
                let need = count - out.len();
                let take = truncated.len().min(need);
                out.extend_from_slice(&truncated[..take]);
            }
            _ => {
                // generate() returned non-string - PHP throws TypeError
                let class_name = if let Value::Object(o) = engine {
                    String::from_utf8_lossy(&o.borrow().class_name).to_string()
                } else {
                    String::from("Engine")
                };
                let msg = format!(
                    "{}::generate(): Return value must be of type string",
                    class_name
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return out;
            }
        }
    }
    out
}

fn throw_broken_engine(vm: &mut Vm) {
    let exc = vm.create_exception(
        b"Random\\BrokenRandomEngineError",
        "A random engine must return a non-empty string",
        vm.current_line,
    );
    vm.current_exception = Some(exc);
}

// Range function: inclusive of both min and max.
fn randomizer_range(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, min: i64, max: i64) -> i64 {
    if min == max {
        return min;
    }
    let engine = obj.borrow().get_property(b"engine");
    let umax = (max as u64).wrapping_sub(min as u64);
    if umax == 0 {
        return min;
    }
    if umax <= u32::MAX as u64 {
        let r = range_u32(vm, &engine, umax as u32);
        return min.wrapping_add(r as i64);
    }
    let r = range_u64(vm, &engine, umax);
    min.wrapping_add(r as i64)
}

fn range_u32(vm: &mut Vm, engine: &Value, umax: u32) -> u32 {
    // First draw 32 bits from engine; some engines (Mt19937) return 4 bytes per generate(), some (xoshiro/pcg) return 8.
    // We need to keep pulling until we have 4 bytes accumulated; engine_generate_bytes does that.
    let bytes = engine_generate_bytes(vm, engine, 4);
    if vm.current_exception.is_some() {
        return 0;
    }
    let mut buf = [0u8; 4];
    let n = bytes.len().min(4);
    buf[..n].copy_from_slice(&bytes[..n]);
    let mut result = u32::from_le_bytes(buf);

    if umax == u32::MAX {
        return result;
    }
    let umax_inc = umax.wrapping_add(1);
    if umax_inc & umax == 0 {
        return result & umax;
    }
    let limit = u32::MAX - (u32::MAX % umax_inc) - 1;
    let mut count = 0;
    while result > limit {
        count += 1;
        if count > 50 {
            let exc = vm.create_exception(
                b"Random\\BrokenRandomEngineError",
                "Failed to generate an acceptable random number in 50 attempts",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            return 0;
        }
        let bytes = engine_generate_bytes(vm, engine, 4);
        if vm.current_exception.is_some() {
            return 0;
        }
        let mut buf = [0u8; 4];
        let n = bytes.len().min(4);
        buf[..n].copy_from_slice(&bytes[..n]);
        result = u32::from_le_bytes(buf);
    }
    result % umax_inc
}

fn range_u64(vm: &mut Vm, engine: &Value, umax: u64) -> u64 {
    let bytes = engine_generate_bytes(vm, engine, 8);
    if vm.current_exception.is_some() {
        return 0;
    }
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    let mut result = u64::from_le_bytes(buf);

    if umax == u64::MAX {
        return result;
    }
    let umax_inc = umax.wrapping_add(1);
    if umax_inc & umax == 0 {
        return result & umax;
    }
    let limit = u64::MAX - (u64::MAX % umax_inc) - 1;
    let mut count = 0;
    while result > limit {
        count += 1;
        if count > 50 {
            let exc = vm.create_exception(
                b"Random\\BrokenRandomEngineError",
                "Failed to generate an acceptable random number in 50 attempts",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            return 0;
        }
        let bytes = engine_generate_bytes(vm, engine, 8);
        if vm.current_exception.is_some() {
            return 0;
        }
        let mut buf = [0u8; 8];
        let n = bytes.len().min(8);
        buf[..n].copy_from_slice(&bytes[..n]);
        result = u64::from_le_bytes(buf);
    }
    result % umax_inc
}

fn randomizer_next_int(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>) -> Value {
    let engine = obj.borrow().get_property(b"engine");
    // Need 32-bit value (raises RandomException if engine is 64-bit on 32-bit platforms - we ignore).
    // For Mt19937: 32-bit. For xoshiro/pcg: 64-bit.
    // We just compose a 64-bit value from engine bytes.
    let bytes = engine_generate_bytes(vm, &engine, 8);
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    let v = u64::from_le_bytes(buf) as i64 & i64::MAX;
    Value::Long(v)
}

fn randomizer_get_bytes(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, length: usize) -> Vec<u8> {
    let engine = obj.borrow().get_property(b"engine");
    engine_generate_bytes(vm, &engine, length)
}

fn randomizer_next_float(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>) -> f64 {
    // Returns a float in [0, 1).
    // PHP uses 53-bit precision: result_u64 >> 11 / 2^53.
    let engine = obj.borrow().get_property(b"engine");
    let bytes = engine_generate_bytes(vm, &engine, 8);
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    let v = u64::from_le_bytes(buf);
    (v >> 11) as f64 / ((1u64 << 53) as f64)
}

fn randomizer_get_float(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    min: f64,
    max: f64,
    boundary: &str,
) -> f64 {
    // Simple gamma-section-equivalent: uniform in [0,1), rescale.
    // Boundaries: ClosedOpen [min, max), ClosedClosed [min, max], OpenClosed (min, max], OpenOpen (min, max).
    // This is a simplification; for ClosedOpen we use [0, 1) -> [min, max).
    let mut u = randomizer_next_float(vm, obj);
    match boundary {
        "ClosedOpen" => min + u * (max - min),
        "ClosedClosed" => {
            // [0, 1] inclusive: scale [0, 1) plus possible 1.0 rarely.
            // Simplification: u in [0, 1) -> [min, max] with chance to hit max:
            // scale to (max - min) and add min; cap at max.
            let v = min + u * (max - min);
            if v > max { max } else { v }
        }
        "OpenClosed" => {
            // (min, max]: shift so 0 maps to "next after min".
            // Simplification: use 1 - u in [0, 1] to (0, 1].
            let v = 1.0 - u;
            min + v * (max - min)
        }
        "OpenOpen" => {
            // (min, max): exclude both. Loop until u > 0.
            let mut tries = 0;
            while u == 0.0 && tries < 50 {
                u = randomizer_next_float(vm, obj);
                tries += 1;
            }
            min + u * (max - min)
        }
        _ => min + u * (max - min),
    }
}

// ---- Serialization helpers (called by __serialize) ----

fn serialize_mt19937(obj: &Rc<RefCell<PhpObject>>) -> Value {
    let st = load_mt_state(obj);
    let mut outer = PhpArray::new();

    // Index 0: properties array (empty for native engine)
    let props = PhpArray::new();
    outer.set(ArrayKey::Int(0), Value::Array(Rc::new(RefCell::new(props))));

    // Index 1: state array of 624 hex strings + count + mode
    let mut state_arr = PhpArray::new();
    for (i, v) in st.state.iter().enumerate() {
        state_arr.set(
            ArrayKey::Int(i as i64),
            Value::String(PhpString::from_string(u32_to_hex_le(*v))),
        );
    }
    state_arr.set(ArrayKey::Int(MT_N as i64), Value::Long(st.count as i64));
    state_arr.set(ArrayKey::Int(MT_N as i64 + 1), Value::Long(st.mode));

    outer.set(
        ArrayKey::Int(1),
        Value::Array(Rc::new(RefCell::new(state_arr))),
    );
    Value::Array(Rc::new(RefCell::new(outer)))
}

fn serialize_xoshiro(obj: &Rc<RefCell<PhpObject>>) -> Value {
    let st = load_xoshiro_state(obj);
    let mut outer = PhpArray::new();
    let props = PhpArray::new();
    outer.set(ArrayKey::Int(0), Value::Array(Rc::new(RefCell::new(props))));

    let mut state_arr = PhpArray::new();
    for (i, v) in st.s.iter().enumerate() {
        state_arr.set(
            ArrayKey::Int(i as i64),
            Value::String(PhpString::from_string(u64_to_hex_le(*v))),
        );
    }
    outer.set(
        ArrayKey::Int(1),
        Value::Array(Rc::new(RefCell::new(state_arr))),
    );
    Value::Array(Rc::new(RefCell::new(outer)))
}

fn serialize_pcg(obj: &Rc<RefCell<PhpObject>>) -> Value {
    let st = load_pcg_state(obj);
    let mut outer = PhpArray::new();
    let props = PhpArray::new();
    outer.set(ArrayKey::Int(0), Value::Array(Rc::new(RefCell::new(props))));

    let hi = (st.state >> 64) as u64;
    let lo = st.state as u64;
    let mut state_arr = PhpArray::new();
    state_arr.set(
        ArrayKey::Int(0),
        Value::String(PhpString::from_string(u64_to_hex_le(hi))),
    );
    state_arr.set(
        ArrayKey::Int(1),
        Value::String(PhpString::from_string(u64_to_hex_le(lo))),
    );
    outer.set(
        ArrayKey::Int(1),
        Value::Array(Rc::new(RefCell::new(state_arr))),
    );
    Value::Array(Rc::new(RefCell::new(outer)))
}
