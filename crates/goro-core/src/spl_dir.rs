//! SplFileInfo and the DirectoryIterator family (DirectoryIterator,
//! FilesystemIterator, RecursiveDirectoryIterator, GlobIterator), after
//! php-src ext/spl/spl_directory.c.
//!
//! An spl_filesystem_object is INFO, DIR or FILE (SplFileObject, see
//! spl_file.rs). Hidden state (`__spl_*` properties):
//!   __spl_fs_type     2 for DIR objects (absent: INFO, or FILE for SplFileObject)
//!   __spl_file_path   file_name (absent/Null: not known yet)
//!   __spl_path        path
//!   __spl_flags       FilesystemIterator flags (DIR)
//!   __spl_dir_open    True while the directory stream is open (dirp != NULL)
//!   __spl_dir_names   entries of the directory stream (glob: full paths)
//!   __spl_dir_types   d_type of each entry (DT_*)
//!   __spl_dir_cursor  read position of the directory stream
//!   __spl_dir_glob    True for glob:// streams
//!   __spl_glob_path   php_glob_stream_get_path() (absent: NULL)
//!   __spl_dir_entry   current d_name ("" when exhausted)
//!   __spl_dir_dtype   d_type of the current entry
//!   __spl_dir_index   u.dir.index
//!   __spl_sub_path    RecursiveDirectoryIterator sub path
//!   __spl_info_class / __spl_file_class   classes for getFileInfo()/openFile()

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{mangle_private, obj_class_lower, val_arr};
use crate::spl_file::php_basename;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

const CURRENT_AS_SELF: i64 = 0x10;
const CURRENT_AS_PATHNAME: i64 = 0x20;
const CURRENT_MODE_MASK: i64 = 0xF0;
const KEY_AS_FILENAME: i64 = 0x100;
const KEY_MODE_MASK: i64 = 0xF00;
const SKIPDOTS: i64 = 0x1000;
const UNIXPATHS: i64 = 0x2000;
const FOLLOW_SYMLINKS: i64 = 0x4000;
const OTHERS_MASK: i64 = 0x7000;

const DIT_CTOR_FLAGS: i64 = 1;
const DIT_CTOR_GLOB: i64 = 2;

const DT_UNKNOWN: i64 = 0;
const DT_DIR: i64 = 4;
const DT_REG: i64 = 8;
const DT_LNK: i64 = 10;

pub const DIR_CLASSES: &[&[u8]] = &[
    b"globiterator",
    b"recursivedirectoryiterator",
    b"filesystemiterator",
    b"directoryiterator",
];

fn prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Value {
    obj.borrow().get_property(name)
}

fn set_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], v: Value) {
    obj.borrow_mut().set_property_slice(name, v);
}

fn sv(s: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(s))
}

fn svec(s: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(s))
}

fn bool_v(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn lprop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> i64 {
    match prop(obj, name) {
        Value::Long(n) => n,
        _ => 0,
    }
}

fn sprop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Option<Vec<u8>> {
    match prop(obj, name) {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn is_dot(name: &[u8]) -> bool {
    name == b"." || name == b".."
}

/// A URL served by a stream wrapper other than the plain files one (the
/// directory and stat operations then go through the stream layer).
fn is_wrapper_url(p: &[u8]) -> bool {
    let n = p
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        .count();
    n > 1 && p[n..].starts_with(b"://") && !p[..n].eq_ignore_ascii_case(b"file")
}

/// The local path of a `file://` URL (the plain-files wrapper's path_for_open);
/// other paths unchanged.
#[inline(never)]
fn file_url_path(p: &[u8]) -> &[u8] {
    if p.len() < 7 || !p[..7].eq_ignore_ascii_case(b"file://") {
        return p;
    }
    let mut rest = &p[7..];
    if rest.len() >= 10 && rest[..10].eq_ignore_ascii_case(b"localhost/") {
        rest = &rest[9..];
    }
    if !rest.starts_with(b"/") {
        return p;
    }
    while rest.len() > 1 && rest[1] == b'/' {
        rest = &rest[1..];
    }
    rest
}

/// The fields of a stat buffer the FileInfoFunction methods use.
struct FsStat {
    mode: i64,
    ino: i64,
    size: i64,
    uid: i64,
    gid: i64,
    atime: i64,
    mtime: i64,
    ctime: i64,
}

const S_IFMT: i64 = 0o170000;

/// strerror() text of an io::Error.
fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// php_glob_stream_path_split(): (path, file) of a glob result.
fn glob_path_split(path: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let file_start = match path.iter().rposition(|&c| c == b'/') {
        Some(i) => i + 1,
        None => 0,
    };
    let mut plen = file_start;
    if plen > 1 {
        plen -= 1;
    }
    (path[..plen].to_vec(), path[file_start..].to_vec())
}

fn dirname(p: &[u8]) -> Vec<u8> {
    // zend_dirname()
    let mut end = p.len();
    while end > 1 && p[end - 1] == b'/' {
        end -= 1;
    }
    match p[..end].iter().rposition(|&c| c == b'/') {
        None => b".".to_vec(),
        Some(i) => {
            let mut j = i;
            while j > 0 && p[j - 1] == b'/' {
                j -= 1;
            }
            if j == 0 {
                b"/".to_vec()
            } else {
                p[..j].to_vec()
            }
        }
    }
}

impl Vm {
    fn fs_is_dir(obj: &Rc<RefCell<PhpObject>>) -> bool {
        lprop(obj, b"__spl_fs_type") == 2
    }

    fn fs_dir_open(obj: &Rc<RefCell<PhpObject>>) -> bool {
        matches!(prop(obj, b"__spl_dir_open"), Value::True)
    }

    fn fs_entry(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
        sprop(obj, b"__spl_dir_entry").unwrap_or_default()
    }

    fn fs_flags(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        lprop(obj, b"__spl_flags")
    }

    fn fs_is_glob(obj: &Rc<RefCell<PhpObject>>) -> bool {
        Self::fs_dir_open(obj) && matches!(prop(obj, b"__spl_dir_glob"), Value::True)
    }

    /// spl_filesystem_object_get_path()
    pub(crate) fn fs_get_path(obj: &Rc<RefCell<PhpObject>>) -> Option<Vec<u8>> {
        if Self::fs_is_dir(obj) && Self::fs_is_glob(obj) {
            return sprop(obj, b"__spl_glob_path").filter(|p| !p.is_empty());
        }
        if let Some(p) = sprop(obj, b"__spl_path") {
            return Some(p);
        }
        sprop(obj, b"__spl_file_path").map(|f| crate::spl_file::split_file_name(&f).1)
    }

    /// spl_filesystem_object_get_file_name()
    pub(crate) fn fs_get_file_name(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<Vec<u8>> {
        if let Some(f) = sprop(obj, b"__spl_file_path") {
            return Some(f);
        }
        if !Self::fs_is_dir(obj) {
            self.spl_throw(b"Error", "Object not initialized");
            return None;
        }
        let entry = Self::fs_entry(obj);
        let name = match Self::fs_get_path(obj) {
            None => entry,
            Some(mut p) => {
                p.push(b'/');
                p.extend_from_slice(&entry);
                p
            }
        };
        set_prop(obj, b"__spl_file_path", svec(name.clone()));
        Some(name)
    }

    /// spl_filesystem_object_get_pathname()
    fn fs_get_pathname(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<Vec<u8>> {
        if Self::fs_is_dir(obj) {
            if Self::fs_entry(obj).is_empty() {
                return None;
            }
            return self.fs_get_file_name(obj);
        }
        sprop(obj, b"__spl_file_path")
    }

    /// file_name minus the "path/" prefix.
    fn fs_short_name(obj: &Rc<RefCell<PhpObject>>, fname: &[u8]) -> Vec<u8> {
        match Self::fs_get_path(obj) {
            Some(p) if !p.is_empty() && p.len() < fname.len() => fname[p.len() + 1..].to_vec(),
            _ => fname.to_vec(),
        }
    }

    /// spl_filesystem_dir_read()
    fn fs_dir_read(obj: &Rc<RefCell<PhpObject>>) {
        set_prop(obj, b"__spl_file_path", Value::Null);
        let mut entry: Vec<u8> = Vec::new();
        let mut dtype = DT_UNKNOWN;
        if Self::fs_dir_open(obj) {
            let cursor = lprop(obj, b"__spl_dir_cursor");
            let names = match prop(obj, b"__spl_dir_names") {
                Value::Array(a) => a,
                _ => Rc::new(RefCell::new(PhpArray::new())),
            };
            let glob = matches!(prop(obj, b"__spl_dir_glob"), Value::True);
            let n = names.borrow().len() as i64;
            if cursor < n {
                let name = names
                    .borrow()
                    .get(&ArrayKey::Int(cursor))
                    .map(|v| v.to_php_string().as_bytes().to_vec())
                    .unwrap_or_default();
                if glob {
                    let (p, f) = glob_path_split(&name);
                    set_prop(obj, b"__spl_glob_path", svec(p));
                    entry = f;
                } else {
                    entry = name;
                    dtype = match prop(obj, b"__spl_dir_types") {
                        Value::Array(t) => match t.borrow().get(&ArrayKey::Int(cursor)) {
                            Some(Value::Long(d)) => *d,
                            _ => DT_UNKNOWN,
                        },
                        _ => DT_UNKNOWN,
                    };
                }
                set_prop(obj, b"__spl_dir_cursor", Value::Long(cursor + 1));
            } else if glob {
                set_prop(obj, b"__spl_glob_path", Value::Null);
            }
        }
        set_prop(obj, b"__spl_dir_entry", svec(entry));
        set_prop(obj, b"__spl_dir_dtype", Value::Long(dtype));
    }

    fn fs_dir_read_skip(obj: &Rc<RefCell<PhpObject>>, skip_dots: bool) {
        loop {
            Self::fs_dir_read(obj);
            if !(skip_dots && is_dot(&Self::fs_entry(obj))) {
                break;
            }
        }
    }

    /// php_stream_rewinddir()
    fn fs_dir_rewind(obj: &Rc<RefCell<PhpObject>>) {
        if Self::fs_dir_open(obj) {
            set_prop(obj, b"__spl_dir_cursor", Value::Long(0));
            if matches!(prop(obj, b"__spl_dir_glob"), Value::True) {
                set_prop(obj, b"__spl_glob_path", Value::Null);
            }
        }
    }

    /// php_stream_opendir() + readdir of a stream wrapper URL: the entries,
    /// or the reason the directory could not be opened.
    fn fs_wrapper_listdir(&mut self, path: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let saved = self.warning_capture.replace(Vec::new());
        let r = self.spl_call_builtin(b"opendir", &[svec(path.to_vec())]);
        let warnings = std::mem::replace(&mut self.warning_capture, saved).unwrap_or_default();
        let Some(Value::Resource(id)) = r else {
            const SEP: &str = ": Failed to open directory: ";
            return Err(warnings
                .iter()
                .rev()
                .find_map(|w| w.find(SEP).map(|i| w[i + SEP.len()..].to_string()))
                .unwrap_or_else(|| "operation failed".to_string()));
        };
        let mut names = Vec::new();
        while let Some(Value::String(n)) =
            self.spl_call_builtin(b"readdir", &[Value::resource(&id)])
        {
            names.push(n.as_bytes().to_vec());
        }
        self.spl_call_builtin(b"closedir", &[Value::resource(&id)]);
        Ok(names)
    }

    /// php_stream_stat_path() of a stream wrapper URL (quiet).
    fn fs_wrapper_stat(&mut self, path: &[u8], link: bool) -> Option<FsStat> {
        let f: &[u8] = if link { b"lstat" } else { b"stat" };
        let saved = self.warning_capture.replace(Vec::new());
        let r = self.spl_call_builtin(f, &[svec(path.to_vec())]);
        self.warning_capture = saved;
        let Some(Value::Array(a)) = r else {
            return None;
        };
        let a = a.borrow();
        let g = |k: &[u8]| {
            a.get(&ArrayKey::String(PhpString::from_bytes(k)))
                .map(|v| v.to_long())
                .unwrap_or(0)
        };
        Some(FsStat {
            mode: g(b"mode"),
            ino: g(b"ino"),
            size: g(b"size"),
            uid: g(b"uid"),
            gid: g(b"gid"),
            atime: g(b"atime"),
            mtime: g(b"mtime"),
            ctime: g(b"ctime"),
        })
    }

    /// spl_filesystem_dir_open()
    fn fs_dir_open_path(&mut self, obj: &Rc<RefCell<PhpObject>>, path: &[u8], func: &str) {
        let skip_dots = Self::fs_flags(obj) & SKIPDOTS != 0;
        set_prop(obj, b"__spl_fs_type", Value::Long(2));
        let mut opened = false;
        let mut error: Option<String> = None;
        if let Some(pattern) = path.strip_prefix(b"glob://") {
            let r = self.spl_call_builtin(b"glob", &[svec(pattern.to_vec())]);
            let mut names = PhpArray::new();
            if let Some(Value::Array(a)) = r {
                for v in a.borrow().values() {
                    names.push(v.clone());
                }
            }
            let first = names
                .get(&ArrayKey::Int(0))
                .map(|v| v.to_php_string().as_bytes().to_vec());
            let (gp, _) = glob_path_split(first.as_deref().unwrap_or(pattern));
            set_prop(obj, b"__spl_glob_path", svec(gp));
            set_prop(obj, b"__spl_dir_names", val_arr(names));
            set_prop(obj, b"__spl_dir_glob", Value::True);
            opened = true;
        } else if is_wrapper_url(path) {
            match self.fs_wrapper_listdir(path) {
                Ok(list) => {
                    let mut names = PhpArray::new();
                    let mut types = PhpArray::new();
                    for n in list {
                        names.push(svec(n));
                        types.push(Value::Long(DT_UNKNOWN));
                    }
                    set_prop(obj, b"__spl_dir_names", val_arr(names));
                    set_prop(obj, b"__spl_dir_types", val_arr(types));
                    set_prop(obj, b"__spl_dir_glob", Value::False);
                    opened = true;
                }
                Err(e) => error = Some(e),
            }
        } else {
            let p = crate::fspath::bytes_path(path);
            match std::fs::read_dir(p) {
                Ok(rd) => {
                    let mut ents: Vec<(Vec<u8>, i64)> =
                        vec![(b".".to_vec(), DT_DIR), (b"..".to_vec(), DT_DIR)];
                    for e in rd.flatten() {
                        let t = match e.file_type() {
                            Ok(ft) if ft.is_symlink() => DT_LNK,
                            Ok(ft) if ft.is_dir() => DT_DIR,
                            Ok(ft) if ft.is_file() => DT_REG,
                            _ => DT_UNKNOWN,
                        };
                        ents.push((crate::fspath::os_bytes(&e.file_name()).into_owned(), t));
                    }
                    ents.sort();
                    let mut names = PhpArray::new();
                    let mut types = PhpArray::new();
                    for (n, t) in ents {
                        names.push(svec(n));
                        types.push(Value::Long(t));
                    }
                    set_prop(obj, b"__spl_dir_names", val_arr(names));
                    set_prop(obj, b"__spl_dir_types", val_arr(types));
                    set_prop(obj, b"__spl_dir_glob", Value::False);
                    opened = true;
                }
                Err(e) => {
                    let msg = if p.exists() && !p.is_dir() {
                        "Not a directory".to_string()
                    } else {
                        strerror(&e)
                    };
                    error = Some(msg);
                }
            }
        }
        set_prop(obj, b"__spl_dir_open", bool_v(opened));
        if opened {
            // intern->u.dir.dirp: the directory stream resource the object
            // owns (listed by get_resources(), not fclose()-able).
            set_prop(obj, b"__spl_dir_res", owned_stream_resource());
        }
        set_prop(obj, b"__spl_dir_cursor", Value::Long(0));
        let stored = if path.len() > 1 && path[path.len() - 1] == b'/' {
            &path[..path.len() - 1]
        } else {
            path
        };
        set_prop(obj, b"__spl_path", svec(stored.to_vec()));
        set_prop(obj, b"__spl_dir_index", Value::Long(0));
        if !opened {
            set_prop(obj, b"__spl_dir_entry", sv(b""));
            let msg = format!(
                "{}({}): Failed to open directory: {}",
                func,
                crate::string::msg_str(path),
                error.unwrap_or_default()
            );
            self.spl_throw(b"UnexpectedValueException", &msg);
        } else {
            Self::fs_dir_read_skip(obj, skip_dots);
        }
    }

    /// spl_filesystem_object_construct()
    fn fs_dir_construct(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        base: &[u8],
        args: &[Value],
    ) -> bool {
        let (func, ctor_flags): (&str, i64) = match base {
            b"globiterator" => ("GlobIterator::__construct", DIT_CTOR_FLAGS | DIT_CTOR_GLOB),
            b"recursivedirectoryiterator" => {
                ("RecursiveDirectoryIterator::__construct", DIT_CTOR_FLAGS)
            }
            b"filesystemiterator" => ("FilesystemIterator::__construct", DIT_CTOR_FLAGS | SKIPDOTS),
            _ => ("DirectoryIterator::__construct", 0),
        };
        let mut flags = ctor_flags & !DIT_CTOR_FLAGS & !DIT_CTOR_GLOB;
        if ctor_flags & DIT_CTOR_FLAGS != 0 {
            if !self.arity_check(func, args.len(), 1, 2) {
                return false;
            }
        } else {
            flags |= CURRENT_AS_SELF;
            if !self.arity_check(func, args.len(), 1, 1) {
                return false;
            }
        }
        let path = match self.spl_arg_string(func, 1, "directory", &args[0]) {
            Some(s) => s.as_bytes().to_vec(),
            None => return false,
        };
        if path.contains(&0) {
            let msg = format!(
                "{}(): Argument #1 ($directory) must not contain any null bytes",
                func
            );
            self.spl_throw(b"ValueError", &msg);
            return false;
        }
        if let Some(f) = args.get(1) {
            match self.spl_arg_long(func, 2, "flags", f) {
                Some(n) => flags = n,
                None => return false,
            }
        }
        if path.is_empty() {
            let msg = format!("{}(): Argument #1 ($directory) must not be empty", func);
            self.spl_throw(b"ValueError", &msg);
            return false;
        }
        if sprop(obj, b"__spl_path").is_some() {
            self.spl_throw(b"Error", "Directory object is already initialized");
            return false;
        }
        set_prop(obj, b"__spl_flags", Value::Long(flags));
        let path = if ctor_flags & DIT_CTOR_GLOB != 0 && !path.starts_with(b"glob://") {
            let mut p = b"glob://".to_vec();
            p.extend_from_slice(&path);
            p
        } else {
            path
        };
        self.fs_dir_open_path(obj, &path, func);
        self.current_exception.is_none()
    }

    /// Validate a class-name argument (zpp "C"/"C!").
    fn fs_class_arg(
        &mut self,
        func: &str,
        v: &Value,
        base: Option<&[u8]>,
        nullable: bool,
    ) -> Result<Option<Vec<u8>>, ()> {
        let v = v.deref();
        if nullable && matches!(v, Value::Null) {
            return Ok(None);
        }
        let s = match &v {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Array(_) | Value::Object(_) => {
                let given = Self::value_type_name(&v);
                let msg = match base {
                    Some(b) => format!(
                        "{}(): Argument #1 ($class) must be a class name derived from {}{}, {} given",
                        func,
                        self.builtin_canonical_name(b),
                        if nullable { " or null" } else { "" },
                        given
                    ),
                    None => format!(
                        "{}(): Argument #1 ($class) must be a valid class name{}, {} given",
                        func,
                        if nullable { " or null" } else { "" },
                        given
                    ),
                };
                self.spl_throw(b"TypeError", &msg);
                return Err(());
            }
            other => other.to_php_string().as_bytes().to_vec(),
        };
        let cl: Vec<u8> = s.iter().map(|b| b.to_ascii_lowercase()).collect();
        if !self.classes.contains_key(&cl) && !self.is_known_builtin_class(&cl) {
            self.try_autoload_class(&s);
            if self.current_exception.is_some() {
                return Err(());
            }
        }
        let exists = self.classes.contains_key(&cl) || self.is_known_builtin_class(&cl);
        if let Some(b) = base {
            let derived = exists && (cl.as_slice() == b || self.class_extends(&cl, b));
            if !derived {
                let msg = format!(
                    "{}(): Argument #1 ($class) must be a class name derived from {}{}, {} given",
                    func,
                    self.builtin_canonical_name(b),
                    if nullable { " or null" } else { "" },
                    crate::string::msg_str(&s)
                );
                self.spl_throw(b"TypeError", &msg);
                return Err(());
            }
        }
        if !exists {
            let msg = format!(
                "{}(): Argument #1 ($class) must be a valid class name{}, {} given",
                func,
                if nullable { " or null" } else { "" },
                crate::string::msg_str(&s)
            );
            self.spl_throw(b"TypeError", &msg);
            return Err(());
        }
        let canonical = self
            .classes
            .get(&cl)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| self.builtin_canonical_name(&cl).into_bytes());
        Ok(Some(canonical))
    }

    fn fs_info_class(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
        sprop(obj, b"__spl_info_class")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| b"SplFileInfo".to_vec())
    }

    fn fs_file_class(obj: &Rc<RefCell<PhpObject>>) -> Vec<u8> {
        sprop(obj, b"__spl_file_class")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| b"SplFileObject".to_vec())
    }

    /// Whether `class_lower`'s constructor is not the built-in one of `base`.
    fn fs_user_ctor(&self, class_lower: &[u8]) -> bool {
        self.spl_has_user_ctor(class_lower)
    }

    /// spl_filesystem_object_create_info()
    fn fs_create_info(&mut self, file_path: &[u8], class: &[u8]) -> Value {
        let new_obj = self.spl_instantiate(class);
        let cl: Vec<u8> = class.iter().map(|b| b.to_ascii_lowercase()).collect();
        if self.fs_user_ctor(&cl) {
            self.call_object_method(
                &Value::Object(new_obj.clone()),
                b"__construct",
                &[svec(file_path.to_vec())],
            );
        } else {
            Self::sfi_set_filename(&new_obj, file_path);
        }
        Value::Object(new_obj)
    }

    /// spl_filesystem_object_create_type(SPL_FS_INFO)
    fn fs_create_info_from(
        &mut self,
        source: &Rc<RefCell<PhpObject>>,
        class: Option<Vec<u8>>,
    ) -> Value {
        if Self::fs_is_dir(source) && Self::fs_entry(source).is_empty() {
            self.spl_throw(b"RuntimeException", "Could not open file");
            return Value::Null;
        }
        let class = class.unwrap_or_else(|| Self::fs_info_class(source));
        let new_obj = self.spl_instantiate(&class);
        let fname = match self.fs_get_file_name(source) {
            Some(f) => f,
            None => return Value::Object(new_obj),
        };
        let cl: Vec<u8> = class.iter().map(|b| b.to_ascii_lowercase()).collect();
        if self.fs_user_ctor(&cl) {
            self.call_object_method(
                &Value::Object(new_obj.clone()),
                b"__construct",
                &[svec(fname)],
            );
        } else {
            set_prop(&new_obj, b"__spl_file_path", svec(fname));
            match Self::fs_get_path(source) {
                Some(p) => set_prop(&new_obj, b"__spl_path", svec(p)),
                None => set_prop(&new_obj, b"__spl_path", Value::Null),
            }
        }
        Value::Object(new_obj)
    }

    /// spl_filesystem_object_create_type(SPL_FS_FILE): openFile().
    fn fs_open_file(&mut self, source: &Rc<RefCell<PhpObject>>, a: &[Value]) -> Value {
        if Self::fs_is_dir(source) && Self::fs_entry(source).is_empty() {
            self.spl_throw(b"RuntimeException", "Could not open file");
            return Value::Null;
        }
        let func = "SplFileInfo::openFile";
        let mode = match a.first() {
            Some(v) => match self.spl_arg_string(func, 1, "mode", v) {
                Some(s) => svec(s.as_bytes().to_vec()),
                None => return Value::Null,
            },
            None => sv(b"r"),
        };
        let class = Self::fs_file_class(source);
        let new_obj = self.spl_instantiate(&class);
        let fname = match self.fs_get_file_name(source) {
            Some(f) => f,
            None => return Value::Object(new_obj),
        };
        let cl: Vec<u8> = class.iter().map(|b| b.to_ascii_lowercase()).collect();
        if self.fs_user_ctor(&cl) {
            self.call_object_method(
                &Value::Object(new_obj.clone()),
                b"__construct",
                &[svec(fname), mode],
            );
        } else {
            let mut cargs = vec![svec(fname), mode];
            cargs.extend_from_slice(a.get(1..).unwrap_or(&[]));
            self.spl_file_object_construct_in(&new_obj, &cargs, func);
        }
        if self.current_exception.is_some() {
            return Value::Null;
        }
        Value::Object(new_obj)
    }

    /// php_stat() for the FileInfoFunction methods.
    fn fs_stat(&mut self, method: &[u8], fname: &[u8]) -> Value {
        use std::os::unix::fs::MetadataExt;
        let (disp, exists_check, link_op) = match method {
            b"getperms" => ("getPerms", false, false),
            b"getinode" => ("getInode", false, false),
            b"getsize" => ("getSize", false, false),
            b"getowner" => ("getOwner", false, false),
            b"getgroup" => ("getGroup", false, false),
            b"getatime" => ("getATime", false, false),
            b"getmtime" => ("getMTime", false, false),
            b"getctime" => ("getCTime", false, false),
            b"gettype" => ("getType", false, true),
            b"iswritable" => ("isWritable", true, false),
            b"isreadable" => ("isReadable", true, false),
            b"isexecutable" => ("isExecutable", true, false),
            b"isfile" => ("isFile", true, false),
            b"isdir" => ("isDir", true, false),
            _ => ("isLink", true, true),
        };
        if fname.is_empty() {
            return Value::False;
        }
        if fname.contains(&0) {
            if !exists_check {
                let msg = format!("SplFileInfo::{}(): Filename contains null byte", disp);
                self.spl_throw(b"RuntimeException", &msg);
            }
            return Value::False;
        }
        match method {
            b"iswritable" | b"isreadable" | b"isexecutable" => {
                let f = match method {
                    b"iswritable" => &b"is_writable"[..],
                    b"isreadable" => b"is_readable",
                    _ => b"is_executable",
                };
                return self
                    .spl_call_builtin(f, &[svec(fname.to_vec())])
                    .unwrap_or(Value::False);
            }
            _ => {}
        }
        let p = crate::fspath::bytes_path(fname);
        let st = if is_wrapper_url(fname) {
            self.fs_wrapper_stat(fname, link_op)
        } else {
            let local = crate::fspath::bytes_path(file_url_path(fname));
            let md = if link_op {
                std::fs::symlink_metadata(local)
            } else {
                std::fs::metadata(local)
            };
            md.ok().map(|m| FsStat {
                mode: m.mode() as i64,
                ino: m.ino() as i64,
                size: m.size() as i64,
                uid: m.uid() as i64,
                gid: m.gid() as i64,
                atime: m.atime(),
                mtime: m.mtime(),
                ctime: m.ctime(),
            })
        };
        let m = match st {
            Some(m) => m,
            None => {
                if !exists_check {
                    let msg = format!(
                        "SplFileInfo::{}(): {}stat failed for {}",
                        disp,
                        if link_op { "L" } else { "" },
                        crate::string::msg_str(fname)
                    );
                    self.spl_throw(b"RuntimeException", &msg);
                }
                return Value::False;
            }
        };
        let fmt = m.mode & S_IFMT;
        match method {
            b"getperms" => Value::Long(m.mode),
            b"getinode" => Value::Long(m.ino),
            b"getsize" => Value::Long(m.size),
            b"getowner" => Value::Long(m.uid),
            b"getgroup" => Value::Long(m.gid),
            b"getatime" => Value::Long(m.atime),
            b"getmtime" => Value::Long(m.mtime),
            b"getctime" => Value::Long(m.ctime),
            b"gettype" => {
                let t: &[u8] = match fmt {
                    0o120000 => b"link",
                    0o040000 => b"dir",
                    0o100000 => b"file",
                    0o010000 => b"fifo",
                    0o020000 => b"char",
                    0o060000 => b"block",
                    0o140000 => b"socket",
                    _ => b"unknown",
                };
                sv(t)
            }
            b"isfile" => bool_v(fmt == 0o100000),
            b"isdir" => bool_v(fmt == 0o040000),
            _ => bool_v(fmt == 0o120000),
        }
    }

    /// spl_filesystem_object_get_debug_info()
    pub(crate) fn fs_debug_info(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpArray {
        let mut out = self.spl_std_props(obj);
        let key = |c: &[u8], n: &[u8]| ArrayKey::String(PhpString::from_vec(mangle_private(c, n)));
        // Computed without caching: callers may hold a borrow of obj.
        let file_name = match sprop(obj, b"__spl_file_path") {
            Some(f) => Some(f),
            None if Self::fs_is_dir(obj) && !Self::fs_entry(obj).is_empty() => {
                let entry = Self::fs_entry(obj);
                Some(match Self::fs_get_path(obj) {
                    None => entry,
                    Some(mut p) => {
                        p.push(b'/');
                        p.extend_from_slice(&entry);
                        p
                    }
                })
            }
            None => None,
        };
        let path_name = if Self::fs_is_dir(obj) && Self::fs_entry(obj).is_empty() {
            Vec::new()
        } else {
            file_name.clone().unwrap_or_default()
        };
        out.set(key(b"SplFileInfo", b"pathName"), svec(path_name));
        if let Some(f) = file_name {
            out.set(
                key(b"SplFileInfo", b"fileName"),
                svec(Self::fs_short_name(obj, &f)),
            );
        }
        let cl = obj_class_lower(obj);
        if Self::fs_is_dir(obj) {
            let glob = if Self::fs_is_glob(obj) {
                svec(sprop(obj, b"__spl_path").unwrap_or_default())
            } else {
                Value::False
            };
            out.set(key(b"DirectoryIterator", b"glob"), glob);
            let sub = sprop(obj, b"__spl_sub_path").unwrap_or_default();
            out.set(
                key(b"RecursiveDirectoryIterator", b"subPathName"),
                svec(sub),
            );
        } else if self.spl_find_base(&cl, &[b"splfileobject"]).is_some()
            && !matches!(prop(obj, b"__spl_stream"), Value::Null | Value::Undef)
        {
            let mode = prop(obj, b"__spl_open_mode");
            out.set(key(b"SplFileObject", b"openMode"), mode);
            let d = match prop(obj, b"__spl_csv_delimiter") {
                Value::String(s) => Value::String(s),
                _ => sv(b","),
            };
            let e = match prop(obj, b"__spl_csv_enclosure") {
                Value::String(s) => Value::String(s),
                _ => sv(b"\""),
            };
            out.set(key(b"SplFileObject", b"delimiter"), d);
            out.set(key(b"SplFileObject", b"enclosure"), e);
        }
        out
    }

    /// spl_filesystem_object_get_method_check() for SplFileObject and
    /// GlobIterator: methods cannot be looked up on an object whose
    /// constructor never ran.
    pub(crate) fn spl_fs_method_check_fails(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        class_lower: &[u8],
    ) -> bool {
        {
            let ob = obj.borrow();
            if ob.has_property(b"__spl_stream")
                || ob.has_property(b"__spl_dir_open")
                || ob.has_property(b"__spl_orig_path")
            {
                return false;
            }
        }
        self.spl_find_base(class_lower, &[b"splfileobject", b"globiterator"])
            .is_some()
    }

    /// CHECK_DIRECTORY_ITERATOR_IS_INITIALIZED
    fn fs_check_dir(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        if !Self::fs_dir_open(obj) {
            self.spl_throw(b"Error", "Object not initialized");
            return false;
        }
        true
    }

    /// Dispatch a method declared by `class` (splfileinfo or one of the
    /// directory iterator classes). Returns None when `class` does not
    /// declare `method` or when it needs arguments that were not passed.
    pub(crate) fn spl_fs_call(
        &mut self,
        class: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: Option<&[Value]>,
    ) -> Option<Value> {
        // parent:: calls name the immediate parent; resolve the declaring class.
        let mut class: &[u8] = class;
        while crate::spl_stubs::spl_stub_method(class, method).is_none() {
            class = match class {
                b"globiterator" | b"recursivedirectoryiterator" => b"filesystemiterator",
                b"filesystemiterator" => b"directoryiterator",
                b"directoryiterator" => b"splfileinfo",
                _ => return None,
            };
        }
        let stub = crate::spl_stubs::spl_stub_method(class, method)?;
        if method == b"__construct" {
            return None;
        }
        if stub.required > 0 && args.is_none() {
            return None;
        }
        let a = args.unwrap_or(&[]);
        // GlobIterator: spl_filesystem_object_get_method_check()
        let cl = obj_class_lower(obj);
        // (count() reaches GlobIterator::count() through count_elements.)
        if method != b"count"
            && self.spl_find_base(&cl, &[b"globiterator"]).is_some()
            && !Self::fs_dir_open(obj)
            && sprop(obj, b"__spl_orig_path").is_none()
        {
            return self.spl_throw(
                b"Error",
                "The parent constructor was not called: the object is in an invalid state",
            );
        }
        let r = match (class, method) {
            // ---------------- SplFileInfo ----------------
            (b"splfileinfo", b"getpath") => svec(Self::fs_get_path(obj).unwrap_or_default()),
            (b"splfileinfo", b"getfilename") => match sprop(obj, b"__spl_file_path") {
                Some(f) => svec(Self::fs_short_name(obj, &f)),
                None => return self.spl_throw(b"Error", "Object not initialized"),
            },
            (b"splfileinfo", b"getextension") => match sprop(obj, b"__spl_file_path") {
                Some(f) => {
                    let base = php_basename(&Self::fs_short_name(obj, &f), b"");
                    match base.iter().rposition(|&c| c == b'.') {
                        Some(i) => svec(base[i + 1..].to_vec()),
                        None => sv(b""),
                    }
                }
                None => return self.spl_throw(b"Error", "Object not initialized"),
            },
            (b"splfileinfo", b"getbasename") => {
                let suffix = match a.first() {
                    Some(v) => {
                        match self.spl_arg_string("SplFileInfo::getBasename", 1, "suffix", v) {
                            Some(s) => s.as_bytes().to_vec(),
                            None => return Some(Value::Null),
                        }
                    }
                    None => Vec::new(),
                };
                match sprop(obj, b"__spl_file_path") {
                    Some(f) => svec(php_basename(&Self::fs_short_name(obj, &f), &suffix)),
                    None => return self.spl_throw(b"Error", "Object not initialized"),
                }
            }
            (b"splfileinfo", b"getpathname") | (b"splfileinfo", b"__tostring") => {
                let p = self.fs_get_pathname(obj);
                if self.current_exception.is_some() {
                    return Some(Value::Null);
                }
                svec(p.unwrap_or_default())
            }
            (b"splfileinfo", m)
                if matches!(
                    m,
                    b"getperms"
                        | b"getinode"
                        | b"getsize"
                        | b"getowner"
                        | b"getgroup"
                        | b"getatime"
                        | b"getmtime"
                        | b"getctime"
                        | b"gettype"
                        | b"iswritable"
                        | b"isreadable"
                        | b"isexecutable"
                        | b"isfile"
                        | b"isdir"
                        | b"islink"
                ) =>
            {
                let f = self.fs_get_file_name(obj)?;
                self.fs_stat(m, &f)
            }
            (b"splfileinfo", b"getlinktarget") => {
                let f = match sprop(obj, b"__spl_file_path") {
                    Some(f) => f,
                    None => match self.fs_get_file_name(obj) {
                        Some(f) => f,
                        None => return Some(Value::Null),
                    },
                };
                if f.is_empty() {
                    return self.spl_throw(b"ValueError", "Filename must not be empty");
                }
                let p = crate::string::msg_str(&f).into_owned();
                let fp = crate::fspath::bytes_path(&f);
                let full = if f.starts_with(b"/") {
                    fp.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map(|c| c.join(fp))
                        .unwrap_or_else(|_| fp.to_path_buf())
                };
                match std::fs::read_link(&full) {
                    Ok(t) => svec(crate::fspath::os_bytes(t.as_os_str()).into_owned()),
                    Err(e) => {
                        let msg = format!("Unable to read link {}, error: {}", p, strerror(&e));
                        self.spl_throw(b"RuntimeException", &msg);
                        Value::False
                    }
                }
            }
            (b"splfileinfo", b"getrealpath") => {
                if Self::fs_is_dir(obj)
                    && sprop(obj, b"__spl_file_path").is_none()
                    && !Self::fs_entry(obj).is_empty()
                {
                    self.fs_get_file_name(obj)?;
                }
                let filename =
                    sprop(obj, b"__spl_orig_path").or_else(|| sprop(obj, b"__spl_file_path"));
                match filename {
                    Some(f) => {
                        let r = if f.is_empty() {
                            std::env::current_dir()
                        } else {
                            std::fs::canonicalize(crate::fspath::bytes_path(&f))
                        };
                        match r {
                            Ok(rp) => svec(crate::fspath::os_bytes(rp.as_os_str()).into_owned()),
                            Err(_) => Value::False,
                        }
                    }
                    None => Value::False,
                }
            }
            (b"splfileinfo", b"openfile") => self.fs_open_file(obj, a),
            (b"splfileinfo", b"setfileclass") | (b"splfileinfo", b"setinfoclass") => {
                let (func, base, pname, default): (&str, &[u8], &[u8], &[u8]) =
                    if method == b"setfileclass" {
                        (
                            "SplFileInfo::setFileClass",
                            b"splfileobject",
                            b"__spl_file_class",
                            b"SplFileObject",
                        )
                    } else {
                        (
                            "SplFileInfo::setInfoClass",
                            b"splfileinfo",
                            b"__spl_info_class",
                            b"SplFileInfo",
                        )
                    };
                let c = match a.first() {
                    Some(v) => match self.fs_class_arg(func, v, Some(base), false) {
                        Ok(c) => c.unwrap_or_else(|| default.to_vec()),
                        Err(()) => return Some(Value::Null),
                    },
                    None => default.to_vec(),
                };
                set_prop(obj, pname, svec(c));
                Value::Null
            }
            (b"splfileinfo", b"getfileinfo") => {
                let ic = Self::fs_info_class(obj);
                let icl: Vec<u8> = ic.iter().map(|b| b.to_ascii_lowercase()).collect();
                let c = match a.first() {
                    Some(v) => {
                        match self.fs_class_arg("SplFileInfo::getFileInfo", v, Some(&icl), true) {
                            Ok(c) => c,
                            Err(()) => return Some(Value::Null),
                        }
                    }
                    None => None,
                };
                self.fs_create_info_from(obj, c)
            }
            (b"splfileinfo", b"getpathinfo") => {
                let c = match a.first() {
                    Some(v) => match self.fs_class_arg("SplFileInfo::getPathInfo", v, None, true) {
                        Ok(c) => c,
                        Err(()) => return Some(Value::Null),
                    },
                    None => None,
                };
                let c = match c {
                    None => Self::fs_info_class(obj),
                    Some(c) => {
                        let ccl: Vec<u8> = c.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if ccl != b"splfileinfo"
                            && self.spl_find_base(&ccl, &[b"splfileinfo"]).is_none()
                        {
                            let msg = format!(
                                "SplFileInfo::getPathInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, {} given",
                                crate::string::msg_str(&c)
                            );
                            return self.spl_throw(b"TypeError", &msg);
                        }
                        c
                    }
                };
                let p = self.fs_get_pathname(obj);
                if self.current_exception.is_some() {
                    return Some(Value::Null);
                }
                match p {
                    Some(p) if !p.is_empty() => {
                        let d = dirname(&p);
                        self.fs_create_info(&d, &c)
                    }
                    _ => Value::Null,
                }
            }
            (b"splfileinfo", b"__debuginfo") => val_arr(self.fs_debug_info(obj)),
            (b"splfileinfo", b"_bad_state_ex") => {
                return self.spl_throw(
                    b"Error",
                    "The parent constructor was not called: the object is in an invalid state",
                );
            }
            // ---------------- DirectoryIterator ----------------
            (b"directoryiterator", m) => {
                if !self.fs_check_dir(obj) {
                    return Some(Value::Null);
                }
                match m {
                    b"getfilename" | b"__tostring" => svec(Self::fs_entry(obj)),
                    b"getextension" => {
                        let base = php_basename(&Self::fs_entry(obj), b"");
                        match base.iter().rposition(|&c| c == b'.') {
                            Some(i) => svec(base[i + 1..].to_vec()),
                            None => sv(b""),
                        }
                    }
                    b"getbasename" => {
                        let suffix = match a.first() {
                            Some(v) => match self.spl_arg_string(
                                "DirectoryIterator::getBasename",
                                1,
                                "suffix",
                                v,
                            ) {
                                Some(s) => s.as_bytes().to_vec(),
                                None => return Some(Value::Null),
                            },
                            None => Vec::new(),
                        };
                        svec(php_basename(&Self::fs_entry(obj), &suffix))
                    }
                    b"isdot" => bool_v(is_dot(&Self::fs_entry(obj))),
                    b"rewind" => {
                        set_prop(obj, b"__spl_dir_index", Value::Long(0));
                        Self::fs_dir_rewind(obj);
                        Self::fs_dir_read(obj);
                        Value::Null
                    }
                    b"valid" => bool_v(!Self::fs_entry(obj).is_empty()),
                    b"key" => Value::Long(lprop(obj, b"__spl_dir_index")),
                    b"current" => Value::Object(obj.clone()),
                    b"next" => {
                        let skip = Self::fs_flags(obj) & SKIPDOTS != 0;
                        set_prop(
                            obj,
                            b"__spl_dir_index",
                            Value::Long(lprop(obj, b"__spl_dir_index") + 1),
                        );
                        Self::fs_dir_read_skip(obj, skip);
                        set_prop(obj, b"__spl_file_path", Value::Null);
                        Value::Null
                    }
                    b"seek" => {
                        let Some(arg) = a.first() else {
                            return self.spl_throw(
                                b"ArgumentCountError",
                                "DirectoryIterator::seek() expects exactly 1 argument, 0 given",
                            );
                        };
                        let pos =
                            match self.spl_arg_long("DirectoryIterator::seek", 1, "offset", arg) {
                                Some(n) => n,
                                None => return Some(Value::Null),
                            };
                        let ov = Value::Object(obj.clone());
                        if lprop(obj, b"__spl_dir_index") > pos {
                            self.call_object_method(&ov, b"rewind", &[]);
                            if self.current_exception.is_some() {
                                return Some(Value::Null);
                            }
                        }
                        let mut guard = 0u64;
                        while lprop(obj, b"__spl_dir_index") < pos {
                            let valid = self
                                .call_object_method(&ov, b"valid", &[])
                                .unwrap_or(Value::False);
                            if self.current_exception.is_some() {
                                return Some(Value::Null);
                            }
                            if !valid.is_truthy() {
                                let msg = format!("Seek position {} is out of range", pos);
                                return self.spl_throw(b"OutOfBoundsException", &msg);
                            }
                            self.call_object_method(&ov, b"next", &[]);
                            if self.current_exception.is_some() {
                                return Some(Value::Null);
                            }
                            guard += 1;
                            if guard > 100_000_000 {
                                break;
                            }
                        }
                        Value::Null
                    }
                    _ => return None,
                }
            }
            // ---------------- FilesystemIterator ----------------
            (b"filesystemiterator", b"rewind") => {
                let skip = Self::fs_flags(obj) & SKIPDOTS != 0;
                set_prop(obj, b"__spl_dir_index", Value::Long(0));
                Self::fs_dir_rewind(obj);
                Self::fs_dir_read_skip(obj, skip);
                Value::Null
            }
            (b"filesystemiterator", b"key") => {
                if Self::fs_flags(obj) & KEY_MODE_MASK == KEY_AS_FILENAME {
                    svec(Self::fs_entry(obj))
                } else {
                    svec(self.fs_get_file_name(obj)?)
                }
            }
            (b"filesystemiterator", b"current") => {
                let mode = Self::fs_flags(obj) & CURRENT_MODE_MASK;
                if mode == CURRENT_AS_PATHNAME {
                    svec(self.fs_get_file_name(obj)?)
                } else if mode == 0 {
                    self.fs_get_file_name(obj)?;
                    self.fs_create_info_from(obj, None)
                } else {
                    Value::Object(obj.clone())
                }
            }
            (b"filesystemiterator", b"getflags") => {
                Value::Long(Self::fs_flags(obj) & (KEY_MODE_MASK | CURRENT_MODE_MASK | OTHERS_MASK))
            }
            (b"filesystemiterator", b"setflags") => {
                let Some(arg) = a.first() else {
                    return self.spl_throw(
                        b"ArgumentCountError",
                        "FilesystemIterator::setFlags() expects exactly 1 argument, 0 given",
                    );
                };
                let f = match self.spl_arg_long("FilesystemIterator::setFlags", 1, "flags", arg) {
                    Some(n) => n,
                    None => return Some(Value::Null),
                };
                let mask = KEY_MODE_MASK | CURRENT_MODE_MASK | OTHERS_MASK;
                let cur = Self::fs_flags(obj);
                set_prop(obj, b"__spl_flags", Value::Long((cur & !mask) | (f & mask)));
                Value::Null
            }
            // ---------------- RecursiveDirectoryIterator ----------------
            (b"recursivedirectoryiterator", b"haschildren") => {
                let allow_links = a.first().map(|v| v.is_truthy()).unwrap_or(false);
                let entry = Self::fs_entry(obj);
                if entry.is_empty() || is_dot(&entry) {
                    Value::False
                } else {
                    let dt = lprop(obj, b"__spl_dir_dtype");
                    if dt == DT_DIR {
                        Value::True
                    } else if dt == DT_REG {
                        Value::False
                    } else {
                        let f = self.fs_get_file_name(obj)?;
                        let p = crate::fspath::bytes_path(&f);
                        if is_wrapper_url(&f) {
                            let st = self.fs_wrapper_stat(&f, false);
                            return Some(bool_v(st.is_some_and(|st| st.mode & S_IFMT == 0o040000)));
                        }
                        match std::fs::symlink_metadata(p) {
                            Err(_) => Value::False,
                            Ok(m) if !m.file_type().is_symlink() => bool_v(m.is_dir()),
                            Ok(_) => {
                                if !allow_links && Self::fs_flags(obj) & FOLLOW_SYMLINKS == 0 {
                                    Value::False
                                } else {
                                    bool_v(p.is_dir())
                                }
                            }
                        }
                    }
                }
            }
            (b"recursivedirectoryiterator", b"getchildren") => {
                let f = self.fs_get_file_name(obj)?;
                let flags = Self::fs_flags(obj);
                let class = obj.borrow().class_name.clone();
                let child = self.spl_instantiate(&class);
                let cv = Value::Object(child.clone());
                let cargs = [svec(f), Value::Long(flags)];
                if self.spl_has_user_ctor(&cl) {
                    self.call_object_method(&cv, b"__construct", &cargs);
                } else {
                    let base = self
                        .spl_find_base(&cl, DIR_CLASSES)
                        .unwrap_or(b"recursivedirectoryiterator");
                    self.spl_push_ctor_frame(&child, &cargs);
                    self.fs_dir_construct(&child, base, &cargs);
                    self.call_stack.pop();
                }
                if self.current_exception.is_some() {
                    return Some(Value::Null);
                }
                let entry = Self::fs_entry(obj);
                let sub = match sprop(obj, b"__spl_sub_path") {
                    Some(s) if !s.is_empty() => {
                        let mut s = s;
                        s.push(b'/');
                        s.extend_from_slice(&entry);
                        s
                    }
                    _ => entry,
                };
                set_prop(&child, b"__spl_sub_path", svec(sub));
                for p in [&b"__spl_info_class"[..], b"__spl_file_class"] {
                    let v = prop(obj, p);
                    if !matches!(v, Value::Null | Value::Undef) {
                        set_prop(&child, p, v);
                    }
                }
                cv
            }
            (b"recursivedirectoryiterator", b"getsubpath") => {
                svec(sprop(obj, b"__spl_sub_path").unwrap_or_default())
            }
            (b"recursivedirectoryiterator", b"getsubpathname") => {
                let entry = Self::fs_entry(obj);
                match sprop(obj, b"__spl_sub_path") {
                    Some(mut s) => {
                        s.push(b'/');
                        s.extend_from_slice(&entry);
                        svec(s)
                    }
                    None => svec(entry),
                }
            }
            // ---------------- GlobIterator ----------------
            (b"globiterator", b"count") => {
                if Self::fs_is_glob(obj) {
                    let n = match prop(obj, b"__spl_dir_names") {
                        Value::Array(a) => a.borrow().len() as i64,
                        _ => 0,
                    };
                    Value::Long(n)
                } else {
                    return self.spl_throw(b"Error", "GlobIterator is not initialized");
                }
            }
            _ => return None,
        };
        let _ = UNIXPATHS;
        Some(r)
    }
}

/// Native constructor handler for DirectoryIterator, FilesystemIterator,
/// RecursiveDirectoryIterator and GlobIterator.
pub fn spl_dir_construct_native(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let cl = obj_class_lower(&obj);
    let base = vm
        .spl_find_base(&cl, DIR_CLASSES)
        .unwrap_or(b"directoryiterator");
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let ok = vm.fs_dir_construct(&obj, base, &args[1..]);
    vm.call_stack.pop();
    if ok {
        Ok(())
    } else {
        let msg = match &vm.current_exception {
            Some(Value::Object(e)) => e
                .borrow()
                .get_property(b"message")
                .to_php_string()
                .to_msg_string(),
            _ => String::new(),
        };
        Err(msg)
    }
}

/// A (directory) stream resource owned by an object: registered as a
/// "stream", refused by fclose(), gone when its last reference is.
pub fn owned_stream_resource() -> Value {
    fn free(_vm: &mut Vm, id: i64) {
        crate::value::set_resource_no_fclose(id, false);
        crate::value::remove_resource(id);
    }
    let id = crate::value::alloc_resource_id();
    crate::value::register_resource(id, b"stream");
    crate::value::set_resource_no_fclose(id, true);
    crate::value::set_resource_dtor(id, free);
    Value::resource(id)
}
