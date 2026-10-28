//! Phar, PharData and PharFileInfo (PHP manual: Phar, PharData,
//! PharFileInfo): the native methods. The classes are declared in PHP
//! (phar.php, phardata.php, pharfileinfo.php) as directory iterators over
//! the archive's `phar://` root. Messages from ext/phar/tests and
//! observations on PHP 8.5.

use goro_ext_phar::{Comp, Entry, Format, SigType};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::{Shared, flush, forget, load, lossy, name_info, readonly, split_url, state};
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const PHAR_CLASS: &str = concat!(include_str!("phar.php"), include_str!("shared.php"));
pub const PHARDATA_CLASS: &str = concat!(include_str!("phardata.php"), include_str!("shared.php"));
pub const PHARFILEINFO_CLASS: &str = include_str!("pharfileinfo.php");

pub fn hooks() -> NativeHooks {
    NativeHooks::default()
}

/// The archive a Phar/PharData object works on.
#[derive(Clone)]
struct PharObj {
    key: Vec<u8>,
    is_data: bool,
}
impl goro_runtime::NativeData for PharObj {}

/// The entry a PharFileInfo object stands for.
#[derive(Clone)]
struct InfoObj {
    key: Vec<u8>,
    inner: Vec<u8>,
}
impl goro_runtime::NativeData for InfoObj {}

fn native<T: Clone + 'static>(o: &goro_runtime::ObjRef) -> Option<T> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<T>().cloned()
}

macro_rules! methods {
    ([$($name:literal => $f:expr),* $(,)?] [$(($iname:literal, $if:expr)),* $(,)?]) => {
        &[
            $((concat!("phar::", $name), $f as NativeFn), (concat!("phardata::", $name), $f as NativeFn),)*
            $(($iname, $if as NativeFn),)*
        ]
    };
}

pub static METHODS: &[(&str, NativeFn)] = methods!([
    "__h_pharinit" => phar_init,
    "addemptydir" => add_empty_dir,
    "addfile" => add_file,
    "addfromstring" => add_from_string,
    "__h_checkwrite" => h_check_write,
    "__h_addfile" => h_add_file,
    "__h_adddata" => h_add_data,
    "__h_save" => h_save,
    "compressfiles" => compress_files,
    "decompressfiles" => decompress_files,
    "compress" => compress,
    "decompress" => decompress,
    "converttoexecutable" => convert_to_executable,
    "converttodata" => convert_to_data,
    "copy" => copy,
    "count" => count,
    "delete" => delete,
    "delmetadata" => del_metadata,
    "extractto" => extract_to,
    "getalias" => get_alias,
    "getpath" => get_path,
    "getmetadata" => get_metadata,
    "getmodified" => get_modified,
    "getsignature" => get_signature,
    "getstub" => get_stub,
    "getversion" => get_version,
    "hasmetadata" => has_metadata,
    "isbuffering" => is_buffering,
    "iscompressed" => is_compressed,
    "isfileformat" => is_file_format,
    "iswritable" => is_writable,
    "offsetexists" => offset_exists,
    "offsetget" => offset_get,
    "offsetset" => offset_set,
    "offsetunset" => offset_unset,
    "setalias" => set_alias,
    "setdefaultstub" => set_default_stub,
    "setmetadata" => set_metadata,
    "setsignaturealgorithm" => set_signature_algorithm,
    "setstub" => set_stub,
    "startbuffering" => start_buffering,
    "stopbuffering" => stop_buffering,
    "apiversion" => api_version,
    "cancompress" => can_compress,
    "canwrite" => can_write,
    "createdefaultstub" => create_default_stub,
    "getsupportedcompression" => get_supported_compression,
    "getsupportedsignatures" => get_supported_signatures,
    "interceptfilefuncs" => intercept_file_funcs,
    "isvalidpharfilename" => is_valid_phar_filename,
    "loadphar" => load_phar,
    "mapphar" => map_phar,
    "running" => running,
    "mount" => mount,
    "mungserver" => mung_server,
    "unlinkarchive" => unlink_archive,
    "webphar" => web_phar,
] [
    ("pharfileinfo::__h_pharinit", info_init),
    ("pharfileinfo::chmod", info_chmod),
    ("pharfileinfo::compress", info_compress),
    ("pharfileinfo::decompress", info_decompress),
    ("pharfileinfo::delmetadata", info_del_metadata),
    ("pharfileinfo::getcrc32", info_get_crc32),
    ("pharfileinfo::getcompressedsize", info_get_compressed_size),
    ("pharfileinfo::getcontent", info_get_content),
    ("pharfileinfo::getmetadata", info_get_metadata),
    ("pharfileinfo::getpharflags", info_get_phar_flags),
    ("pharfileinfo::hasmetadata", info_has_metadata),
    ("pharfileinfo::iscrcchecked", info_is_crc_checked),
    ("pharfileinfo::iscompressed", info_is_compressed),
    ("pharfileinfo::setmetadata", info_set_metadata),
]);

// ---------------------------------------------------------------- helpers

fn this_phar(vm: &mut Vm, a: &Args) -> R<(Shared, PharObj)> {
    let o = a.this_obj().clone();
    let Some(p) = native::<PharObj>(&o) else {
        return vm.throw(
            "BadMethodCallException",
            "Cannot call method on an uninitialized Phar object",
        );
    };
    match state(vm).archives.get(&p.key).cloned() {
        Some(s) => Ok((s, p)),
        None => {
            // unlinked: opened again from disk if still there
            match load(vm, &p.key, true, p.is_data) {
                Ok(s) => Ok((s, p)),
                Err(m) => vm.throw("UnexpectedValueException", &m),
            }
        }
    }
}

fn this_info(vm: &mut Vm, a: &Args) -> R<(Shared, InfoObj)> {
    let o = a.this_obj().clone();
    let Some(p) = native::<InfoObj>(&o) else {
        return vm.throw(
            "BadMethodCallException",
            "Cannot call method on an uninitialized PharFileInfo object",
        );
    };
    let is_data = !name_info(&p.key).exec;
    match load(vm, &p.key, false, is_data) {
        Ok(s) => Ok((s, p)),
        Err(m) => vm.throw("UnexpectedValueException", &m),
    }
}

/// Throws when the archive can't be written (PharData always can).
fn check_write(vm: &mut Vm, s: &Shared, class: &str, msg: &str) -> R<()> {
    if !s.borrow().is_data && readonly(vm) {
        return vm.throw(class, msg);
    }
    Ok(())
}

const RO: &str = "Write operations disabled by the php.ini setting phar.readonly";

/// Writes the archive, a failure being a PharException.
fn save(vm: &mut Vm, s: &Shared) -> R<()> {
    if let Err(m) = flush(vm, s) {
        return vm.throw("PharException", &m);
    }
    Ok(())
}

fn fmt_name(s: &Shared) -> String {
    lossy(&s.borrow().a.fname)
}

/// A PharFileInfo (or the info class) for an entry.
fn new_info(vm: &mut Vm, a: &Args, key: &[u8], inner: &[u8]) -> R<Value> {
    let o = a.this_obj().clone();
    let cls_name = match vm.get_prop_raw(&o, b"__infoClass") {
        Value::Str(s) if !s.is_empty() => s.to_vec(),
        _ => b"PharFileInfo".to_vec(),
    };
    let Some(cls) = vm.find_class(&cls_name, true)? else {
        return Err(vm.fatal("Class \"PharFileInfo\" not found"));
    };
    let mut url = b"phar://".to_vec();
    url.extend_from_slice(key);
    url.push(b'/');
    url.extend_from_slice(inner);
    let obj = vm.instantiate(&cls)?;
    if let Some(f) = cls.magic.borrow().construct.clone() {
        vm.call_func_this(&f, Some(obj.clone()), vec![Value::Str(PhpStr::from(url))])?;
    }
    Ok(Value::Object(obj))
}

/// Why a local name can't be used: a double slash or a `..` component
/// (tests phar_offset_get_error.phpt, addfuncs.phpt, phar_copy.phpt).
fn bad_path(raw: &[u8]) -> Option<&'static str> {
    let r = raw.strip_prefix(b"/").unwrap_or(raw);
    if r.windows(2).any(|w| w == b"//") {
        return Some("contains double slash");
    }
    if r.split(|&c| c == b'/').any(|c| c == b"..") {
        return Some("contains upper directory reference");
    }
    None
}

/// Characters an alias can't have (test phar_setalias2, badalias tests).
fn bad_alias(al: &[u8]) -> bool {
    al.iter().any(|c| matches!(c, b'/' | b'\\' | b':' | b';'))
}

/// A local name for an entry: no leading slash, `.`/`..` resolved.
fn local_name(n: &[u8]) -> Vec<u8> {
    super::norm_inner(n)
}

fn magic_check(vm: &mut Vm, name: &[u8]) -> R<bool> {
    if goro_ext_phar::is_meta_name(name) {
        vm.throw(
            "BadMethodCallException",
            "Cannot create any files in magic \".phar\" directory",
        )?;
        return Ok(false);
    }
    Ok(true)
}

/// Adds or replaces a file and saves.
fn put_file(vm: &mut Vm, s: &Shared, name: &[u8], data: Vec<u8>) -> R<()> {
    {
        let mut l = s.borrow_mut();
        match l.a.get_mut(name) {
            Some(e) if !e.is_dir => e.set_contents(data),
            _ => {
                l.a.put(Entry::file(name, data, 0, new_perms()));
            }
        }
    }
    save(vm, s)
}

fn comp_flag(c: Comp) -> i64 {
    match c {
        Comp::None => 0,
        Comp::Gz => 4096,
        Comp::Bz2 => 8192,
    }
}

// ---------------------------------------------------------------- construction

/// `__h_pharInit($filename, $alias, $isData, $format)`: opens or creates
/// the archive and gives the phar:// directory to iterate.
fn phar_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fname = a.str(0);
    let alias = a.opt_str(1);
    let is_data = a.bool(2);
    let format = a.int_or(3, 0);
    let cls = if is_data { "PharData" } else { "Phar" };
    if fname.windows(3).any(|w| w == b"://") && !fname.starts_with(b"phar://") {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "Cannot create a phar archive from a URL like \"{}\". Phar objects can only be created from local files",
                fname.to_string_lossy()
            ),
        );
    }
    let (key, inner) = if fname.starts_with(b"phar://") {
        match split_url(vm, &fname) {
            Ok(x) => x,
            Err(m) => return vm.throw("UnexpectedValueException", &m),
        }
    } else {
        (super::abs_path(vm, &fname), Vec::new())
    };
    let exists = state(vm).archives.contains_key(&key) || super::os_path(&key).is_file();
    if !exists {
        let ni = name_info(&key);
        let dir_ok = super::os_path(&key).parent().is_some_and(|p| p.is_dir());
        let ext_ok = if is_data {
            ni.has_ext && !ni.exec
        } else {
            ni.exec
        };
        if !ext_ok || !dir_ok {
            if is_data && ni.exec && dir_ok {
                let ext = &key[key.iter().rposition(|&c| c == b'/').unwrap_or(0)..];
                let ext = ext
                    .iter()
                    .skip(2)
                    .position(|&c| c == b'.')
                    .map(|p| &ext[p + 3..])
                    .unwrap_or(b"");
                return vm.throw(
                    "UnexpectedValueException",
                    &format!(
                        "data phar \"{}\" has invalid extension {}",
                        lossy(&key),
                        lossy(ext)
                    ),
                );
            }
            return vm.throw(
                "UnexpectedValueException",
                &format!(
                    "Cannot create phar '{}', file extension (or combination) not recognised or the directory does not exist",
                    fname.to_string_lossy()
                ),
            );
        }
    }
    if let Some(al) = &alias
        && let Some(other) = state(vm).aliases.get(&al.to_vec()).cloned()
        && other != key
    {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
                al.to_string_lossy(),
                lossy(&other),
                lossy(&key)
            ),
        );
    }
    let s = match load(vm, &key, true, is_data) {
        Ok(s) => s,
        Err(m) => return vm.throw("UnexpectedValueException", &m),
    };
    {
        let mut l = s.borrow_mut();
        if !exists && is_data && name_info(&key).format == Format::Phar {
            // PharData without a tar/zip extension: the format asked for
            l.a.format = if format == 3 {
                Format::Zip
            } else {
                Format::Tar
            };
        }
        if is_data && !l.on_disk {
            l.a.sig_type = None;
        }
    }
    if let Some(al) = alias {
        let al = al.to_vec();
        let cur = s.borrow().a.alias.clone();
        if let Some(c) = &cur
            && c != &al
            && s.borrow().on_disk
        {
            // test alias_acrobatics.phpt
            return vm.throw(
                "UnexpectedValueException",
                &format!(
                    "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
                    lossy(&al),
                    lossy(&key),
                    lossy(&key)
                ),
            );
        }
        if cur.as_deref() != Some(&al[..]) {
            state(vm).aliases.insert(al.clone(), key.clone());
            if cur.is_none() || !s.borrow().on_disk {
                s.borrow_mut().a.alias = Some(al);
            }
        }
    }
    let _ = cls;
    *a.this_obj().obj().native.borrow_mut() = Some(Box::new(PharObj {
        key: key.clone(),
        is_data,
    }));
    let mut root = b"phar://".to_vec();
    root.extend_from_slice(&key);
    if !inner.is_empty() {
        root.push(b'/');
        root.extend_from_slice(&inner);
    }
    Ok(Value::Str(PhpStr::from(root)))
}

fn info_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    if !f.starts_with(b"phar://") {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "'{}' is not a valid phar archive URL (must have at least phar://filename.phar)",
                f.to_string_lossy()
            ),
        );
    }
    let (key, inner) = match split_url(vm, &f) {
        Ok(x) => x,
        Err(_) => {
            return vm.throw(
                "UnexpectedValueException",
                &format!(
                    "'{}' is not a valid phar archive URL (must have at least phar://filename.phar)",
                    f.to_string_lossy()
                ),
            );
        }
    };
    let is_data = !name_info(&key).exec;
    let s = match load(vm, &key, false, is_data) {
        Ok(s) => s,
        Err(m) => return vm.throw("UnexpectedValueException", &m),
    };
    let ok = {
        let l = s.borrow();
        l.a.get(&inner).is_some() || (!inner.is_empty() && l.a.is_dir(&inner))
    };
    if !ok {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "Cannot access phar file entry '{}' in archive '{}'",
                lossy(&inner),
                lossy(&key)
            ),
        );
    }
    *a.this_obj().obj().native.borrow_mut() = Some(Box::new(InfoObj { key, inner }));
    Ok(Value::Null)
}

// ---------------------------------------------------------------- contents

fn add_empty_dir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "UnexpectedValueException", RO)?;
    let d = local_name(a.bytes(0));
    if goro_ext_phar::is_meta_name(&d) {
        return vm.throw(
            "BadMethodCallException",
            "Cannot create a directory in magic \".phar\" directory",
        );
    }
    s.borrow_mut().a.put(Entry::dir(&d, 0));
    save(vm, &s)?;
    Ok(Value::Null)
}

fn add_from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "UnexpectedValueException", RO)?;
    let n = local_name(a.bytes(0));
    if !magic_check(vm, &n)? {
        return Ok(Value::Null);
    }
    put_file(vm, &s, &n, a.bytes(1).to_vec())?;
    Ok(Value::Null)
}

/// Reads a whole file through the stream layer.
fn read_file(vm: &mut Vm, a: &Args, path: &[u8]) -> R<Option<Vec<u8>>> {
    let Some(mut st) = crate::builtins::file::open_stream(vm, a, path, b"rb", true)? else {
        return Ok(None);
    };
    let data = crate::builtins::file::read_rest(vm, &mut st);
    crate::builtins::streams::close_stream(vm, &mut st)?;
    Ok(Some(data))
}

fn add_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "UnexpectedValueException", RO)?;
    let f = a.str(0);
    let name = match a.opt_str(1) {
        Some(n) => local_name(&n),
        None => local_name(&f),
    };
    if !magic_check(vm, &name)? {
        return Ok(Value::Null);
    }
    let Some(data) = read_file(vm, a, &f)? else {
        return vm.throw(
            "RuntimeException",
            &format!(
                "phar error: unable to open file \"{}\" to add to phar archive",
                f.to_string_lossy()
            ),
        );
    };
    // the file's permissions (test bug77022.phpt)
    let real = super::abs_path(vm, &f);
    if let Some(perms) = file_perms(&real) {
        let mut e = Entry::file(&name, data, 0, perms);
        e.flags = perms;
        s.borrow_mut().a.put(e);
        save(vm, &s)?;
        return Ok(Value::Null);
    }
    put_file(vm, &s, &name, data)?;
    Ok(Value::Null)
}

fn offset_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "BadMethodCallException", RO)?;
    let raw = vm.to_php_str(&a.val(0))?;
    if raw.contains(&0) {
        return vm.throw(
            "ValueError",
            &format!(
                "{}(): Argument #1 ($localName) must not contain any null bytes",
                a.fname()
            ),
        );
    }
    if let Some(why) = bad_path(&raw) {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Entry {} does not exist and cannot be created: phar error: invalid path \"{}\" {}",
                raw.to_string_lossy(),
                raw.to_string_lossy(),
                why
            ),
        );
    }
    let n = local_name(&raw);
    if s.borrow().open_writes > 0 && s.borrow().a.get(&n).is_some() {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Entry {} does not exist and cannot be created: phar error: file \"{}\" in phar \"{}\" cannot be opened for writing, readable file pointers are open",
                lossy(&n),
                lossy(&n),
                fmt_name(&s)
            ),
        );
    }
    if n == b".phar/stub.php" {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Cannot set stub \".phar/stub.php\" directly in phar \"{}\", use setStub",
                fmt_name(&s)
            ),
        );
    }
    if n == b".phar/alias.txt" {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Cannot set alias \".phar/alias.txt\" directly in phar \"{}\", use setAlias",
                fmt_name(&s)
            ),
        );
    }
    if !magic_check(vm, &n)? {
        return Ok(Value::Null);
    }
    let data = match a.val(1) {
        Value::Resource(_) => {
            let mut args = vec![a.val(1)];
            let v = vm.call_value(
                &Value::from("stream_get_contents"),
                std::mem::take(&mut args),
            )?;
            vm.to_php_str(&v)?.to_vec()
        }
        Value::Object(_) | Value::Array(_) => {
            return vm.throw(
                "BadMethodCallException",
                "Entry must be a string or a resource",
            );
        }
        v => vm.to_php_str(&v)?.to_vec(),
    };
    if s.borrow().a.get(&n).is_some_and(|e| e.is_dir) {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Cannot create file \"{}\" in phar \"{}\", a directory exists with that name",
                lossy(&n),
                fmt_name(&s)
            ),
        );
    }
    put_file(vm, &s, &n, data)?;
    Ok(Value::Null)
}

fn offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    let raw = vm.to_php_str(&a.val(0))?;
    let n = local_name(&raw);
    if goro_ext_phar::is_meta_name(&n)
        && !matches!(&n[..], b".phar/stub.php" | b".phar/alias.txt" | b".phar/signature.bin")
    {
        return vm.throw(
            "BadMethodCallException",
            "Cannot directly get any files or directories in magic \".phar\" directory",
        );
    }
    let ok = {
        let l = s.borrow();
        !goro_ext_phar::is_meta_name(&n)
            && (l.a.get(&n).is_some() || (!n.is_empty() && l.a.is_dir(&n)))
    };
    if !ok {
        return vm.throw(
            "BadMethodCallException",
            &format!("Entry {} does not exist", raw.to_string_lossy()),
        );
    }
    new_info(vm, a, &p.key, &n)
}

fn offset_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let raw = vm.to_php_str(&a.val(0))?;
    let n = local_name(&raw);
    let l = s.borrow();
    if goro_ext_phar::is_meta_name(&n) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(
        l.a.get(&n).is_some() || (!n.is_empty() && l.a.is_dir(&n)),
    ))
}

fn remove_entry(vm: &mut Vm, a: &Args, s: &Shared, n: &[u8], strict: bool) -> R<bool> {
    let found = s.borrow_mut().a.remove(n).is_some();
    if !found {
        if strict {
            vm.throw(
                "BadMethodCallException",
                &format!("Entry {} does not exist and cannot be deleted", lossy(n)),
            )?;
        }
        return Ok(false);
    }
    let _ = a;
    save(vm, s)?;
    Ok(true)
}

fn offset_unset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "BadMethodCallException", RO)?;
    let raw = vm.to_php_str(&a.val(0))?;
    let n = local_name(&raw);
    remove_entry(vm, a, &s, &n, false)?;
    Ok(Value::Null)
}

fn delete(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "BadMethodCallException",
        "Cannot write out phar archive, phar is read-only",
    )?;
    let n = local_name(a.bytes(0));
    remove_entry(vm, a, &s, &n, true)?;
    Ok(Value::Bool(true))
}

fn count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let n = s
        .borrow()
        .a
        .entries()
        .iter()
        .filter(|e| !goro_ext_phar::is_meta_name(&e.name))
        .count();
    Ok(Value::Int(n as i64))
}

fn copy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        &format!(
            "Cannot copy \"{}\" to \"{}\", phar is read-only",
            lossy(a.bytes(0)),
            lossy(a.bytes(1))
        ),
    )?;
    if bad_path(a.bytes(1)).is_some() {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "file \"{}\" contains invalid characters upper directory reference, cannot be copied from \"{}\" in phar {}",
                lossy(a.bytes(1)),
                lossy(a.bytes(0)),
                fmt_name(&s)
            ),
        );
    }
    let from = local_name(a.bytes(0));
    let to = local_name(a.bytes(1));
    let fail = |vm: &mut Vm, why: &str| -> R<Value> {
        vm.throw(
            "UnexpectedValueException",
            &format!(
                "file \"{}\" cannot be copied to file \"{}\", {} in {}",
                lossy(&from),
                lossy(&to),
                why,
                fmt_name(&s)
            ),
        )
    };
    if goro_ext_phar::is_meta_name(&from) {
        return fail(vm, "cannot copy Phar meta-file");
    }
    if goro_ext_phar::is_meta_name(&to) {
        return fail(vm, "cannot copy to Phar meta-file");
    }
    let src = {
        let l = s.borrow();
        l.a.get(&from).filter(|e| !e.is_dir).cloned()
    };
    let Some(mut e) = src else {
        return fail(vm, "file does not exist");
    };
    if s.borrow().a.get(&to).is_some() {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "file \"{}\" cannot be copied to file \"{}\", file must not already exist in phar {}",
                lossy(&from),
                lossy(&to),
                fmt_name(&s)
            ),
        );
    }
    e.name = to;
    s.borrow_mut().a.put(e);
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- properties

fn get_alias(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    Ok(match s.borrow().a.alias.clone() {
        Some(al) => Value::Str(PhpStr::from(al)),
        None => Value::Null,
    })
}

fn get_path(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, p) = this_phar(vm, a)?;
    Ok(Value::Str(PhpStr::from(p.key)))
}

fn get_version(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let api = s.borrow().a.api;
    Ok(Value::from(
        format!("{}.{}.{}", api >> 12, (api >> 8) & 15, (api >> 4) & 15).as_str(),
    ))
}

fn get_modified(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    Ok(Value::Bool(s.borrow().modified))
}

fn get_signature(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let sig = s.borrow().a.sig.clone();
    Ok(match sig {
        Some((t, b)) => {
            let mut arr = PhpArray::new();
            arr.insert(
                Key::from("hash"),
                Value::from(goro_ext_phar::sig::hex(&b).as_str()),
            );
            arr.insert(Key::from("hash_type"), Value::from(t.name()));
            Value::Array(arr)
        }
        None => Value::Bool(false),
    })
}

fn get_stub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let l = s.borrow();
    let stub = if l.a.stub.is_empty() && !l.is_data {
        match l.a.format {
            Format::Tar => goro_ext_phar::tar::DEFAULT_STUB.to_vec(),
            Format::Zip => goro_ext_phar::zip::DEFAULT_STUB.to_vec(),
            Format::Phar => Vec::new(),
        }
    } else {
        l.a.stub.clone()
    };
    Ok(Value::Str(PhpStr::from(stub)))
}

fn is_buffering(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    Ok(Value::Bool(s.borrow().buffering))
}

fn is_compressed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    Ok(match s.borrow().a.comp {
        Comp::None => Value::Bool(false),
        c => Value::Int(comp_flag(c)),
    })
}

fn is_file_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let f = s.borrow().a.format;
    Ok(Value::Bool(match a.int(0) {
        1 => f == Format::Phar,
        2 => f == Format::Tar,
        3 => f == Format::Zip,
        _ => return vm.throw("PharException", "Unknown file format specified"),
    }))
}

fn is_writable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    if !s.borrow().is_data && readonly(vm) {
        return Ok(Value::Bool(false));
    }
    let path = super::os_path(&p.key);
    Ok(Value::Bool(match std::fs::metadata(&path) {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => path.parent().is_some_and(|d| d.is_dir()),
    }))
}

fn get_metadata_of(vm: &mut Vm, meta: Option<Vec<u8>>, opts: &PhpArray) -> R<Value> {
    match meta {
        Some(m) => vm.unserialize_bytes(&m, opts),
        None => Ok(Value::Null),
    }
}

fn get_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let m = s.borrow().a.metadata.clone();
    let opts = if a.has(0) {
        a.array(0)
    } else {
        PhpArray::new()
    };
    get_metadata_of(vm, m, &opts)
}

fn has_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    Ok(Value::Bool(s.borrow().a.metadata.is_some()))
}

fn set_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "UnexpectedValueException", RO)?;
    let m = vm.serialize_value(&a.val(0))?;
    s.borrow_mut().a.metadata = Some(m);
    save(vm, &s)?;
    Ok(Value::Null)
}

fn del_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(vm, &s, "UnexpectedValueException", RO)?;
    if s.borrow_mut().a.metadata.take().is_none() {
        return Ok(Value::Bool(true));
    }
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn set_alias(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    if p.is_data {
        let what = if s.borrow().a.format == Format::Zip {
            "zip"
        } else {
            "tar"
        };
        return vm.throw(
            "UnexpectedValueException",
            &format!("A Phar alias cannot be set in a plain {what} archive"),
        );
    }
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot write out phar archive, phar is read-only",
    )?;
    let al = a.bytes(0).to_vec();
    if bad_alias(&al) {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "Invalid alias \"{}\" specified for phar \"{}\"",
                lossy(&al),
                lossy(&p.key)
            ),
        );
    }
    if let Some(other) = state(vm).aliases.get(&al).cloned()
        && other != p.key
    {
        return vm.throw(
            "UnexpectedValueException",
            &format!(
                "alias \"{}\" is already used for archive \"{}\" and cannot be used for other archives",
                lossy(&al),
                lossy(&other)
            ),
        );
    }
    let old = s.borrow_mut().a.alias.replace(al.clone());
    if let Some(o) = old {
        state(vm).aliases.remove(&o);
    }
    state(vm).aliases.insert(al, p.key.clone());
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn set_stub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    if p.is_data {
        let what = if s.borrow().a.format == Format::Zip {
            "zip"
        } else {
            "tar"
        };
        return vm.throw(
            "UnexpectedValueException",
            &format!("A Phar stub cannot be set in a plain {what} archive"),
        );
    }
    if !matches!(a.val(0), Value::Resource(_) | Value::Str(_)) {
        let other = a.val(0);
        return vm.throw(
            "TypeError",
            &format!(
                "{}(): Argument #1 ($stub) must be of type string, {} given",
                a.fname(),
                Vm::type_name_given(&other)
            ),
        );
    }
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot change stub, phar is read-only",
    )?;
    let stub = match a.val(0) {
        Value::Resource(_) => {
            // PHP 8.5 deprecation (test cache_list/copyonwrite10)
            vm.deprecated(&format!(
                "Calling {}(resource $stub, int $length) is deprecated",
                a.fname()
            ))?;
            let len = a.int_or(1, -1);
            let v = vm.call_value(
                &Value::from("stream_get_contents"),
                vec![
                    a.val(0),
                    if len < 0 {
                        Value::Null
                    } else {
                        Value::Int(len)
                    },
                ],
            )?;
            vm.to_php_str(&v)?.to_vec()
        }
        Value::Str(st) => st.to_vec(),
        other => {
            return vm.throw(
                "TypeError",
                &format!(
                    "{}(): Argument #1 ($stub) must be of type string, {} given",
                    a.fname(),
                    Vm::type_name_given(&other)
                ),
            );
        }
    };
    let format = s.borrow().a.format;
    let Some(norm) = goro_ext_phar::stub::normalize(&stub) else {
        let msg = match format {
            Format::Phar => format!(
                "illegal stub for phar \"{}\" (__HALT_COMPILER(); is missing)",
                lossy(&p.key)
            ),
            Format::Tar => format!("illegal stub for tar-based phar \"{}\"", lossy(&p.key)),
            Format::Zip => format!("illegal stub for zip-based phar \"{}\"", lossy(&p.key)),
        };
        return vm.throw("PharException", &msg);
    };
    let _ = format;
    s.borrow_mut().a.stub = norm;
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn set_default_stub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    if p.is_data {
        let what = if s.borrow().a.format == Format::Zip {
            "zip"
        } else {
            "tar"
        };
        return vm.throw(
            "UnexpectedValueException",
            &format!("A Phar stub cannot be set in a plain {what} archive"),
        );
    }
    let format = s.borrow().a.format;
    let index = a.opt_str(0);
    let web = a.opt_str(1);
    if format != Format::Phar && index.is_some() {
        return vm.throw(
            "ValueError",
            &format!(
                "{}(): Argument #1 ($index) must be null for a tar- or zip-based phar stub, string given",
                a.fname()
            ),
        );
    }
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot change stub: phar.readonly=1",
    )?;
    let stub = if format == Format::Phar {
        match goro_ext_phar::stub::default_stub(index.as_deref(), web.as_deref()) {
            Ok(st) => st,
            Err(m) => return vm.throw("UnexpectedValueException", &m),
        }
    } else if format == Format::Tar {
        goro_ext_phar::tar::DEFAULT_STUB.to_vec()
    } else {
        goro_ext_phar::zip::DEFAULT_STUB.to_vec()
    };
    s.borrow_mut().a.stub = stub;
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn set_signature_algorithm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot set signature algorithm, phar is read-only",
    )?;
    let Some(t) = SigType::from_flag(a.int(0) as u32) else {
        return vm.throw(
            "UnexpectedValueException",
            "Unknown signature algorithm specified",
        );
    };
    {
        let mut l = s.borrow_mut();
        l.a.sig_type = Some(t);
        if t.is_openssl() {
            l.private_key = a.opt_str(1).map(|k| k.to_vec());
        }
    }
    save(vm, &s)?;
    Ok(Value::Null)
}

fn start_buffering(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    s.borrow_mut().buffering = true;
    Ok(Value::Null)
}

fn stop_buffering(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot write out phar archive, phar is read-only",
    )?;
    s.borrow_mut().buffering = false;
    if let Err(m) = super::write_out(vm, &s) {
        return vm.throw("PharException", &m);
    }
    Ok(Value::Null)
}

// ---------------------------------------------------------------- compression and conversion

fn comp_of(vm: &mut Vm, n: i64) -> R<Comp> {
    Ok(match n {
        0 => Comp::None,
        4096 => Comp::Gz,
        8192 => Comp::Bz2,
        _ => {
            return vm.throw(
                "BadMethodCallException",
                "Unknown compression specified, please pass one of Phar::GZ or Phar::BZ2",
            );
        }
    })
}

fn compress_files(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "BadMethodCallException",
        "Phar is readonly, cannot change compression",
    )?;
    let c = comp_of(vm, a.int(0))?;
    if c == Comp::None {
        return vm.throw(
            "BadMethodCallException",
            "Unknown compression specified, please pass one of Phar::GZ or Phar::BZ2",
        );
    }
    if s.borrow().a.format == Format::Tar {
        return vm.throw(
            "BadMethodCallException",
            "Cannot compress with Gzip compression, tar archives cannot compress individual files, use compress() to compress the whole archive",
        );
    }
    set_all_comp(vm, &s, c)?;
    Ok(Value::Null)
}

fn set_all_comp(vm: &mut Vm, s: &Shared, c: Comp) -> R<()> {
    let r = {
        let mut l = s.borrow_mut();
        let fname = l.a.fname.clone();
        let mut r = Ok(());
        for e in l.a.entries_mut() {
            if let Err(m) = e.set_comp(c, &fname) {
                r = Err(m);
                break;
            }
        }
        r
    };
    if let Err(m) = r {
        return vm.throw("BadMethodCallException", &m);
    }
    save(vm, s)
}

fn decompress_files(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "BadMethodCallException",
        "Phar is readonly, cannot change compression",
    )?;
    set_all_comp(vm, &s, Comp::None)?;
    Ok(Value::Bool(true))
}

/// The file name of a converted archive: the base name before its
/// extension, then `ext`.
fn converted_name(key: &[u8], ext: &[u8]) -> Vec<u8> {
    // the archive extensions at the end go (test bug74196.phpt:
    // "x.1.2.3.phar.tar.gz" decompresses to "x.1.2.3.phar.tar")
    let slash = key.iter().rposition(|&c| c == b'/').map_or(0, |p| p + 1);
    let mut end = key.len();
    loop {
        let base = &key[slash..end];
        let Some(d) = base.iter().rposition(|&c| c == b'.').filter(|&d| d > 0) else {
            break;
        };
        let comp = base[d + 1..].to_ascii_lowercase();
        if matches!(&comp[..], b"phar" | b"tar" | b"zip" | b"tgz" | b"tbz" | b"gz" | b"bz2") {
            end = slash + d;
        } else {
            break;
        }
    }
    let mut out = key[..end].to_vec();
    if !ext.starts_with(b".") {
        out.push(b'.');
    }
    out.extend_from_slice(ext);
    out
}

fn default_ext(format: Format, comp: Comp, exec: bool) -> &'static str {
    match (exec, format, comp) {
        (true, Format::Phar, Comp::None) => "phar",
        (true, Format::Phar, Comp::Gz) => "phar.gz",
        (true, Format::Phar, Comp::Bz2) => "phar.bz2",
        (true, Format::Tar, Comp::None) => "phar.tar",
        (true, Format::Tar, Comp::Gz) => "phar.tar.gz",
        (true, Format::Tar, Comp::Bz2) => "phar.tar.bz2",
        (true, Format::Zip, _) => "phar.zip",
        (false, Format::Tar | Format::Phar, Comp::None) => "tar",
        (false, Format::Tar | Format::Phar, Comp::Gz) => "tar.gz",
        (false, Format::Tar | Format::Phar, Comp::Bz2) => "tar.bz2",
        (false, Format::Zip, _) => "zip",
    }
}

/// Writes a copy of the archive in another format/compression/name and
/// gives an object for it.
fn convert(
    vm: &mut Vm,
    a: &Args,
    s: &Shared,
    format: Format,
    comp: Comp,
    ext: Option<&[u8]>,
    exec: bool,
) -> R<Value> {
    if format == Format::Zip && comp != Comp::None {
        let w = if comp == Comp::Gz { "gzip" } else { "bz2" };
        return vm.throw(
            "BadMethodCallException",
            &format!("Cannot compress entire archive with {w}, zip archives do not support whole-archive compression"),
        );
    }
    let key = s.borrow().a.fname.clone();
    let ext = ext
        .map(|e| e.to_vec())
        .unwrap_or_else(|| default_ext(format, comp, exec).as_bytes().to_vec());
    let target = converted_name(&key, &ext);
    if exec && !name_info(&target).exec {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "phar \"{}\" has invalid extension {}",
                lossy(&target),
                lossy(&ext)
            ),
        );
    }
    if !exec && name_info(&target).exec {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "data phar \"{}\" has invalid extension {}",
                lossy(&target),
                lossy(&ext)
            ),
        );
    }
    if target == key || state(vm).archives.contains_key(&target) || super::os_path(&target).exists()
    {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "phar \"{}\" exists and must be unlinked prior to conversion",
                lossy(&target)
            ),
        );
    }
    let mut na = s.borrow().a.clone();
    na.fname = target.clone();
    na.format = format;
    na.comp = if format == Format::Zip {
        Comp::None
    } else {
        comp
    };
    na.is_data = !exec;
    // contents are read now: the copy doesn't depend on the old file
    {
        let fname = key.clone();
        for e in na.entries_mut() {
            if !e.is_dir && e.contents(&fname).is_err() {
                // left as stored
            }
            if format == Format::Tar {
                let _ = e.set_comp(Comp::None, &fname);
            }
        }
    }
    let from = s.borrow().a.format;
    if exec {
        // a new format gets its default stub; a tar its file name as alias,
        // other formats none (test phar_convert_repeated.phpt)
        if format != from || na.stub.is_empty() {
            na.stub = match format {
                Format::Phar => goro_ext_phar::stub::default_stub(None, None).unwrap_or_default(),
                Format::Tar => goro_ext_phar::tar::DEFAULT_STUB.to_vec(),
                Format::Zip => goro_ext_phar::zip::DEFAULT_STUB.to_vec(),
            };
        }
        if format != from {
            na.alias = (format == Format::Tar).then(|| target.clone());
        }
        if na.sig_type.is_none() {
            na.sig_type = Some(SigType::Sha256);
        }
    } else {
        na.sig_type = None;
        na.alias = None;
        na.stub.clear();
    }
    let loaded = super::Loaded {
        a: na,
        is_data: !exec,
        buffering: false,
        modified: true,
        on_disk: false,
        private_key: s.borrow().private_key.clone(),
        mounts: Vec::new(),
        open_writes: 0,
    };
    let ns = std::rc::Rc::new(std::cell::RefCell::new(loaded));
    if let Err(m) = super::write_out(vm, &ns) {
        return vm.throw("BadMethodCallException", &m);
    }
    state(vm).archives.insert(target.clone(), ns);
    let cls = if exec { "Phar" } else { "PharData" };
    let Some(c) = vm.find_class(cls.as_bytes(), false)? else {
        return Ok(Value::Null);
    };
    let o = vm.instantiate(&c)?;
    if let Some(f) = c.magic.borrow().construct.clone() {
        vm.call_func_this(&f, Some(o.clone()), vec![Value::Str(PhpStr::from(target))])?;
    }
    let _ = a;
    Ok(Value::Object(o))
}

fn format_of(vm: &mut Vm, n: Option<i64>, cur: Format) -> R<Format> {
    Ok(match n {
        None => cur,
        Some(1) => Format::Phar,
        Some(2) => Format::Tar,
        Some(3) => Format::Zip,
        Some(_) => {
            return vm.throw(
                "BadMethodCallException",
                "Unknown file format specified, please pass one of Phar::PHAR, Phar::TAR or Phar::ZIP",
            );
        }
    })
}

fn compress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "BadMethodCallException",
        "Cannot compress phar archive, phar is read-only",
    )?;
    let c = comp_of(vm, a.int(0))?;
    let format = s.borrow().a.format;
    let ext = a.opt_str(1);
    convert(vm, a, &s, format, c, ext.as_deref(), !p.is_data)
}

fn decompress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "BadMethodCallException",
        "Cannot decompress phar archive, phar is read-only",
    )?;
    let format = s.borrow().a.format;
    if format == Format::Zip {
        return vm.throw(
            "BadMethodCallException",
            "Cannot decompress zip-based archives with whole-archive compression",
        );
    }
    let ext = a.opt_str(0);
    convert(vm, a, &s, format, Comp::None, ext.as_deref(), !p.is_data)
}

fn convert_to_executable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    if readonly(vm) {
        return vm.throw(
            "UnexpectedValueException",
            "Cannot write out executable phar archive, phar is read-only",
        );
    }
    let cur = s.borrow().a.format;
    let format = format_of(vm, a.opt_int(0), cur)?;
    let c = match a.opt_int(1) {
        Some(n) => comp_of(vm, n)?,
        None => s.borrow().a.comp,
    };
    let ext = a.opt_str(2);
    convert(vm, a, &s, format, c, ext.as_deref(), true)
}

fn convert_to_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let cur = s.borrow().a.format;
    let format = format_of(
        vm,
        a.opt_int(0),
        if cur == Format::Phar {
            Format::Tar
        } else {
            cur
        },
    )?;
    if format == Format::Phar {
        return vm.throw(
            "BadMethodCallException",
            "Cannot write out data phar archive, use Phar::TAR or Phar::ZIP",
        );
    }
    let c = match a.opt_int(1) {
        Some(n) => comp_of(vm, n)?,
        None => s.borrow().a.comp,
    };
    let ext = a.opt_str(2);
    convert(vm, a, &s, format, c, ext.as_deref(), false)
}

// ---------------------------------------------------------------- building

/// `__h_checkWrite()`: buildFromIterator() on a read-only phar.
fn h_check_write(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    check_write(
        vm,
        &s,
        "UnexpectedValueException",
        "Cannot write out phar archive, phar is read-only",
    )?;
    Ok(Value::Null)
}

/// `__h_addFile($name, $path)`: adds a file from disk with its
/// permissions (test bug77022.phpt); its real path, none for phar's own
/// names, false when it can't be read.
fn h_add_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let name = local_name(&vm.to_php_str(&a.val(0))?);
    let path = vm.to_php_str(&a.val(1))?.to_vec();
    if goro_ext_phar::is_meta_name(&name) {
        return Ok(Value::Null);
    }
    let Some(data) = read_file(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    let real = super::abs_path(vm, &path);
    let perms = file_perms(&real).unwrap_or(0o666);
    let mut e = Entry::file(&name, data, 0, perms);
    e.flags = perms;
    s.borrow_mut().a.put(e);
    Ok(Value::Str(PhpStr::from(real)))
}

/// `__h_addData($name, $data)`: adds a file's contents (false for phar's
/// own names).
fn h_add_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let name = local_name(&vm.to_php_str(&a.val(0))?);
    if goro_ext_phar::is_meta_name(&name) {
        return Ok(Value::Bool(false));
    }
    let data = vm.to_php_str(&a.val(1))?.to_vec();
    s.borrow_mut().a.put(Entry::file(&name, data, 0, new_perms()));
    Ok(Value::Bool(true))
}

fn h_save(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    save(vm, &s)?;
    Ok(Value::Null)
}

/// The permissions of a file on disk.
fn file_perms(p: &[u8]) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(super::os_path(p))
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}

/// New entries: 0666 less the umask (test bug77022.phpt).
pub(crate) fn new_perms() -> u32 {
    // SAFETY: umask() only swaps the process mask; it is put back.
    let m = unsafe {
        let m = libc::umask(0o022);
        libc::umask(m);
        m
    };
    0o666 & !(m as u32)
}

fn extract_to(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, _) = this_phar(vm, a)?;
    let dir = a.str(0).to_vec();
    let overwrite = a.bool_or(2, false);
    let base = super::abs_path(vm, &dir);
    let bp = super::os_path(&base);
    if bp.is_file() {
        return vm.throw(
            "PharException",
            &format!(
                "Unable to use path \"{}\" for extraction, it is a file, must be a directory",
                lossy(&dir)
            ),
        );
    }
    let all: Vec<Vec<u8>> = s
        .borrow()
        .a
        .entries()
        .iter()
        .filter(|e| !goro_ext_phar::is_meta_name(&e.name))
        .map(|e| e.name.clone())
        .collect();
    let wanted: Vec<Vec<u8>> = match a.val(1) {
        Value::Null => all.clone(),
        Value::Str(f) => vec![local_name(&f)],
        Value::Array(arr) => {
            let mut v = Vec::new();
            for (_, x) in arr.iter() {
                match x.deref() {
                    Value::Str(f) => v.push(local_name(&f)),
                    _ => {
                        return vm.throw(
                            "PharException",
                            "Invalid argument, expected a filename (string) or array of filenames",
                        );
                    }
                }
            }
            v
        }
        _ => all.clone(),
    };
    for w in wanted {
        let list: Vec<Vec<u8>> = {
            let l = s.borrow();
            if l.a.get(&w).is_some() {
                let mut v = vec![w.clone()];
                if l.a.get(&w).is_some_and(|e| e.is_dir) {
                    let mut p = w.clone();
                    p.push(b'/');
                    v.extend(all.iter().filter(|n| n.starts_with(&p)).cloned());
                }
                v
            } else if l.a.is_dir(&w) {
                let mut p = w.clone();
                p.push(b'/');
                all.iter().filter(|n| n.starts_with(&p)).cloned().collect()
            } else {
                drop(l);
                return vm.throw(
                    "PharException",
                    &format!(
                        "phar error: attempted to extract non-existent file or directory \"{}\" from phar \"{}\"",
                        lossy(&w),
                        fmt_name(&s)
                    ),
                );
            }
        };
        for n in list {
            // names are kept inside the directory (test bug70019.phpt)
            let mut target = base.clone();
            target.push(b'/');
            target.extend_from_slice(&local_name(&n));
            let tp = super::os_path(&target);
            let (is_dir, data, perms) = {
                let mut l = s.borrow_mut();
                let fname = l.a.fname.clone();
                let e = l.a.get_mut(&n).unwrap();
                let perms = e.perms();
                if e.is_dir {
                    (true, Ok(Vec::new()), perms)
                } else {
                    (false, e.contents(&fname).map(|d| d.to_vec()), perms)
                }
            };
            if is_dir {
                let _ = std::fs::create_dir_all(&tp);
                continue;
            }
            if tp.exists() && !overwrite {
                continue;
            }
            let data = match data {
                Ok(d) => d,
                Err(m) => return vm.throw("PharException", &m),
            };
            if let Some(p) = tp.parent()
                && std::fs::create_dir_all(p).is_err()
            {
                return vm.throw(
                    "PharException",
                    &format!(
                        "Extraction from phar \"{}\" failed: Cannot extract \"{}\", could not create directory \"{}\"",
                        fmt_name(&s),
                        lossy(&n),
                        p.display()
                    ),
                );
            }
            let written = std::fs::write(&tp, &data);
            if written.is_ok() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tp, std::fs::Permissions::from_mode(perms));
            }
            if written.is_err() {
                return vm.throw(
                    "PharException",
                    &format!(
                        "Extraction from phar \"{}\" failed: Cannot extract \"{}\" to \"{}\", could not open for writing",
                        fmt_name(&s),
                        lossy(&n),
                        lossy(&target)
                    ),
                );
            }
        }
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- static methods

fn api_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::from("1.1.1"))
}

fn can_compress(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(matches!(a.int_or(0, 0), 0 | 4096 | 8192)))
}

fn can_write(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(!readonly(vm)))
}

fn create_default_stub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = a.opt_str(0);
    let w = a.opt_str(1);
    match goro_ext_phar::stub::default_stub(i.as_deref(), w.as_deref()) {
        Ok(s) => Ok(Value::Str(PhpStr::from(s))),
        Err(m) => vm.throw("PharException", &m),
    }
}

fn get_supported_compression(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values([
        Value::from("GZ"),
        Value::from("BZIP2"),
    ])))
}

fn get_supported_signatures(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        [
            "MD5",
            "SHA-1",
            "SHA-256",
            "SHA-512",
            "OpenSSL",
            "OpenSSL_SHA256",
            "OpenSSL_SHA512",
        ]
        .map(Value::from),
    )))
}

fn intercept_file_funcs(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    state(vm).intercept = true;
    Ok(Value::Null)
}

fn is_valid_phar_filename(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ni = name_info(a.bytes(0));
    Ok(Value::Bool(if a.bool_or(1, true) {
        ni.exec
    } else {
        ni.has_ext && !ni.exec
    }))
}

fn register_alias(vm: &mut Vm, s: &Shared, key: &[u8], alias: Option<Vec<u8>>) -> R<()> {
    let Some(al) = alias else {
        return Ok(());
    };
    if let Some(other) = state(vm).aliases.get(&al).cloned()
        && other != key
    {
        return vm.throw(
            "PharException",
            &format!(
                "alias \"{}\" is already used for archive \"{}\" cannot be overloaded with \"{}\"",
                lossy(&al),
                lossy(&other),
                lossy(key)
            ),
        );
    }
    state(vm).aliases.insert(al.clone(), key.to_vec());
    let mut l = s.borrow_mut();
    if l.a.alias.is_none() {
        l.a.alias = Some(al);
    }
    Ok(())
}

fn load_phar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = super::abs_path(vm, a.bytes(0));
    let s = match load(vm, &key, false, false) {
        Ok(s) => s,
        Err(m) => return vm.throw("PharException", &m),
    };
    register_alias(vm, &s, &key, a.opt_str(1).map(|x| x.to_vec()))?;
    Ok(Value::Bool(true))
}

fn map_phar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let file = vm.cur_file().to_vec();
    let key = super::abs_path(vm, &file);
    let alias = a.opt_str(0).map(|x| x.to_vec());
    if let Some(s) = state(vm).archives.get(&key).cloned() {
        register_alias(vm, &s, &key, alias)?;
        return Ok(Value::Bool(true));
    }
    let data = std::fs::read(super::os_path(&key)).unwrap_or_default();
    let opts = goro_ext_phar::ParseOptions {
        require_hash: super::ini_bool(vm, b"phar.require_hash"),
        verify: None,
        is_data: false,
        hint: None,
    };
    // a tar or zip runs the stub inside it
    let tarzip = goro_ext_phar::zip::looks_like_zip(&data)
        || goro_ext_phar::tar::looks_like_tar(&data)
        || data.starts_with(b"BZh")
        || data.starts_with(b"\x1f\x8b");
    let parsed = if tarzip {
        goro_ext_phar::Archive::parse(&key, &data, &opts)
    } else {
        match goro_ext_phar::phar::manifest_start(&data) {
            Ok(start) => goro_ext_phar::phar::parse_at(&key, &data, start, &opts),
            Err(m) => Err(m),
        }
    };
    let arc = match parsed {
        Ok(x) => x,
        Err(m) => return vm.throw("PharException", &m),
    };
    let stored = arc.alias.clone();
    let loaded = super::Loaded {
        a: arc,
        is_data: false,
        buffering: false,
        modified: false,
        on_disk: true,
        private_key: None,
        mounts: Vec::new(),
        open_writes: 0,
    };
    let s = std::rc::Rc::new(std::cell::RefCell::new(loaded));
    let al = alias.or(stored);
    if let Some(al) = &al
        && let Some(other) = state(vm).aliases.get(al).cloned()
        && other != key
    {
        return vm.throw(
            "PharException",
            &format!(
                "Cannot open archive \"{}\", alias is already in use by existing archive",
                lossy(&key)
            ),
        );
    }
    state(vm).archives.insert(key.clone(), s.clone());
    register_alias(vm, &s, &key, al)?;
    Ok(Value::Bool(true))
}

fn running(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((key, _)) = super::running(vm) else {
        return Ok(Value::from(""));
    };
    if a.bool_or(0, true) {
        let mut u = b"phar://".to_vec();
        u.extend_from_slice(&key);
        Ok(Value::Str(PhpStr::from(u)))
    } else {
        Ok(Value::Str(PhpStr::from(key)))
    }
}

fn mount(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pp = a.str(0).to_vec();
    let ext = a.str(1).to_vec();
    let (key, inner) = if pp.starts_with(b"phar://") {
        match split_url(vm, &pp) {
            Ok(x) => x,
            Err(_) => {
                return vm.throw(
                    "PharException",
                    &format!("Mounting of {} to {} failed", lossy(&pp), lossy(&ext)),
                );
            }
        }
    } else {
        match super::running(vm) {
            Some((k, _)) => (k, super::norm_inner(&pp)),
            None => {
                return vm.throw(
                    "PharException",
                    &format!("Mounting of {} to {} failed", lossy(&pp), lossy(&ext)),
                );
            }
        }
    };
    let s = match load(vm, &key, false, !name_info(&key).exec) {
        Ok(s) => s,
        Err(_) => {
            return vm.throw(
                "PharException",
                &format!("Mounting of {} to {} failed", lossy(&pp), lossy(&ext)),
            );
        }
    };
    let ext_abs = super::abs_path(vm, &ext);
    if !super::os_path(&ext_abs).exists() || s.borrow().a.get(&inner).is_some() {
        return vm.throw(
            "PharException",
            &format!("Mounting of {} to {} failed", lossy(&pp), lossy(&ext)),
        );
    }
    s.borrow_mut().mounts.push((inner, ext_abs));
    Ok(Value::Null)
}

/// Phar::mungServer(): which $_SERVER entries a web phar rewrites; the
/// list is checked (tests frontcontroller18-20), nothing else happens on
/// the CLI.
fn mung_server(vm: &mut Vm, a: &mut Args) -> R<Value> {
    const WHICH: &str = "expecting an array of any of these strings: PHP_SELF, REQUEST_URI, SCRIPT_FILENAME, SCRIPT_NAME";
    let arr = a.array(0);
    if arr.is_empty() {
        return vm.throw(
            "PharException",
            &format!("No values passed to Phar::mungServer(), {WHICH}"),
        );
    }
    if arr.len() > 4 {
        return vm.throw(
            "PharException",
            &format!("Too many values passed to Phar::mungServer(), {WHICH}"),
        );
    }
    for (_, v) in arr.iter() {
        if !matches!(v.deref(), Value::Str(_)) {
            return vm.throw(
                "PharException",
                &format!("Non-string value passed to Phar::mungServer(), {WHICH}"),
            );
        }
    }
    Ok(Value::Null)
}

fn web_phar(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // the CLI is not a web request (PHP manual: Phar::webPhar)
    Ok(Value::Null)
}

fn unlink_archive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.bytes(0).to_vec();
    let key = if f.starts_with(b"phar://") {
        match split_url(vm, &f) {
            Ok((k, _)) => k,
            Err(_) => {
                return vm.throw(
                    "PharException",
                    &format!("Unknown phar archive \"{}\"", lossy(&f)),
                );
            }
        }
    } else {
        super::abs_path(vm, &f)
    };
    if let Some((k, _)) = super::running(vm)
        && k == key
    {
        return vm.throw(
            "PharException",
            &format!(
                "phar archive \"{}\" cannot be unlinked from within itself",
                lossy(&key)
            ),
        );
    }
    if !state(vm).archives.contains_key(&key)
        && load(vm, &key, false, !name_info(&key).exec).is_err()
    {
        return vm.throw(
            "PharException",
            &format!("Unknown phar archive \"{}\"", lossy(&f)),
        );
    }
    forget(vm, &key);
    let _ = std::fs::remove_file(super::os_path(&key));
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- PharFileInfo

/// Runs `f` on the entry of a PharFileInfo; none for a directory implied
/// by others' paths.
fn with_entry<T>(s: &Shared, inner: &[u8], f: impl FnOnce(&mut Entry, &[u8]) -> T) -> Option<T> {
    let mut l = s.borrow_mut();
    let fname = l.a.fname.clone();
    l.a.get_mut(inner).map(|e| f(e, &fname))
}

fn info_chmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    if !s.borrow().is_data && readonly(vm) {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Cannot modify permissions for file \"{}\" in phar \"{}\", write operations are prohibited",
                lossy(&p.inner),
                lossy(&p.key)
            ),
        );
    }
    let m = a.int(0) as u32;
    if with_entry(&s, &p.inner, |e, _| {
        e.flags = (e.flags & !0o777) | (m & 0o777)
    })
    .is_none()
    {
        return vm.throw(
            "BadMethodCallException",
            &format!(
                "Phar entry \"{}\" is a temporary directory (not an actual entry in the archive), cannot chmod",
                lossy(&p.inner)
            ),
        );
    }
    save(vm, &s)?;
    Ok(Value::Null)
}

fn info_compress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    let is_dir = with_entry(&s, &p.inner, |e, _| e.is_dir).unwrap_or(true);
    if is_dir {
        return vm.throw(
            "BadMethodCallException",
            "Phar entry is a directory, cannot set compression",
        );
    }
    if !s.borrow().is_data && readonly(vm) {
        return vm.throw(
            "BadMethodCallException",
            "Phar is readonly, cannot change compression",
        );
    }
    let c = match a.int(0) {
        4096 => Comp::Gz,
        8192 => Comp::Bz2,
        _ => {
            return vm.throw(
                "BadMethodCallException",
                "Unknown compression type specified",
            );
        }
    };
    if s.borrow().a.format == Format::Tar {
        return vm.throw(
            "BadMethodCallException",
            "Cannot compress with Gzip compression, not possible with tar-based phar archives",
        );
    }
    if let Some(Err(m)) = with_entry(&s, &p.inner, |e, f| e.set_comp(c, f)) {
        return vm.throw("BadMethodCallException", &m);
    }
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn info_decompress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    let is_dir = with_entry(&s, &p.inner, |e, _| e.is_dir).unwrap_or(true);
    if is_dir {
        return vm.throw(
            "BadMethodCallException",
            "Phar entry is a directory, cannot set compression",
        );
    }
    let comp = with_entry(&s, &p.inner, |e, _| e.comp).unwrap_or(Comp::None);
    if comp == Comp::None {
        return Ok(Value::Bool(true));
    }
    if !s.borrow().is_data && readonly(vm) {
        return vm.throw(
            "BadMethodCallException",
            "Phar is readonly, cannot decompress",
        );
    }
    if let Some(Err(m)) = with_entry(&s, &p.inner, |e, f| e.set_comp(Comp::None, f)) {
        return vm.throw("BadMethodCallException", &m);
    }
    save(vm, &s)?;
    Ok(Value::Bool(true))
}

fn info_get_crc32(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    match with_entry(&s, &p.inner, |e, _| (e.is_dir, e.checked, e.crc32)) {
        Some((false, true, c)) => Ok(Value::Int(c as i64)),
        Some((false, false, _)) => {
            vm.throw("BadMethodCallException", "Phar entry was not CRC checked")
        }
        _ => vm.throw(
            "BadMethodCallException",
            "Phar entry is a directory, does not have a CRC",
        ),
    }
}

fn info_get_compressed_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    Ok(Value::Int(
        with_entry(&s, &p.inner, |e, _| e.raw.len() as i64).unwrap_or(0),
    ))
}

fn info_get_content(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    let r = with_entry(&s, &p.inner, |e, f| {
        if e.is_dir {
            return Err(String::new());
        }
        e.contents(f).map(|d| d.to_vec())
    });
    match r {
        Some(Ok(d)) => Ok(Value::Str(PhpStr::from(d))),
        Some(Err(m)) if !m.is_empty() => vm.throw("BadMethodCallException", &m),
        _ => vm.throw(
            "BadMethodCallException",
            &format!(
                "phar error: Cannot retrieve contents, \"{}\" in phar \"{}\" is a directory",
                lossy(&p.inner),
                lossy(&p.key)
            ),
        ),
    }
}

fn info_get_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    let m = with_entry(&s, &p.inner, |e, _| e.metadata.clone()).flatten();
    let opts = if a.has(0) {
        a.array(0)
    } else {
        PhpArray::new()
    };
    get_metadata_of(vm, m, &opts)
}

fn info_has_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    Ok(Value::Bool(
        with_entry(&s, &p.inner, |e, _| e.metadata.is_some()).unwrap_or(false),
    ))
}

fn info_set_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    if s.borrow().a.get(&p.inner).is_none() {
        return vm.throw(
            "BadMethodCallException",
            "Phar entry is a temporary directory (not an actual entry in the archive), cannot set metadata",
        );
    }
    check_write(vm, &s, "BadMethodCallException", RO)?;
    let m = vm.serialize_value(&a.val(0))?;
    with_entry(&s, &p.inner, |e, _| e.metadata = Some(m));
    save(vm, &s)?;
    Ok(Value::Null)
}

fn info_del_metadata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    if s.borrow().a.get(&p.inner).is_none() {
        return vm.throw(
            "BadMethodCallException",
            "Phar entry is a temporary directory (not an actual entry in the archive), cannot delete metadata",
        );
    }
    check_write(vm, &s, "BadMethodCallException", RO)?;
    let had = with_entry(&s, &p.inner, |e, _| e.metadata.take().is_some()).unwrap_or(false);
    if had {
        save(vm, &s)?;
    }
    Ok(Value::Bool(true))
}

fn info_get_phar_flags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    Ok(Value::Int(
        with_entry(&s, &p.inner, |e, _| comp_flag(e.comp)).unwrap_or(0),
    ))
}

fn info_is_crc_checked(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    Ok(Value::Bool(
        with_entry(&s, &p.inner, |e, _| e.checked).unwrap_or(false),
    ))
}

fn info_is_compressed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (s, p) = this_info(vm, a)?;
    let c = with_entry(&s, &p.inner, |e, _| e.comp).unwrap_or(Comp::None);
    Ok(Value::Bool(match a.opt_int(0) {
        None => c != Comp::None,
        Some(4096) => c == Comp::Gz,
        Some(8192) => c == Comp::Bz2,
        Some(_) => {
            return vm.throw(
                "BadMethodCallException",
                "Unknown compression type specified",
            );
        }
    }))
}
