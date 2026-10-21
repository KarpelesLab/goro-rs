//! Stream filters: main/streams/filter.c (bucket brigades, filter chains),
//! ext/standard/user_filters.c (php_user_filter, StreamBucket,
//! stream_bucket_*), the built-in string.* / consumed filters of
//! ext/standard/filters.c, and the zlib.* / bzip2.* filters
//! (ext/zlib/zlib_filter.c, ext/bz2/bz2_filter.c).
//! convert.* and dechunk live in `filters_conv`.
//!
//! Every stream id has a read chain and a write chain. A filter attached
//! with STREAM_FILTER_ALL is two filter instances, one per chain (only the
//! last one created gets a resource, as in PHP). The stream layers
//! (misc.rs for descriptor/memory streams, userstream.rs for user-space
//! streams) drive the chains through `run_write_chain` / `run_read_chain`.

use goro_core::array::ArrayKey;
use goro_core::object::{PhpObject, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::filters_conv::{ConvertFilter, DechunkFilter};

pub const STREAM_FILTER_READ: i64 = 1;
pub const STREAM_FILTER_WRITE: i64 = 2;
pub const STREAM_FILTER_ALL: i64 = STREAM_FILTER_READ | STREAM_FILTER_WRITE;

pub const PSFS_ERR_FATAL: i32 = 0;
pub const PSFS_FEED_ME: i32 = 1;
pub const PSFS_PASS_ON: i32 = 2;

pub const PSFS_FLAG_NORMAL: i32 = 0;
pub const PSFS_FLAG_FLUSH_INC: i32 = 1;
pub const PSFS_FLAG_FLUSH_CLOSE: i32 = 2;

// ---------------------------------------------------------------------------
// Buckets and brigades
// ---------------------------------------------------------------------------

/// php_stream_bucket
pub struct Bucket {
    pub buf: Vec<u8>,
    /// Id of the brigade the bucket is linked into (0 = none).
    brigade: u64,
}

pub type BucketRef = Rc<RefCell<Bucket>>;

pub fn new_bucket(buf: Vec<u8>) -> BucketRef {
    Rc::new(RefCell::new(Bucket { buf, brigade: 0 }))
}

/// php_stream_bucket_brigade
pub struct Brigade {
    id: u64,
    list: VecDeque<BucketRef>,
}

pub type BrigadeRef = Rc<RefCell<Brigade>>;

thread_local! {
    static NEXT_BRIGADE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    /// Brigades currently exposed to user filters, by brigade id.
    static LIVE_BRIGADES: RefCell<HashMap<u64, BrigadeRef>> = RefCell::new(HashMap::new());
    /// "userfilter.bucket brigade" resources.
    static BRIGADE_RES: RefCell<HashMap<i64, BrigadeRef>> = RefCell::new(HashMap::new());
    /// "userfilter.bucket" resources.
    static BUCKET_RES: RefCell<HashMap<i64, BucketRef>> = RefCell::new(HashMap::new());
}

impl Default for Brigade {
    fn default() -> Self {
        Self::new()
    }
}

impl Brigade {
    pub fn new() -> Self {
        let id = NEXT_BRIGADE.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        Brigade {
            id,
            list: VecDeque::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn head(&self) -> Option<BucketRef> {
        self.list.front().cloned()
    }

    /// php_stream_bucket_append()
    pub fn append(&mut self, b: BucketRef) {
        // "TODO: this was added as a bad workaround for bug #35916"
        if let Some(t) = self.list.back() {
            if Rc::ptr_eq(t, &b) {
                return;
            }
        }
        b.borrow_mut().brigade = self.id;
        self.list.push_back(b);
    }

    /// php_stream_bucket_prepend()
    pub fn prepend(&mut self, b: BucketRef) {
        b.borrow_mut().brigade = self.id;
        self.list.push_front(b);
    }

    /// Unlink and return the head bucket (php_stream_bucket_make_writeable()
    /// of brigade->head).
    pub fn pop_front(&mut self) -> Option<BucketRef> {
        let b = self.list.pop_front()?;
        b.borrow_mut().brigade = 0;
        Some(b)
    }

    fn unlink(&mut self, b: &BucketRef) {
        if let Some(pos) = self.list.iter().position(|x| Rc::ptr_eq(x, b)) {
            self.list.remove(pos);
        }
        b.borrow_mut().brigade = 0;
    }

    pub fn total_len(&self) -> usize {
        self.list.iter().map(|b| b.borrow().buf.len()).sum()
    }

    /// Concatenated contents (the buckets are dropped).
    pub fn take_all(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(b) = self.pop_front() {
            out.extend_from_slice(&b.borrow().buf);
        }
        out
    }

    pub fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }

    fn bufs(&mut self) -> Vec<Vec<u8>> {
        let mut v = Vec::new();
        while let Some(b) = self.pop_front() {
            v.push(std::mem::take(&mut b.borrow_mut().buf));
        }
        v
    }
}

/// php_stream_bucket_unlink(): remove a bucket from whatever brigade holds it.
fn bucket_unlink(b: &BucketRef) {
    let bid = b.borrow().brigade;
    if bid == 0 {
        return;
    }
    let br = LIVE_BRIGADES.with(|m| m.borrow().get(&bid).cloned());
    match br {
        Some(br) => br.borrow_mut().unlink(b),
        None => b.borrow_mut().brigade = 0,
    }
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

/// The built-in filter implementations (php_stream_filter_ops).
pub enum Builtin {
    Rot13,
    ToUpper,
    ToLower,
    Consumed {
        offset: Option<i64>,
        consumed: usize,
    },
    Convert(Box<ConvertFilter>),
    Dechunk(Box<DechunkFilter>),
    ZlibInflate(Box<ZlibInflate>),
    ZlibDeflate(Box<ZlibDeflate>),
    BzDecompress(Box<BzDecompress>),
    BzCompress(Box<BzCompress>),
}

pub enum Ops {
    Builtin(Builtin),
    /// userfilter_ops: the filter object.
    User(Value),
}

/// php_stream_filter
pub struct Filter {
    /// fops->label for built-ins, the requested name for user filters.
    pub name: Vec<u8>,
    pub ops: Ops,
    /// Stream the filter is attached to (0 = detached).
    pub stream: i64,
    pub is_read: bool,
    /// "stream filter" resource id (0 = none).
    pub res: i64,
}

pub type FilterRef = Rc<RefCell<Filter>>;

#[derive(Default)]
struct Chains {
    read: Vec<FilterRef>,
    write: Vec<FilterRef>,
}

thread_local! {
    static CHAINS: RefCell<HashMap<i64, Chains>> = RefCell::new(HashMap::new());
    static FILTER_RES: RefCell<HashMap<i64, FilterRef>> = RefCell::new(HashMap::new());
    /// BG(user_filter_map): filter name -> class name, in registration order.
    static USER_FILTERS: RefCell<Vec<(Vec<u8>, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    /// Streams whose user filter callback is running (PHP_STREAM_FLAG_NO_FCLOSE).
    static IN_CALLBACK: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
}

pub fn in_filter_callback(stream: i64) -> bool {
    IN_CALLBACK.with(|v| v.borrow().contains(&stream))
}

/// Ids of streams that have filters attached.
pub fn streams_with_filters() -> Vec<i64> {
    CHAINS.with(|m| {
        m.borrow()
            .iter()
            .filter(|(_, c)| !c.read.is_empty() || !c.write.is_empty())
            .map(|(k, _)| *k)
            .collect()
    })
}

/// Built-in factories, in php-src MINIT registration order.
const BUILTIN_FACTORIES: &[&[u8]] = &[
    b"zlib.*",
    b"bzip2.*",
    b"convert.iconv.*",
    b"string.rot13",
    b"string.toupper",
    b"string.tolower",
    b"convert.*",
    b"consumed",
    b"dechunk",
];

/// stream_get_filters()
pub fn list_filter_names() -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = BUILTIN_FACTORIES.iter().map(|n| n.to_vec()).collect();
    USER_FILTERS.with(|m| {
        for (k, _) in m.borrow().iter() {
            names.push(k.clone());
        }
    });
    names
}

/// stream_filter_register(): false when the name is taken.
pub fn register_user_filter(name: &[u8], class: &[u8]) -> bool {
    if USER_FILTERS.with(|m| m.borrow().iter().any(|(n, _)| n == name)) {
        return false;
    }
    // php_stream_filter_register_factory_volatile() fails for names of
    // the built-in factories.
    if BUILTIN_FACTORIES.contains(&name) {
        return false;
    }
    USER_FILTERS.with(|m| m.borrow_mut().push((name.to_vec(), class.to_vec())));
    true
}

pub fn is_user_filter(name: &[u8]) -> bool {
    USER_FILTERS.with(|m| m.borrow().iter().any(|(n, _)| n == name))
}

/// Which factory serves `name` (exact name, then "prefix.*" wildcards
/// from the longest prefix down). Returns (factory key, user class).
fn find_factory(name: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let lookup = |key: &[u8]| -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        if let Some(c) = USER_FILTERS.with(|m| {
            m.borrow()
                .iter()
                .find(|(n, _)| n.as_slice() == key)
                .map(|(_, c)| c.clone())
        }) {
            return Some((key.to_vec(), Some(c)));
        }
        if BUILTIN_FACTORIES.contains(&key) {
            return Some((key.to_vec(), None));
        }
        None
    };
    if let Some(f) = lookup(name) {
        return Some(f);
    }
    let mut cut = name.len();
    while let Some(p) = name[..cut].iter().rposition(|&c| c == b'.') {
        let mut wild = name[..=p].to_vec();
        wild.push(b'*');
        if let Some(f) = lookup(&wild) {
            return Some(f);
        }
        cut = p;
    }
    None
}

/// Name of the running internal function, for php_error_docref().
fn cur_fn(vm: &Vm) -> String {
    vm.current_builtin_name().unwrap_or_default()
}

/// php_error_docref(NULL, E_WARNING, ...)
pub fn docref_warning(vm: &mut Vm, msg: &str) {
    let f = cur_fn(vm);
    if f.is_empty() {
        vm.emit_warning(msg);
    } else {
        vm.emit_warning(&format!("{}(): {}", f, msg));
    }
}

fn docref_notice(vm: &mut Vm, msg: &str) {
    let f = cur_fn(vm);
    let line = vm.current_line;
    if f.is_empty() {
        vm.emit_notice_at(msg, line);
    } else {
        vm.emit_notice_at(&format!("{}(): {}", f, msg), line);
    }
}

fn pending(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = if class == b"TypeError" {
        vm.throw_type_error(msg.to_string())
    } else {
        vm.create_exception(class, msg, vm.current_line)
    };
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// php_stream_filter_create(). Ok(None) when the filter could not be
/// created (the warnings were emitted); Err for exceptions.
pub fn create_filter(
    vm: &mut Vm,
    name: &[u8],
    params: Option<&Value>,
    persistent: bool,
) -> Result<Option<Filter>, VmError> {
    let (key, user_class) = match find_factory(name) {
        Some(f) => f,
        None => {
            docref_warning(vm, &format!("Unable to locate filter \"{}\"", lossy(name)));
            return Ok(None);
        }
    };
    let ops = match user_class {
        Some(class) => user_filter_create(vm, name, &class, params, persistent)?,
        None => builtin_create(vm, &key, name, params),
    };
    match ops {
        Some(ops) => Ok(Some(Filter {
            name: name.to_vec(),
            ops,
            stream: 0,
            is_read: false,
            res: 0,
        })),
        None => {
            if vm.current_exception.is_none() {
                docref_warning(
                    vm,
                    &format!("Unable to create or locate filter \"{}\"", lossy(name)),
                );
            }
            Ok(None)
        }
    }
}

fn builtin_create(vm: &mut Vm, key: &[u8], name: &[u8], params: Option<&Value>) -> Option<Ops> {
    let b = match key {
        b"string.rot13" => Builtin::Rot13,
        b"string.toupper" => Builtin::ToUpper,
        b"string.tolower" => Builtin::ToLower,
        b"consumed" => Builtin::Consumed {
            offset: None,
            consumed: 0,
        },
        b"convert.*" => match ConvertFilter::create(name, params) {
            Ok(f) => Builtin::Convert(Box::new(f)),
            Err(w) => {
                for m in w {
                    docref_warning(vm, &m);
                }
                return None;
            }
        },
        b"dechunk" => match DechunkFilter::create(name, params) {
            Ok(f) => Builtin::Dechunk(Box::new(f)),
            Err(w) => {
                for m in w {
                    docref_warning(vm, &m);
                }
                return None;
            }
        },
        b"zlib.*" => zlib_create(vm, name, params)?,
        b"bzip2.*" => bz2_create(vm, name, params)?,
        // convert.iconv.* needs the iconv extension.
        _ => return None,
    };
    Some(Ops::Builtin(b))
}

// ---------------------------------------------------------------------------
// Running filters
// ---------------------------------------------------------------------------

/// fops->filter()
fn call_filter(
    vm: &mut Vm,
    f: &FilterRef,
    inp: &BrigadeRef,
    out: &BrigadeRef,
    consumed: Option<&mut usize>,
    flags: i32,
) -> i32 {
    let is_user = matches!(f.borrow().ops, Ops::User(_));
    if is_user {
        return user_filter_filter(vm, f, inp, out, consumed, flags);
    }
    let stream = f.borrow().stream;
    let mut fb = f.borrow_mut();
    let Ops::Builtin(b) = &mut fb.ops else {
        return PSFS_ERR_FATAL;
    };
    let mut inp = inp.borrow_mut();
    let mut out = out.borrow_mut();
    builtin_filter(vm, b, stream, &mut inp, &mut out, consumed, flags)
}

fn builtin_filter(
    vm: &mut Vm,
    b: &mut Builtin,
    stream: i64,
    inp: &mut Brigade,
    out: &mut Brigade,
    consumed: Option<&mut usize>,
    flags: i32,
) -> i32 {
    let simple = |inp: &mut Brigade, out: &mut Brigade, map: fn(u8) -> u8| -> usize {
        let mut c = 0;
        while let Some(bk) = inp.pop_front() {
            {
                let mut bm = bk.borrow_mut();
                for x in bm.buf.iter_mut() {
                    *x = map(*x);
                }
                c += bm.buf.len();
            }
            out.append(bk);
        }
        c
    };
    match b {
        Builtin::Rot13 => {
            let c = simple(inp, out, |x| match x {
                b'a'..=b'm' | b'A'..=b'M' => x + 13,
                b'n'..=b'z' | b'N'..=b'Z' => x - 13,
                _ => x,
            });
            if let Some(cp) = consumed {
                *cp = c;
            }
            PSFS_PASS_ON
        }
        Builtin::ToUpper => {
            let c = simple(inp, out, |x| x.to_ascii_uppercase());
            if let Some(cp) = consumed {
                *cp = c;
            }
            PSFS_PASS_ON
        }
        Builtin::ToLower => {
            let c = simple(inp, out, |x| x.to_ascii_lowercase());
            if let Some(cp) = consumed {
                *cp = c;
            }
            PSFS_PASS_ON
        }
        Builtin::Consumed {
            offset,
            consumed: total,
        } => {
            // consumed_filter_filter()
            if offset.is_none() {
                *offset = Some(crate::misc::filter_stream_tell(stream).unwrap_or(-1));
            }
            let mut c = 0;
            while let Some(bk) = inp.pop_front() {
                c += bk.borrow().buf.len();
                out.append(bk);
            }
            if let Some(cp) = consumed {
                *cp = c;
            }
            if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
                let pos = offset.unwrap_or(0) + *total as i64;
                crate::misc::filter_stream_seek(stream, pos);
            }
            *total += c;
            PSFS_PASS_ON
        }
        Builtin::Convert(cf) => {
            let r = cf.filter(&inp.bufs(), flags);
            finish_conv(vm, r, out, consumed)
        }
        Builtin::Dechunk(df) => {
            let r = df.filter(&inp.bufs(), flags);
            finish_conv(vm, r, out, consumed)
        }
        Builtin::ZlibInflate(z) => z.filter(vm, inp, out, consumed, flags),
        Builtin::ZlibDeflate(z) => z.filter(inp, out, consumed, flags),
        Builtin::BzDecompress(z) => z.filter(vm, inp, out, consumed, flags),
        Builtin::BzCompress(z) => z.filter(inp, out, consumed, flags),
    }
}

fn finish_conv(
    vm: &mut Vm,
    r: crate::filters_conv::FilterOutput,
    out: &mut Brigade,
    consumed: Option<&mut usize>,
) -> i32 {
    for w in &r.warnings {
        docref_warning(vm, w);
    }
    for o in r.out {
        out.append(new_bucket(o));
    }
    if let Some(cp) = consumed {
        *cp = r.consumed;
    }
    r.status
}

/// Result of running a chain.
pub struct ChainResult {
    pub status: i32,
    /// PSFS_PASS_ON output of the last filter.
    pub out: Vec<u8>,
}

fn chain_of(stream: i64, is_read: bool) -> Vec<FilterRef> {
    CHAINS.with(|m| {
        m.borrow()
            .get(&stream)
            .map(|c| {
                if is_read {
                    c.read.clone()
                } else {
                    c.write.clone()
                }
            })
            .unwrap_or_default()
    })
}

fn run_chain(
    vm: &mut Vm,
    chain: &[FilterRef],
    data: Option<&[u8]>,
    mut consumed: Option<&mut usize>,
    flags: i32,
) -> ChainResult {
    let mut inp: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
    let mut outp: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
    if let Some(d) = data {
        inp.borrow_mut().append(new_bucket(d.to_vec()));
    }
    let mut status = PSFS_ERR_FATAL;
    for (i, f) in chain.iter().enumerate() {
        let c = if i == 0 {
            consumed.as_deref_mut()
        } else {
            None
        };
        status = call_filter(vm, f, &inp, &outp, c, flags);
        if status != PSFS_PASS_ON {
            break;
        }
        std::mem::swap(&mut inp, &mut outp);
        outp.borrow_mut().clear();
    }
    let out = if status == PSFS_PASS_ON {
        inp.borrow_mut().take_all()
    } else {
        Vec::new()
    };
    ChainResult { status, out }
}

/// _php_stream_write_filtered(): returns the bytes consumed by the first
/// filter (-1 for PSFS_ERR_FATAL) and the data to write to the stream.
pub fn run_write_chain(
    vm: &mut Vm,
    stream: i64,
    data: Option<&[u8]>,
    flags: i32,
) -> (isize, Vec<u8>) {
    let chain = chain_of(stream, false);
    if chain.is_empty() {
        return (
            data.map(|d| d.len()).unwrap_or(0) as isize,
            data.unwrap_or_default().to_vec(),
        );
    }
    let mut consumed = 0usize;
    let r = run_chain(vm, &chain, data, Some(&mut consumed), flags);
    match r.status {
        PSFS_PASS_ON => (consumed as isize, r.out),
        PSFS_ERR_FATAL => (-1, Vec::new()),
        _ => (consumed as isize, Vec::new()),
    }
}

/// One iteration of php_stream_fill_read_buffer()'s filtered loop: feed
/// `data` (a chunk just read, None when nothing was read) through the read
/// chain.
pub fn run_read_chain(vm: &mut Vm, stream: i64, data: Option<&[u8]>, flags: i32) -> ChainResult {
    let chain = chain_of(stream, true);
    run_chain(vm, &chain, data.filter(|d| !d.is_empty()), None, flags)
}

pub fn chain_has_direction(stream_id: i64, direction: i64) -> bool {
    CHAINS.with(|m| {
        m.borrow().get(&stream_id).is_some_and(|c| {
            (direction & STREAM_FILTER_READ != 0 && !c.read.is_empty())
                || (direction & STREAM_FILTER_WRITE != 0 && !c.write.is_empty())
        })
    })
}

pub fn chain_has_any(stream_id: i64) -> bool {
    chain_has_direction(stream_id, STREAM_FILTER_ALL)
}

/// php_stream_filter_append_ex() / prepend_ex(): link `f` into a chain.
/// For an append to the read chain, pre-buffered data is run through the
/// new filter: `buffered` holds the stream's unread data and is replaced
/// by the filter's output. Err(()) when the filter failed on it (the
/// filter was unlinked and freed).
pub fn attach(
    vm: &mut Vm,
    stream: i64,
    f: Filter,
    is_read: bool,
    append: bool,
    buffered: Option<&mut Vec<u8>>,
) -> Result<FilterRef, FilterFailed> {
    let mut f = f;
    f.stream = stream;
    f.is_read = is_read;
    let fr = Rc::new(RefCell::new(f));
    CHAINS.with(|m| {
        let mut m = m.borrow_mut();
        let c = m.entry(stream).or_default();
        let chain = if is_read { &mut c.read } else { &mut c.write };
        if append {
            chain.push(fr.clone());
        } else {
            chain.insert(0, fr.clone());
        }
    });
    if append && is_read {
        if let Some(buf) = buffered {
            if !buf.is_empty() {
                let inp: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
                let out: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
                inp.borrow_mut().append(new_bucket(buf.clone()));
                let mut consumed = 0usize;
                let mut status =
                    call_filter(vm, &fr, &inp, &out, Some(&mut consumed), PSFS_FLAG_NORMAL);
                if consumed > buf.len() {
                    status = PSFS_ERR_FATAL;
                }
                match status {
                    PSFS_ERR_FATAL => {
                        docref_warning(vm, "Filter failed to process pre-buffered data");
                        detach(vm, &fr, true);
                        return Err(FilterFailed);
                    }
                    PSFS_FEED_ME => {
                        buf.clear();
                    }
                    _ => {
                        *buf = out.borrow_mut().take_all();
                    }
                }
            }
        }
    }
    Ok(fr)
}

/// A filter failed (PSFS_ERR_FATAL / FAILURE).
#[derive(Debug)]
pub struct FilterFailed;

/// Give `f` a "stream filter" resource.
pub fn register_resource(f: &FilterRef) -> i64 {
    let id = crate::misc::alloc_resource_id();
    f.borrow_mut().res = id;
    FILTER_RES.with(|m| m.borrow_mut().insert(id, f.clone()));
    goro_core::value::register_resource(id, b"stream filter");
    id
}

pub fn filter_by_resource(res: i64) -> Option<FilterRef> {
    if !goro_core::value::is_open_resource(res) {
        return None;
    }
    FILTER_RES.with(|m| m.borrow().get(&res).cloned())
}

pub fn is_filter_resource(res: i64) -> bool {
    filter_by_resource(res).is_some()
}

/// php_stream_filter_remove(filter, call_dtor): unlink from the chain,
/// close the resource and (optionally) run the destructor.
pub fn detach(vm: &mut Vm, f: &FilterRef, call_dtor: bool) {
    let (stream, is_read, res) = {
        let b = f.borrow();
        (b.stream, b.is_read, b.res)
    };
    CHAINS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&stream) {
            let chain = if is_read { &mut c.read } else { &mut c.write };
            chain.retain(|x| !Rc::ptr_eq(x, f));
        }
    });
    if res != 0 {
        FILTER_RES.with(|m| m.borrow_mut().remove(&res));
        goro_core::value::close_resource(res);
        f.borrow_mut().res = 0;
    }
    f.borrow_mut().stream = 0;
    if call_dtor {
        filter_dtor(vm, f);
    }
}

/// fops->dtor(): user filters get onClose().
fn filter_dtor(vm: &mut Vm, f: &FilterRef) {
    let obj = match &f.borrow().ops {
        Ops::User(o) => o.clone(),
        _ => return,
    };
    let saved = vm.current_exception.take();
    let _ = crate::misc::userstream::call_method_pub(vm, &obj, b"onclose", vec![]);
    if vm.current_exception.is_none() {
        vm.current_exception = saved;
    }
}

/// _php_stream_free(): remove every filter of a stream (read chain first).
pub fn remove_stream_filters(vm: &mut Vm, stream_id: i64) {
    let chains = CHAINS.with(|m| m.borrow_mut().remove(&stream_id));
    if let Some(c) = chains {
        for f in c.read.iter().chain(c.write.iter()) {
            let res = f.borrow().res;
            if res != 0 {
                FILTER_RES.with(|m| m.borrow_mut().remove(&res));
                goro_core::value::close_resource(res);
                f.borrow_mut().res = 0;
            }
            f.borrow_mut().stream = 0;
            filter_dtor(vm, f);
        }
    }
}

/// _php_stream_filter_flush(filter, finish): run the chain from `f` on.
/// Ok(Some((is_read, data))) gives data to put in the read buffer or write
/// to the stream; Err(()) on PSFS_ERR_FATAL.
pub fn flush_filter(
    vm: &mut Vm,
    f: &FilterRef,
    finish: bool,
) -> Result<Option<(bool, Vec<u8>)>, FilterFailed> {
    let (stream, is_read) = {
        let b = f.borrow();
        (b.stream, b.is_read)
    };
    if stream == 0 {
        return Err(FilterFailed);
    }
    let chain = chain_of(stream, is_read);
    let start = match chain.iter().position(|x| Rc::ptr_eq(x, f)) {
        Some(p) => p,
        None => return Err(FilterFailed),
    };
    let mut inp: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
    let mut outp: BrigadeRef = Rc::new(RefCell::new(Brigade::new()));
    let mut flags = if finish {
        PSFS_FLAG_FLUSH_CLOSE
    } else {
        PSFS_FLAG_FLUSH_INC
    };
    for cur in &chain[start..] {
        let status = call_filter(vm, cur, &inp, &outp, None, flags);
        if status == PSFS_FEED_ME {
            return Ok(None);
        }
        if status == PSFS_ERR_FATAL {
            return Err(FilterFailed);
        }
        std::mem::swap(&mut inp, &mut outp);
        outp.borrow_mut().clear();
        flags = PSFS_FLAG_NORMAL;
    }
    let data = inp.borrow_mut().take_all();
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some((is_read, data)))
}

// ---------------------------------------------------------------------------
// User filters (user_filters.c)
// ---------------------------------------------------------------------------

/// user_filter_factory_create()
fn user_filter_create(
    vm: &mut Vm,
    name: &[u8],
    class: &[u8],
    params: Option<&Value>,
    persistent: bool,
) -> Result<Option<Ops>, VmError> {
    if persistent {
        docref_warning(
            vm,
            "Cannot use a user-space filter with a persistent stream",
        );
        return Ok(None);
    }
    let cl = class.to_ascii_lowercase();
    if !vm.classes.contains_key(&cl) {
        vm.try_autoload_class(class);
        if vm.current_exception.is_some() {
            return Err(pending(vm));
        }
    }
    let (canonical, flags) = match vm.classes.get(&cl) {
        Some(c) => (
            c.name.clone(),
            (c.is_abstract, c.is_interface, c.is_trait, c.is_enum),
        ),
        None => {
            docref_warning(
                vm,
                &format!(
                    "User-filter \"{}\" requires class \"{}\", but that class is not defined",
                    lossy(name),
                    lossy(class)
                ),
            );
            return Ok(None);
        }
    };
    // object_init_ex()
    let disp = goro_core::value::display_class_name(&canonical).to_string();
    let kind = if flags.1 {
        Some("interface")
    } else if flags.2 {
        Some("trait")
    } else if flags.3 {
        Some("enum")
    } else if flags.0 {
        Some("abstract class")
    } else {
        None
    };
    if let Some(k) = kind {
        return Err(throw(
            vm,
            b"Error",
            &format!("Cannot instantiate {} {}", k, disp),
        ));
    }
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(canonical.clone(), id);
    vm.apply_class_property_defaults(&mut obj, &cl);
    let objv = Value::Object(Rc::new(RefCell::new(obj)));
    // add_property_string(&obj, "filtername", ...) and params.
    write_prop(
        vm,
        &objv,
        b"filtername",
        Value::String(PhpString::from_bytes(name)),
        None,
    )?;
    write_prop(
        vm,
        &objv,
        b"params",
        params.cloned().unwrap_or(Value::Null),
        None,
    )?;
    if vm
        .classes
        .get(&cl)
        .map(|c| c.get_method(b"__destruct").is_some())
        .unwrap_or(false)
    {
        vm.track_destructible(objv.clone());
    }
    // onCreate(): returning false fails the creation.
    let r = crate::misc::userstream::call_method_pub(vm, &objv, b"oncreate", vec![])?;
    if vm.current_exception.is_some() {
        return Err(pending(vm));
    }
    if matches!(r, Some(Value::False)) {
        return Ok(None);
    }
    Ok(Some(Ops::User(objv)))
}

/// Property write through the standard handler (visibility from `scope`,
/// readonly, typed properties), as zend_update_property()/add_property_*.
fn write_prop(
    vm: &mut Vm,
    obj: &Value,
    name: &[u8],
    val: Value,
    scope: Option<&[u8]>,
) -> Result<(), VmError> {
    let Value::Object(o) = obj else {
        return Ok(());
    };
    let cl = o.borrow().class_name.to_ascii_lowercase();
    let canonical = o.borrow().class_name.clone();
    let pdef = vm.classes.get(&cl).and_then(|c| {
        c.properties
            .iter()
            .find(|p| p.name == name && !p.is_static)
            .cloned()
    });
    if let Some(pd) = &pdef {
        let decl = pd.declaring_class.to_ascii_lowercase();
        let scope_l = scope.map(|s| s.to_ascii_lowercase());
        let accessible = match pd.visibility {
            Visibility::Public => true,
            Visibility::Private => scope_l.as_deref() == Some(decl.as_slice()),
            Visibility::Protected => match &scope_l {
                Some(sc) => {
                    *sc == decl || vm.class_extends(sc, &decl) || vm.class_extends(&decl, sc)
                }
                None => false,
            },
        };
        if !accessible {
            let decl_name = vm
                .classes
                .get(&decl)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| canonical.clone());
            let vis = if matches!(pd.visibility, Visibility::Private) {
                "private"
            } else {
                "protected"
            };
            return Err(throw(
                vm,
                b"Error",
                &format!(
                    "Cannot access {} property {}::${}",
                    vis,
                    goro_core::value::display_class_name(&decl_name),
                    lossy(name)
                ),
            ));
        }
        if let Some(pt) = &pd.property_type {
            if !vm.value_matches_type(&val, pt) {
                let msg = format!(
                    "Cannot assign {} to property {}::${} of type {}",
                    Vm::value_type_name(&val),
                    goro_core::value::display_class_name(&canonical),
                    lossy(name),
                    vm.param_type_name(pt)
                );
                return Err(throw(vm, b"TypeError", &msg));
            }
        }
    }
    o.borrow_mut().set_property(name.to_vec(), val);
    Ok(())
}

/// has_property(ZEND_PROPERTY_EXISTS): declared (not unset) or dynamic.
fn prop_exists(obj: &Value, name: &[u8]) -> bool {
    match obj {
        Value::Object(o) => o.borrow().has_property(name),
        _ => false,
    }
}

/// Register a brigade as a "userfilter.bucket brigade" resource.
fn brigade_resource(br: &BrigadeRef) -> i64 {
    let id = crate::misc::alloc_resource_id();
    let bid = br.borrow().id;
    LIVE_BRIGADES.with(|m| m.borrow_mut().insert(bid, br.clone()));
    BRIGADE_RES.with(|m| m.borrow_mut().insert(id, br.clone()));
    goro_core::value::register_resource(id, b"userfilter.bucket brigade");
    id
}

fn release_brigade_resource(id: i64) {
    if let Some(br) = BRIGADE_RES.with(|m| m.borrow_mut().remove(&id)) {
        let bid = br.borrow().id;
        LIVE_BRIGADES.with(|m| m.borrow_mut().remove(&bid));
    }
    goro_core::value::close_resource(id);
}

/// userfilter_filter()
fn user_filter_filter(
    vm: &mut Vm,
    f: &FilterRef,
    inp: &BrigadeRef,
    out: &BrigadeRef,
    consumed: Option<&mut usize>,
    flags: i32,
) -> i32 {
    let (obj, stream) = {
        let b = f.borrow();
        match &b.ops {
            Ops::User(o) => (o.clone(), b.stream),
            _ => return PSFS_ERR_FATAL,
        }
    };
    let class = match &obj {
        Value::Object(o) => o.borrow().class_name.clone(),
        _ => return PSFS_ERR_FATAL,
    };
    // Give the userfilter class a hook back to the stream.
    let has_stream_prop = prop_exists(&obj, b"stream");
    if has_stream_prop {
        let sv = if stream != 0 {
            Value::Resource(stream)
        } else {
            Value::Null
        };
        if write_prop(vm, &obj, b"stream", sv, Some(&class)).is_err()
            || vm.current_exception.is_some()
        {
            if !inp.borrow().is_empty() {
                docref_warning(vm, "Unprocessed filter buckets remaining on input brigade");
            }
            return PSFS_ERR_FATAL;
        }
    }
    let in_res = brigade_resource(inp);
    let out_res = brigade_resource(out);
    let has_consumed = consumed.is_some();
    let consumed_rc = Rc::new(RefCell::new(match &consumed {
        Some(c) => Value::Long(**c as i64),
        None => Value::Null,
    }));
    let args = vec![
        Value::Resource(in_res),
        Value::Resource(out_res),
        Value::Reference(consumed_rc.clone()),
        if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
            Value::True
        } else {
            Value::False
        },
    ];
    let mut ret = PSFS_ERR_FATAL;
    IN_CALLBACK.with(|v| v.borrow_mut().push(stream));
    let call = crate::misc::userstream::call_method_pub(vm, &obj, b"filter", args);
    IN_CALLBACK.with(|v| {
        let mut v = v.borrow_mut();
        if let Some(p) = v.iter().rposition(|&x| x == stream) {
            v.remove(p);
        }
    });
    match call {
        Ok(Some(v)) if vm.current_exception.is_none() => {
            ret = v.to_long() as i32;
        }
        Ok(Some(_)) => {}
        Ok(None) => {
            if vm.current_exception.is_none() {
                docref_warning(vm, "Failed to call filter function");
            }
        }
        Err(_) => {}
    }
    if has_consumed {
        if let Some(c) = consumed {
            *c = consumed_rc.borrow().to_long().max(0) as usize;
        }
    }
    if !inp.borrow().is_empty() {
        docref_warning(vm, "Unprocessed filter buckets remaining on input brigade");
    }
    if has_stream_prop {
        let saved = vm.current_exception.take();
        let _ = write_prop(vm, &obj, b"stream", Value::Null, Some(&class));
        if saved.is_some() {
            vm.current_exception = saved;
        }
    }
    release_brigade_resource(in_res);
    release_brigade_resource(out_res);
    ret
}

// ---------------------------------------------------------------------------
// stream_bucket_* (user_filters.c)
// ---------------------------------------------------------------------------

fn arg_resource(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    n: usize,
    name: &str,
) -> Result<i64, VmError> {
    match args.get(n - 1).map(|v| v.deref()) {
        Some(Value::Resource(id)) => Ok(id),
        Some(v) => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #{} (${}) must be of type resource, {} given",
                func,
                n,
                name,
                Vm::value_type_name(&v)
            ),
        )),
        None => Err(throw(
            vm,
            b"ArgumentCountError",
            &format!(
                "{}() expects exactly 2 arguments, {} given",
                func,
                args.len()
            ),
        )),
    }
}

fn fetch_brigade(vm: &mut Vm, func: &str, id: i64) -> Result<BrigadeRef, VmError> {
    match BRIGADE_RES.with(|m| m.borrow().get(&id).cloned()) {
        Some(b) if goro_core::value::is_open_resource(id) => Ok(b),
        _ => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): supplied resource is not a valid userfilter.bucket brigade resource",
                func
            ),
        )),
    }
}

/// object_init_ex(StreamBucket) with bucket/data/datalen/dataLength.
fn bucket_object(vm: &mut Vm, b: &BucketRef) -> Value {
    let rid = crate::misc::alloc_resource_id();
    BUCKET_RES.with(|m| m.borrow_mut().insert(rid, b.clone()));
    goro_core::value::register_resource(rid, b"userfilter.bucket");
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"StreamBucket".to_vec(), id);
    vm.apply_class_property_defaults(&mut obj, b"streambucket");
    let data = b.borrow().buf.clone();
    let len = data.len() as i64;
    obj.set_property(b"bucket".to_vec(), Value::Resource(rid));
    obj.set_property(b"data".to_vec(), Value::String(PhpString::from_vec(data)));
    obj.set_property(b"datalen".to_vec(), Value::Long(len));
    obj.set_property(b"dataLength".to_vec(), Value::Long(len));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// PHP_FUNCTION(stream_bucket_make_writeable)
pub fn stream_bucket_make_writeable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_bucket_make_writeable";
    if args.len() != 1 {
        return Err(throw(
            vm,
            b"ArgumentCountError",
            &format!("{}() expects exactly 1 argument, {} given", F, args.len()),
        ));
    }
    let id = arg_resource(vm, F, args, 1, "brigade")?;
    let br = fetch_brigade(vm, F, id)?;
    let head = br.borrow_mut().pop_front();
    match head {
        Some(b) => Ok(bucket_object(vm, &b)),
        None => Ok(Value::Null),
    }
}

/// php_stream_bucket_attach()
fn bucket_attach(vm: &mut Vm, args: &[Value], append: bool) -> Result<Value, VmError> {
    let f = if append {
        "stream_bucket_append"
    } else {
        "stream_bucket_prepend"
    };
    if args.len() != 2 {
        return Err(throw(
            vm,
            b"ArgumentCountError",
            &format!("{}() expects exactly 2 arguments, {} given", f, args.len()),
        ));
    }
    let id = arg_resource(vm, f, args, 1, "brigade")?;
    let obj = match args[1].deref() {
        Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(b"StreamBucket") => o,
        v => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #2 ($bucket) must be of type StreamBucket, {} given",
                    f,
                    Vm::value_type_name(&v)
                ),
            ));
        }
    };
    let br = fetch_brigade(vm, f, id)?;
    let bucket_v = obj.borrow().get_property(b"bucket");
    let bucket = match bucket_v.deref() {
        Value::Resource(rid) if goro_core::value::is_open_resource(rid) => {
            BUCKET_RES.with(|m| m.borrow().get(&rid).cloned())
        }
        _ => None,
    };
    let Some(bucket) = bucket else {
        return Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): supplied resource is not a valid userfilter.bucket resource",
                f
            ),
        ));
    };
    let data = obj.borrow().get_property(b"data");
    if !matches!(data, Value::Undef) {
        let d = data.deref().to_php_string().as_bytes().to_vec();
        bucket.borrow_mut().buf = d;
    }
    bucket_unlink(&bucket);
    if append {
        br.borrow_mut().append(bucket);
    } else {
        br.borrow_mut().prepend(bucket);
    }
    Ok(Value::Null)
}

pub fn stream_bucket_append(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    bucket_attach(vm, args, true)
}

pub fn stream_bucket_prepend(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    bucket_attach(vm, args, false)
}

/// PHP_FUNCTION(stream_bucket_new)
pub fn stream_bucket_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "stream_bucket_new";
    if args.len() != 2 {
        return Err(throw(
            vm,
            b"ArgumentCountError",
            &format!("{}() expects exactly 2 arguments, {} given", F, args.len()),
        ));
    }
    let id = arg_resource(vm, F, args, 1, "stream")?;
    if !crate::misc::has_file_handle(id) {
        return Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($stream) must be an open stream resource",
                F
            ),
        ));
    }
    let data = match args[1].deref() {
        Value::Array(_) => {
            return Err(throw(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #2 ($buffer) must be of type string, array given",
                    F
                ),
            ));
        }
        Value::Object(_) => {
            let v = args[1].deref();
            vm.value_to_string_checked(&v)?.as_bytes().to_vec()
        }
        v => v.to_php_string().as_bytes().to_vec(),
    };
    let b = new_bucket(data);
    Ok(bucket_object(vm, &b))
}

/// Reset all filter state (between independent tests).
pub fn reset_all() {
    CHAINS.with(|m| m.borrow_mut().clear());
    FILTER_RES.with(|m| m.borrow_mut().clear());
    USER_FILTERS.with(|m| m.borrow_mut().clear());
    BRIGADE_RES.with(|m| m.borrow_mut().clear());
    BUCKET_RES.with(|m| m.borrow_mut().clear());
    LIVE_BRIGADES.with(|m| m.borrow_mut().clear());
}

// ---------------------------------------------------------------------------
// zlib.* (ext/zlib/zlib_filter.c)
// ---------------------------------------------------------------------------

const ZLIB_BUF: usize = 0x8000;

/// Find a key of an array or object parameter (zend_hash_str_find_ind on
/// HASH_OF()).
fn param_find(params: &Value, key: &[u8]) -> Option<Value> {
    match params.deref() {
        Value::Array(a) => a
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(key)))
            .map(|v| v.deref()),
        Value::Object(o) => {
            let o = o.borrow();
            if o.has_property(key) {
                Some(o.get_property(key).deref())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// php_zlib_filter_data for zlib.inflate
pub struct ZlibInflate {
    strm: Box<goro_zlib::Inflate>,
    /// Pending output (data->outbuf / strm.avail_out).
    outbuf: Vec<u8>,
    finished: bool,
}

/// php_zlib_filter_data for zlib.deflate
pub struct ZlibDeflate {
    strm: Box<goro_zlib::Deflate>,
    outbuf: Vec<u8>,
    /// signals that no flush is pending
    finished: bool,
}

/// php_zlib_filter_create()
fn zlib_create(vm: &mut Vm, name: &[u8], params: Option<&Value>) -> Option<Builtin> {
    use goro_zlib::*;
    if name.eq_ignore_ascii_case(b"zlib.inflate") {
        let mut window_bits: i64 = -(MAX_WBITS as i64);
        if let Some(p) = params {
            if matches!(p.deref(), Value::Array(_) | Value::Object(_)) {
                if let Some(t) = param_find(p, b"window") {
                    let tmp = t.to_long();
                    if tmp < -(MAX_WBITS as i64) || tmp > MAX_WBITS as i64 + 32 {
                        docref_warning(
                            vm,
                            &format!("Invalid parameter given for window size ({})", tmp),
                        );
                    } else {
                        window_bits = tmp;
                    }
                }
            }
        }
        let strm = Inflate::new(window_bits as i32).ok()?;
        Some(Builtin::ZlibInflate(Box::new(ZlibInflate {
            strm,
            outbuf: Vec::new(),
            finished: false,
        })))
    } else if name.eq_ignore_ascii_case(b"zlib.deflate") {
        let mut level: i64 = Z_DEFAULT_COMPRESSION as i64;
        let mut window_bits: i64 = -(MAX_WBITS as i64);
        let mut mem_level: i64 = MAX_MEM_LEVEL as i64;
        let set_level = |vm: &mut Vm, tmp: i64, level: &mut i64| {
            if !(-1..=9).contains(&tmp) {
                docref_warning(
                    vm,
                    &format!("Invalid compression level specified. ({})", tmp),
                );
            } else {
                *level = tmp;
            }
        };
        if let Some(p) = params {
            match p.deref() {
                Value::Array(_) | Value::Object(_) => {
                    if let Some(t) = param_find(p, b"memory") {
                        let tmp = t.to_long();
                        if !(1..=MAX_MEM_LEVEL as i64).contains(&tmp) {
                            docref_warning(
                                vm,
                                &format!("Invalid parameter given for memory level ({})", tmp),
                            );
                        } else {
                            mem_level = tmp;
                        }
                    }
                    if let Some(t) = param_find(p, b"window") {
                        let tmp = t.to_long();
                        if tmp < -(MAX_WBITS as i64) || tmp > MAX_WBITS as i64 + 16 {
                            docref_warning(
                                vm,
                                &format!("Invalid parameter given for window size ({})", tmp),
                            );
                        } else {
                            window_bits = tmp;
                        }
                    }
                    if let Some(t) = param_find(p, b"level") {
                        set_level(vm, t.to_long(), &mut level);
                    }
                }
                Value::String(_) | Value::Double(_) | Value::Long(_) => {
                    set_level(vm, p.deref().to_long(), &mut level);
                }
                _ => docref_warning(vm, "Invalid filter parameter, ignored"),
            }
        }
        let strm = Deflate::new(
            level as i32,
            Z_DEFLATED,
            window_bits as i32,
            mem_level as i32,
            0,
        )
        .ok()?;
        Some(Builtin::ZlibDeflate(Box::new(ZlibDeflate {
            strm,
            outbuf: Vec::new(),
            finished: true,
        })))
    } else {
        None
    }
}

/// One inflate()/deflate() round on `input` with a ZLIB_BUF output
/// window: returns (status, bytes consumed); new output is appended to
/// `outbuf`.
fn zlib_round(
    outbuf: &mut Vec<u8>,
    input: &[u8],
    f: impl FnOnce(&mut goro_zlib::Io) -> i32,
) -> (i32, usize) {
    let used = outbuf.len();
    outbuf.resize(ZLIB_BUF, 0);
    let mut io = goro_zlib::Io::new(input, outbuf);
    io.out_pos = used;
    let st = f(&mut io);
    let (consumed, produced) = (io.in_pos, io.out_pos);
    outbuf.truncate(produced);
    (st, consumed)
}

/// Pass the filled part of the output buffer on as a bucket.
fn zlib_emit(outbuf: &mut Vec<u8>, out: &mut Brigade) -> bool {
    if outbuf.is_empty() {
        return false;
    }
    out.append(new_bucket(std::mem::take(outbuf)));
    true
}

impl ZlibInflate {
    /// php_zlib_inflate_filter()
    fn filter(
        &mut self,
        vm: &mut Vm,
        inp: &mut Brigade,
        out: &mut Brigade,
        consumed: Option<&mut usize>,
        flags: i32,
    ) -> i32 {
        use goro_zlib::*;
        let mut exit = PSFS_FEED_ME;
        let mut total = 0usize;
        let flush = if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
            Z_FINISH
        } else {
            Z_SYNC_FLUSH
        };
        while let Some(b) = inp.pop_front() {
            let buf = std::mem::take(&mut b.borrow_mut().buf);
            let mut bin = 0usize;
            while bin < buf.len() && !self.finished {
                let desired = (buf.len() - bin).min(ZLIB_BUF);
                let chunk = &buf[bin..bin + desired];
                let strm = &mut self.strm;
                let (status, used) =
                    zlib_round(&mut self.outbuf, chunk, |io| strm.inflate(io, flush));
                if status == Z_STREAM_END {
                    self.finished = true;
                    exit = PSFS_PASS_ON;
                } else if status != Z_OK && status != Z_BUF_ERROR {
                    docref_notice(vm, &format!("zlib: {}", z_error(status)));
                    return PSFS_ERR_FATAL;
                }
                bin += used;
                if zlib_emit(&mut self.outbuf, out) {
                    exit = PSFS_PASS_ON;
                }
            }
            total += buf.len();
        }
        if !self.finished && flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
            let mut status = Z_OK;
            while status == Z_OK {
                let strm = &mut self.strm;
                status = zlib_round(&mut self.outbuf, &[], |io| strm.inflate(io, Z_FINISH)).0;
                if zlib_emit(&mut self.outbuf, out) {
                    exit = PSFS_PASS_ON;
                }
            }
        }
        if let Some(c) = consumed {
            *c = total;
        }
        exit
    }
}

impl ZlibDeflate {
    /// php_zlib_deflate_filter()
    fn filter(
        &mut self,
        inp: &mut Brigade,
        out: &mut Brigade,
        consumed: Option<&mut usize>,
        flags: i32,
    ) -> i32 {
        use goro_zlib::*;
        let mut exit = PSFS_FEED_ME;
        let mut total = 0usize;
        while let Some(b) = inp.pop_front() {
            let buf = std::mem::take(&mut b.borrow_mut().buf);
            let mut bin = 0usize;
            while bin < buf.len() {
                let desired = (buf.len() - bin).min(ZLIB_BUF);
                let chunk = &buf[bin..bin + desired];
                let flush_mode = if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
                    Z_FULL_FLUSH
                } else if flags & PSFS_FLAG_FLUSH_INC != 0 {
                    Z_SYNC_FLUSH
                } else {
                    Z_NO_FLUSH
                };
                self.finished = flush_mode != Z_NO_FLUSH;
                let strm = &mut self.strm;
                let (status, used) =
                    zlib_round(&mut self.outbuf, chunk, |io| strm.deflate(io, flush_mode));
                if status != Z_OK {
                    return PSFS_ERR_FATAL;
                }
                bin += used;
                if zlib_emit(&mut self.outbuf, out) {
                    exit = PSFS_PASS_ON;
                }
            }
            total += buf.len();
        }
        if flags & PSFS_FLAG_FLUSH_CLOSE != 0
            || (flags & PSFS_FLAG_FLUSH_INC != 0 && !self.finished)
        {
            let mode = if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
                Z_FINISH
            } else {
                Z_SYNC_FLUSH
            };
            let mut status = Z_OK;
            while status == Z_OK {
                let strm = &mut self.strm;
                status = zlib_round(&mut self.outbuf, &[], |io| strm.deflate(io, mode)).0;
                self.finished = true;
                if zlib_emit(&mut self.outbuf, out) {
                    exit = PSFS_PASS_ON;
                }
            }
        }
        if let Some(c) = consumed {
            *c = total;
        }
        exit
    }
}

// ---------------------------------------------------------------------------
// bzip2.* (ext/bz2/bz2_filter.c)
// ---------------------------------------------------------------------------

const BZ_BUF: usize = 2048;

#[derive(PartialEq, Clone, Copy)]
enum BzStatus {
    Uninitialized,
    Running,
    Finished,
}

pub struct BzDecompress {
    dec: Option<bzip2::Decompress>,
    status: BzStatus,
    small: bool,
    concatenated: bool,
}

pub struct BzCompress {
    enc: bzip2::Compress,
    is_flushed: bool,
}

fn bz2_create(vm: &mut Vm, name: &[u8], params: Option<&Value>) -> Option<Builtin> {
    if name.eq_ignore_ascii_case(b"bzip2.decompress") {
        let mut small = false;
        let mut concatenated = false;
        if let Some(p) = params {
            let tmp = if matches!(p.deref(), Value::Array(_) | Value::Object(_)) {
                if let Some(c) = param_find(p, b"concatenated") {
                    concatenated = c.is_truthy();
                }
                param_find(p, b"small")
            } else {
                Some(p.deref())
            };
            if let Some(t) = tmp {
                small = t.is_truthy();
            }
        }
        Some(Builtin::BzDecompress(Box::new(BzDecompress {
            dec: None,
            status: BzStatus::Uninitialized,
            small,
            concatenated,
        })))
    } else if name.eq_ignore_ascii_case(b"bzip2.compress") {
        let mut blocks = 4u32;
        let mut work = 0u32;
        if let Some(p) = params {
            if matches!(p.deref(), Value::Array(_) | Value::Object(_)) {
                if let Some(t) = param_find(p, b"blocks") {
                    let b = t.to_long();
                    if !(1..=9).contains(&b) {
                        docref_warning(
                            vm,
                            &format!(
                                "Invalid parameter given for number of blocks to allocate ({})",
                                b
                            ),
                        );
                    } else {
                        blocks = b as u32;
                    }
                }
                if let Some(t) = param_find(p, b"work") {
                    let w = t.to_long();
                    if !(0..=250).contains(&w) {
                        docref_warning(
                            vm,
                            &format!("Invalid parameter given for work factor ({})", w),
                        );
                    } else {
                        work = w as u32;
                    }
                }
            }
        }
        Some(Builtin::BzCompress(Box::new(BzCompress {
            enc: bzip2::Compress::new(bzip2::Compression::new(blocks), work),
            is_flushed: true,
        })))
    } else {
        None
    }
}

impl BzDecompress {
    /// php_bz2_decompress_filter()
    fn filter(
        &mut self,
        vm: &mut Vm,
        inp: &mut Brigade,
        out: &mut Brigade,
        consumed: Option<&mut usize>,
        flags: i32,
    ) -> i32 {
        let mut exit = PSFS_FEED_ME;
        let mut total = 0usize;
        while let Some(b) = inp.pop_front() {
            let buf = std::mem::take(&mut b.borrow_mut().buf);
            let mut bin = 0;
            while bin < buf.len() {
                if self.status == BzStatus::Uninitialized {
                    self.dec = Some(bzip2::Decompress::new(self.small));
                    self.status = BzStatus::Running;
                }
                if self.status != BzStatus::Running {
                    total += buf.len();
                    break;
                }
                let desired = (buf.len() - bin).min(BZ_BUF);
                let dec = self.dec.as_mut().unwrap();
                let mut obuf = vec![0u8; BZ_BUF];
                let before_in = dec.total_in();
                let before_out = dec.total_out();
                let r = dec.decompress(&buf[bin..bin + desired], &mut obuf);
                let used = (dec.total_in() - before_in) as usize;
                let made = (dec.total_out() - before_out) as usize;
                let ended = matches!(r, Ok(bzip2::Status::StreamEnd));
                match r {
                    Ok(bzip2::Status::StreamEnd) => {
                        self.status = if self.concatenated {
                            BzStatus::Uninitialized
                        } else {
                            BzStatus::Finished
                        };
                    }
                    Ok(_) => {}
                    Err(_) => {
                        docref_notice(vm, "bzip2 decompression failed");
                        return PSFS_ERR_FATAL;
                    }
                }
                total += used;
                bin += used;
                if made > 0 {
                    obuf.truncate(made);
                    out.append(new_bucket(obuf));
                    exit = PSFS_PASS_ON;
                } else if ended {
                    return PSFS_PASS_ON;
                } else if used == 0 {
                    // No progress possible with this input.
                    break;
                }
            }
        }
        if self.status == BzStatus::Running && flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
            if let Some(dec) = self.dec.as_mut() {
                loop {
                    let mut obuf = vec![0u8; BZ_BUF];
                    let before_out = dec.total_out();
                    let r = dec.decompress(&[], &mut obuf);
                    let made = (dec.total_out() - before_out) as usize;
                    if made > 0 {
                        obuf.truncate(made);
                        out.append(new_bucket(obuf));
                        exit = PSFS_PASS_ON;
                    } else {
                        break;
                    }
                    if !matches!(r, Ok(bzip2::Status::Ok)) {
                        break;
                    }
                }
            }
        }
        if let Some(c) = consumed {
            *c = total;
        }
        exit
    }
}

impl BzCompress {
    fn run(&mut self, input: &[u8], action: bzip2::Action, out: &mut Brigade) -> Result<bool, ()> {
        let mut made_any = false;
        let mut pos = 0;
        loop {
            let mut obuf = vec![0u8; BZ_BUF];
            let before_in = self.enc.total_in();
            let before_out = self.enc.total_out();
            let r = self.enc.compress(&input[pos..], &mut obuf, action);
            let used = (self.enc.total_in() - before_in) as usize;
            let made = (self.enc.total_out() - before_out) as usize;
            pos += used;
            if made > 0 {
                obuf.truncate(made);
                out.append(new_bucket(obuf));
                made_any = true;
            }
            match r {
                Ok(bzip2::Status::StreamEnd) => return Ok(made_any),
                Ok(bzip2::Status::RunOk) => {
                    if pos >= input.len() {
                        return Ok(made_any);
                    }
                }
                Ok(bzip2::Status::FlushOk) | Ok(bzip2::Status::FinishOk) => {}
                Ok(_) => {
                    if pos >= input.len() && made == 0 {
                        return Ok(made_any);
                    }
                }
                Err(_) => return Err(()),
            }
            if pos >= input.len() && made == 0 && matches!(action, bzip2::Action::Run) {
                return Ok(made_any);
            }
        }
    }

    /// php_bz2_compress_filter()
    fn filter(
        &mut self,
        inp: &mut Brigade,
        out: &mut Brigade,
        consumed: Option<&mut usize>,
        flags: i32,
    ) -> i32 {
        let mut exit = PSFS_FEED_ME;
        let mut total = 0usize;
        while let Some(b) = inp.pop_front() {
            let buf = std::mem::take(&mut b.borrow_mut().buf);
            total += buf.len();
            if buf.is_empty() {
                continue;
            }
            let action = if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
                bzip2::Action::Finish
            } else if flags & PSFS_FLAG_FLUSH_INC != 0 {
                bzip2::Action::Flush
            } else {
                bzip2::Action::Run
            };
            self.is_flushed = !matches!(action, bzip2::Action::Run);
            match self.run(&buf, action, out) {
                Ok(true) => exit = PSFS_PASS_ON,
                Ok(false) => {}
                Err(()) => return PSFS_ERR_FATAL,
            }
        }
        if flags & PSFS_FLAG_FLUSH_CLOSE != 0
            || (flags & PSFS_FLAG_FLUSH_INC != 0 && !self.is_flushed)
        {
            let action = if flags & PSFS_FLAG_FLUSH_CLOSE != 0 {
                bzip2::Action::Finish
            } else {
                bzip2::Action::Flush
            };
            if let Ok(true) = self.run(&[], action, out) {
                exit = PSFS_PASS_ON;
            }
            self.is_flushed = true;
        }
        if let Some(c) = consumed {
            *c = total;
        }
        exit
    }
}
