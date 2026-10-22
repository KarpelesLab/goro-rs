//! ext/sysvshm (sysvshm.c): PHP variables in System V shared memory, in
//! php-src's chunk layout (so segments are shared with php itself).

use std::cell::Cell;

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp;
use crate::{R, errno, native, new_object, strerror, warn, xlong};

const CLASS: &str = "SysvSharedMemory";

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("shm_attach", shm_attach),
    ("shm_detach", shm_detach),
    ("shm_has_var", shm_has_var),
    ("shm_remove", shm_remove),
    ("shm_put_var", shm_put_var),
    ("shm_get_var", shm_get_var),
    ("shm_remove_var", shm_remove_var),
];

pub fn register(vm: &mut Vm) {
    if crate::register_ext(vm, b"sysvshm", FUNCTIONS, &[]) {
        crate::register_class(
            vm,
            CLASS.as_bytes(),
            "Cannot directly construct SysvSharedMemory, use shm_attach() instead",
        );
    }
}

/// sysvshm_chunk_head
#[repr(C)]
struct ChunkHead {
    magic: [u8; 8],
    start: usize,
    end: usize,
    free: usize,
    total: usize,
}

/// sysvshm_chunk (`mem` follows)
#[repr(C)]
struct Chunk {
    key: i64,
    length: usize,
    next: usize,
}

const CHUNK_SIZE: usize = std::mem::size_of::<Chunk>() + std::mem::size_of::<usize>();

/// sysvshm_shm
struct Shm {
    key: i32,
    id: i64,
    ptr: Cell<*mut ChunkHead>,
}

impl Drop for Shm {
    /// sysvshm_free_obj()
    fn drop(&mut self) {
        let p = self.ptr.get();
        if !p.is_null() {
            unsafe {
                libc::shmdt(p as *const libc::c_void);
            }
        }
    }
}

/// php_sysvshm.init_mem (sysvshm.init_mem from php.ini)
const INIT_MEM: i64 = 10000;

fn shm_attach(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shm_attach";
    let Ok(a) = zpp::parse(vm, f, CLASS, "l|zl", args) else {
        return Ok(Value::Null);
    };
    let key = a[0].long_or(0);
    // l! (as 'z' to tell null apart)
    let size = match &a[1] {
        zpp::Arg::Any(v) => match v.deref() {
            Value::Null => INIT_MEM,
            other => match zpp::coerce_long(vm, f, 2, &other) {
                Some(n) => n,
                None => return Ok(Value::Null),
            },
        },
        _ => INIT_MEM,
    };
    let flag = a[2].long_or(0o666);
    if size < 1 {
        zpp::value_error(vm, f, 2, "must be greater than 0");
        return Ok(Value::Null);
    }
    let mut id = unsafe { libc::shmget(key as libc::key_t, 0, 0) } as i64;
    if id < 0 {
        if size < std::mem::size_of::<ChunkHead>() as i64 {
            warn(
                vm,
                f,
                &format!("Failed for key 0x{}: memorysize too small", xlong(key)),
            );
            return Ok(Value::False);
        }
        id = unsafe {
            libc::shmget(
                key as libc::key_t,
                size as usize,
                flag as i32 | libc::IPC_CREAT | libc::IPC_EXCL,
            )
        } as i64;
        if id < 0 {
            let e = errno();
            warn(
                vm,
                f,
                &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
            );
            return Ok(Value::False);
        }
    }
    let p = unsafe { libc::shmat(id as i32, std::ptr::null(), 0) };
    if p as isize == -1 {
        let e = errno();
        warn(
            vm,
            f,
            &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
        );
        return Ok(Value::False);
    }
    let head = p as *mut ChunkHead;
    unsafe {
        let magic = &mut (*head).magic;
        let len = magic.iter().position(|&c| c == 0).unwrap_or(8);
        if &magic[..len] != b"PHP_SM" {
            magic[..7].copy_from_slice(b"PHP_SM\0");
            (*head).start = std::mem::size_of::<ChunkHead>();
            (*head).end = (*head).start;
            (*head).total = size as usize;
            (*head).free = (size as usize).wrapping_sub((*head).end);
        }
    }
    let shm = Shm {
        key: key as i32,
        id,
        ptr: Cell::new(head),
    };
    Ok(new_object(vm, CLASS.as_bytes(), shm))
}

/// The attached segment of the object argument, or the "already
/// destroyed" Error.
fn attached(
    vm: &mut Vm,
    f: &str,
    spec: &str,
    args: &[Value],
) -> Option<(Vec<zpp::Arg>, std::rc::Rc<Shm>)> {
    let a = zpp::parse(vm, f, CLASS, spec, args).ok()?;
    let s = native::<Shm>(&a[0])?;
    if s.ptr.get().is_null() {
        zpp::throw(
            vm,
            b"Error",
            "Shared memory block has already been destroyed",
        );
        return None;
    }
    Some((a, s))
}

fn shm_detach(vm: &mut Vm, args: &[Value]) -> R {
    let Some((_, s)) = attached(vm, "shm_detach", "O", args) else {
        return Ok(Value::Null);
    };
    unsafe {
        libc::shmdt(s.ptr.get() as *const libc::c_void);
    }
    s.ptr.set(std::ptr::null_mut());
    Ok(Value::True)
}

fn shm_remove(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shm_remove";
    let Some((_, s)) = attached(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    if unsafe { libc::shmctl(s.id as i32, libc::IPC_RMID, std::ptr::null_mut()) } < 0 {
        let e = errno();
        // (Z_LVAL_P() of the object: its handle)
        warn(
            vm,
            f,
            &format!("Failed for key 0x{:x}, id {}: {}", s.key, s.id, strerror(e)),
        );
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn shm_put_var(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shm_put_var";
    let Some((a, s)) = attached(vm, f, "Olz", args) else {
        return Ok(Value::Null);
    };
    let key = a[1].long_or(0);
    let var = match &a[2] {
        zpp::Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    let Some(data) = goro_ext_standard::serialize::php_serialize(vm, &var) else {
        return Ok(Value::Null);
    };
    if s.ptr.get().is_null() {
        zpp::throw(
            vm,
            b"Error",
            "Shared memory block has been destroyed by the serialization function",
        );
        return Ok(Value::Null);
    }
    if !unsafe { put_shm_data(s.ptr.get(), key, &data) } {
        warn(vm, f, "Not enough shared memory left");
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn shm_get_var(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shm_get_var";
    let Some((a, s)) = attached(vm, f, "Ol", args) else {
        return Ok(Value::Null);
    };
    let key = a[1].long_or(0);
    let pos = unsafe { check_shm_data(s.ptr.get(), key) };
    if pos == -1 {
        warn(vm, f, &format!("Variable key {} doesn't exist", key));
        return Ok(Value::False);
    }
    let data = unsafe {
        let chunk = (s.ptr.get() as *mut u8).add(pos as usize) as *const Chunk;
        let mem = (chunk as *const u8).add(std::mem::size_of::<Chunk>());
        std::slice::from_raw_parts(mem, (*chunk).length).to_vec()
    };
    match goro_ext_standard::serialize::php_var_unserialize(vm, &data) {
        Some(v) => Ok(v),
        None => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            warn(vm, f, "Variable data in shared memory is corrupted");
            Ok(Value::False)
        }
    }
}

fn shm_has_var(vm: &mut Vm, args: &[Value]) -> R {
    let Some((a, s)) = attached(vm, "shm_has_var", "Ol", args) else {
        return Ok(Value::Null);
    };
    let key = a[1].long_or(0);
    Ok(crate::bval(
        unsafe { check_shm_data(s.ptr.get(), key) } >= 0,
    ))
}

fn shm_remove_var(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shm_remove_var";
    let Some((a, s)) = attached(vm, f, "Ol", args) else {
        return Ok(Value::Null);
    };
    let key = a[1].long_or(0);
    let pos = unsafe { check_shm_data(s.ptr.get(), key) };
    if pos == -1 {
        warn(vm, f, &format!("Variable key {} doesn't exist", key));
        return Ok(Value::False);
    }
    unsafe { remove_shm_data(s.ptr.get(), pos as usize) };
    Ok(Value::True)
}

/// php_put_shm_data()
unsafe fn put_shm_data(ptr: *mut ChunkHead, key: i64, data: &[u8]) -> bool {
    let l = std::mem::size_of::<i64>();
    let total_size = ((data.len() + CHUNK_SIZE - 1) / l) * l + l;
    unsafe {
        let pos = check_shm_data(ptr, key);
        if pos > 0 {
            remove_shm_data(ptr, pos as usize);
        }
        if (*ptr).free < total_size {
            return false;
        }
        let chunk = (ptr as *mut u8).add((*ptr).end) as *mut Chunk;
        (*chunk).key = key;
        (*chunk).length = data.len();
        (*chunk).next = total_size;
        let mem = (chunk as *mut u8).add(std::mem::size_of::<Chunk>());
        std::ptr::copy_nonoverlapping(data.as_ptr(), mem, data.len());
        (*ptr).end += total_size;
        (*ptr).free -= total_size;
    }
    true
}

/// php_check_shm_data(): the position of `key`'s chunk, or -1.
unsafe fn check_shm_data(ptr: *mut ChunkHead, key: i64) -> isize {
    unsafe {
        let mut pos = (*ptr).start;
        loop {
            if pos >= (*ptr).end {
                return -1;
            }
            let chunk = (ptr as *const u8).add(pos) as *const Chunk;
            if (*chunk).key == key {
                return pos as isize;
            }
            pos = pos.wrapping_add((*chunk).next);
            if (*chunk).next == 0 || pos < (*ptr).start {
                return -1;
            }
        }
    }
}

/// php_remove_shm_data()
unsafe fn remove_shm_data(ptr: *mut ChunkHead, pos: usize) {
    unsafe {
        let chunk = (ptr as *mut u8).add(pos) as *mut Chunk;
        let next = (*chunk).next;
        let next_chunk = (chunk as *mut u8).add(next);
        let len = (*ptr).end - pos - next;
        (*ptr).free += next;
        (*ptr).end -= next;
        if len > 0 {
            std::ptr::copy(next_chunk, chunk as *mut u8, len);
        }
    }
}
