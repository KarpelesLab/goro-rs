//! ext/phar (PHP manual: Phar) on the goro-ext-phar crate: the archives
//! opened in a request (by file name and alias), the `phar://` stream
//! wrapper with its path operations and includes, and the Phar, PharData,
//! PharFileInfo and PharException classes (classes.rs). Behaviour from the
//! manual, tests ext/phar/tests and observations on PHP 8.5.

mod classes;
mod wrapper;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_ext_phar::{Archive, Comp, Format, ParseOptions, SigType};

use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "Phar",
    classes: &[
        (
            "pharexception",
            "<?php class PharException extends Exception {}",
        ),
        ("phar", classes::PHAR_CLASS),
        ("phardata", classes::PHARDATA_CLASS),
        ("pharfileinfo", classes::PHARFILEINFO_CLASS),
    ],
    methods: classes::METHODS,
    hooks: &[
        ("phar", classes::hooks),
        ("phardata", classes::hooks),
        ("pharfileinfo", classes::hooks),
    ],
    // PHP manual: Phar runtime configuration
    ini: &[
        ("phar.readonly", "1"),
        ("phar.require_hash", "1"),
        ("phar.cache_list", ""),
    ],
    init: Some(init),
    on_ini_set: Some(on_ini_set),
    ..Module::EMPTY
};

fn init(vm: &mut Vm) {
    super::streams::register_wrapper(vm, "phar", &wrapper::WRAPPER);
}

/// phar.readonly and phar.require_hash can be turned on at run time but
/// not off (PHP manual: "This setting can only be unset in php.ini due to
/// security reasons"; test ini_set.phpt).
fn on_ini_set(vm: &mut Vm, name: &[u8], value: &[u8]) -> R<bool> {
    if name == b"phar.readonly" || name == b"phar.require_hash" {
        let cur = ini_bool(vm, name);
        if cur && !parse_bool(value) && !vm.ext::<PharState>().startup {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_bool(v: &[u8]) -> bool {
    let l = v.to_ascii_lowercase();
    matches!(&l[..], b"1" | b"on" | b"yes" | b"true")
        || (!l.is_empty() && l.iter().all(|c| c.is_ascii_digit()) && l != b"0")
}

pub(crate) fn ini_bool(vm: &Vm, name: &[u8]) -> bool {
    vm.get_ini(name).is_some_and(|v| parse_bool(&v))
}

/// phar.readonly.
pub(crate) fn readonly(vm: &Vm) -> bool {
    ini_bool(vm, b"phar.readonly")
}

// ---------------------------------------------------------------- state

/// An archive opened in this request.
pub struct Loaded {
    pub a: Archive,
    /// Opened as PharData (writable whatever phar.readonly says).
    pub is_data: bool,
    /// Between startBuffering() and stopBuffering().
    pub buffering: bool,
    /// Changed since read.
    pub modified: bool,
    /// The file exists on disk.
    pub on_disk: bool,
    /// The private key of an OpenSSL signature (setSignatureAlgorithm).
    pub private_key: Option<Vec<u8>>,
    /// Phar::mount(): (path inside, external path).
    pub mounts: Vec<(Vec<u8>, Vec<u8>)>,
    /// Write streams open on entries.
    pub open_writes: usize,
}

pub type Shared = Rc<RefCell<Loaded>>;

#[derive(Default)]
pub struct PharState {
    /// By absolute file name.
    pub archives: HashMap<Vec<u8>, Shared>,
    /// Alias → absolute file name.
    pub aliases: HashMap<Vec<u8>, Vec<u8>>,
    /// Phar::interceptFileFuncs() was called.
    pub intercept: bool,
    /// Startup settings are being applied.
    pub startup: bool,
}

pub(crate) fn state(vm: &mut Vm) -> &mut PharState {
    vm.ext::<PharState>()
}

pub(crate) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

/// An absolute path with `.`, `..` and repeated slashes resolved
/// (without following links).
pub(crate) fn abs_path(vm: &Vm, p: &[u8]) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    let full = if p.starts_with(b"/") {
        p.to_vec()
    } else {
        let mut c = vm.cwd.as_os_str().as_bytes().to_vec();
        c.push(b'/');
        c.extend_from_slice(p);
        c
    };
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in full.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = Vec::new();
    for s in parts {
        out.push(b'/');
        out.extend_from_slice(s);
    }
    if out.is_empty() {
        out.push(b'/');
    }
    out
}

/// A path inside an archive: no leading slash, `.` and `..` resolved.
pub(crate) fn norm_inner(p: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in p.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join(&b'/')
}

/// What a file name's extension says (test phar_isvalidpharfilename.phpt):
/// the extension starts at the first dot after the first character of the
/// base name; an executable phar has a "phar" component.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NameInfo {
    pub exec: bool,
    pub has_ext: bool,
    pub format: Format,
    pub comp: Comp,
}

pub(crate) fn name_info(fname: &[u8]) -> NameInfo {
    let base = match fname.iter().rposition(|&c| c == b'/') {
        Some(p) => &fname[p + 1..],
        None => fname,
    };
    let ext = base
        .iter()
        .skip(1)
        .position(|&c| c == b'.')
        .map(|p| &base[p + 2..])
        .unwrap_or(b"");
    let has_ext = base.len() > 1 && base[1..].contains(&b'.');
    let comps: Vec<Vec<u8>> = ext
        .split(|&c| c == b'.')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let has = |n: &[u8]| comps.iter().any(|c| c == n);
    let exec = has(b"phar");
    let format = if has(b"zip") {
        Format::Zip
    } else if has(b"tar") || has(b"tgz") || has(b"tbz") {
        Format::Tar
    } else {
        Format::Phar
    };
    let last = comps.last().cloned().unwrap_or_default();
    let comp = match &last[..] {
        b"gz" | b"tgz" => Comp::Gz,
        b"bz2" | b"tbz" => Comp::Bz2,
        _ => Comp::None,
    };
    NameInfo {
        exec,
        has_ext,
        format,
        comp,
    }
}

/// Splits a `phar://` URL into the archive's absolute file name and the
/// path inside it.
pub(crate) fn split_url(vm: &mut Vm, url: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let invalid = || {
        format!(
            "phar error: invalid url or non-existent phar \"{}\"",
            lossy(url)
        )
    };
    let Some(rest) = url
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case(b"phar://"))
        .map(|_| &url[7..])
    else {
        return Err(invalid());
    };
    // an alias first
    let first_end = rest.iter().position(|&c| c == b'/').unwrap_or(rest.len());
    if first_end > 0
        && let Some(f) = state(vm).aliases.get(&rest[..first_end]).cloned()
    {
        return Ok((f, norm_inner(&rest[first_end..])));
    }
    let mut cuts: Vec<usize> = rest
        .iter()
        .enumerate()
        .filter(|&(i, &c)| c == b'/' && i > 0)
        .map(|(i, _)| i)
        .collect();
    cuts.push(rest.len());
    for &c in &cuts {
        let cand = &rest[..c];
        let abs = abs_path(vm, cand);
        if state(vm).archives.contains_key(&abs) {
            return Ok((abs, norm_inner(&rest[c..])));
        }
        if name_info(cand).has_ext
            && std::path::Path::new(&*String::from_utf8_lossy(&abs)).is_file()
        {
            return Ok((abs, norm_inner(&rest[c..])));
        }
    }
    for &c in &cuts {
        let cand = &rest[..c];
        let ni = name_info(cand);
        if ni.exec || (ni.has_ext && ni.format != Format::Phar) {
            let abs = abs_path(vm, cand);
            return Ok((abs, norm_inner(&rest[c..])));
        }
    }
    Err(invalid())
}

fn os_path(p: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(p))
}

/// Checks OpenSSL signatures with the key in `<archive>.pubkey` (PHP
/// manual: Phar::setSignatureAlgorithm).
fn verifier(fname: &[u8]) -> Box<goro_ext_phar::sig::Verifier> {
    let mut kf = fname.to_vec();
    kf.extend_from_slice(b".pubkey");
    let fname = fname.to_vec();
    Box::new(move |t, data, sig| {
        let Ok(pem) = std::fs::read(os_path(&kf)) else {
            return Err(format!(
                "phar \"{}\" openssl signature could not be verified: openssl public key could not be read",
                lossy(&fname)
            ));
        };
        let Ok(key) = goro_ext_openssl::key::load_public_pem(&pem) else {
            return Ok(false);
        };
        let md = goro_ext_openssl::digest::Md::by_name(openssl_md(t));
        Ok(key.verify(md, data, sig, false).unwrap_or(false))
    })
}

fn openssl_md(t: SigType) -> &'static str {
    match t {
        SigType::OpenSslSha256 => "sha256",
        SigType::OpenSslSha512 => "sha512",
        _ => "sha1",
    }
}

/// Opens an archive (from the request's list, or from disk); `create`
/// makes a new empty one when the file doesn't exist.
pub(crate) fn load(vm: &mut Vm, abs: &[u8], create: bool, is_data: bool) -> Result<Shared, String> {
    if let Some(s) = state(vm).archives.get(abs) {
        return Ok(s.clone());
    }
    let path = os_path(abs);
    let ni = name_info(abs);
    let loaded = match std::fs::read(&path) {
        Ok(data) if path.is_file() => {
            let opts = ParseOptions {
                require_hash: !is_data && ini_bool(vm, b"phar.require_hash"),
                verify: Some(verifier(abs)),
                is_data,
                hint: match ni.format {
                    Format::Phar => None,
                    f => Some(f),
                },
            };
            let a = Archive::parse(abs, &data, &opts)?;
            Loaded {
                a,
                is_data,
                buffering: false,
                modified: false,
                on_disk: true,
                private_key: None,
                mounts: Vec::new(),
                open_writes: 0,
            }
        }
        _ => {
            if !create {
                return Err(format!(
                    "phar error: invalid url or non-existent phar \"phar://{}\"",
                    lossy(abs)
                ));
            }
            let mut a = Archive::new(abs, ni.format, is_data);
            if ni.format != Format::Zip {
                a.comp = ni.comp;
            }
            if !is_data && ni.format == Format::Phar {
                a.stub = goro_ext_phar::stub::default_stub(None, None).unwrap_or_default();
            }
            Loaded {
                a,
                is_data,
                buffering: false,
                modified: false,
                on_disk: false,
                private_key: None,
                mounts: Vec::new(),
                open_writes: 0,
            }
        }
    };
    if let Some(al) = loaded.a.alias.clone() {
        if let Some(other) = state(vm).aliases.get(&al)
            && other != abs
        {
            return Err(format!(
                "Cannot open archive \"{}\", alias is already in use by existing archive",
                lossy(abs)
            ));
        }
        state(vm).aliases.insert(al, abs.to_vec());
    }
    let s = Rc::new(RefCell::new(loaded));
    state(vm).archives.insert(abs.to_vec(), s.clone());
    Ok(s)
}

/// Forgets an archive (Phar::unlinkArchive, a failed creation).
pub(crate) fn forget(vm: &mut Vm, abs: &[u8]) {
    let st = state(vm);
    st.archives.remove(abs);
    st.aliases.retain(|_, v| v != abs);
}

/// Writes an archive to its file (unless buffering).
pub(crate) fn flush(vm: &mut Vm, s: &Shared) -> Result<(), String> {
    {
        let mut l = s.borrow_mut();
        l.modified = true;
        if l.buffering {
            return Ok(());
        }
    }
    write_out(vm, s)
}

/// Writes an archive to its file now.
pub(crate) fn write_out(vm: &mut Vm, s: &Shared) -> Result<(), String> {
    let mut l = s.borrow_mut();
    if !l.is_data && readonly(vm) {
        return Err("write operations disabled by the php.ini setting phar.readonly".into());
    }
    let key = l.private_key.clone();
    let fname = l.a.fname.clone();
    let signer = move |t: SigType, data: &[u8]| -> Result<Vec<u8>, String> {
        let fail = || {
            format!(
                "unable to write phar \"{}\" with requested openssl signature",
                lossy(&fname)
            )
        };
        let Some(k) = &key else {
            return Err(fail());
        };
        let Ok(key) = goro_ext_openssl::key::load_private_pem(k, None) else {
            return Err(fail());
        };
        let md = goro_ext_openssl::digest::Md::by_name(openssl_md(t));
        key.sign(md, data, false).map_err(|_| fail())
    };
    let bytes = l.a.serialize(now(), &signer)?;
    let path = os_path(&l.a.fname);
    if std::fs::write(&path, &bytes).is_err() {
        return Err(format!(
            "unable to open new phar \"{}\" for writing",
            lossy(&l.a.fname)
        ));
    }
    l.on_disk = true;
    Ok(())
}

/// The archive and entry of the file running now, when it is inside a
/// phar (Phar::running()).
pub(crate) fn running(vm: &mut Vm) -> Option<(Vec<u8>, Vec<u8>)> {
    let f = vm.cur_file().to_vec();
    if !f.starts_with(b"phar://") {
        return None;
    }
    split_url(vm, &f).ok()
}
