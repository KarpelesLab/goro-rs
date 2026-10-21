//! open_basedir enforcement, ported from php-src main/fopen_wrappers.c.

use crate::vm::Vm;

pub const MAXPATHLEN: usize = 4096;

/// The current open_basedir INI value ("" when unrestricted).
pub fn open_basedir_value(vm: &Vm) -> String {
    vm.constants
        .get(b"open_basedir".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default()
}

/// expand_filepath(): make `path` absolute relative to the CWD and collapse
/// `.`, `..` and duplicate slashes lexically (no symlink resolution).
pub fn expand_filepath(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let base = if path.starts_with('/') {
        String::new()
    } else {
        std::env::current_dir().ok()?.to_string_lossy().to_string()
    };
    let full = if base.is_empty() {
        path.to_string()
    } else {
        format!("{}/{}", base, path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for comp in full.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// php_check_specific_open_basedir: true if `path` is inside `basedir`.
pub fn check_specific_open_basedir(basedir: &str, path: &str) -> bool {
    let local_basedir = if basedir == "." {
        match std::env::current_dir() {
            Ok(d) => d.to_string_lossy().to_string(),
            Err(_) => basedir.to_string(),
        }
    } else {
        basedir.to_string()
    };
    if path.len() > MAXPATHLEN - 1 {
        return false;
    }
    let expanded = match expand_filepath(path) {
        Some(e) => e,
        None => return false,
    };
    let mut path_tmp = expanded.clone();
    let mut nesting = 0;
    let mut resolved_name: String;
    loop {
        match std::fs::canonicalize(&path_tmp) {
            Ok(p) => {
                resolved_name = p.to_string_lossy().to_string();
                break;
            }
            Err(_) => {
                if nesting == 0 {
                    if let Ok(target) = std::fs::read_link(&path_tmp) {
                        path_tmp = target.to_string_lossy().to_string();
                    }
                }
                match path_tmp.rfind('/') {
                    None => return false,
                    Some(pos) => path_tmp.truncate(pos),
                }
                if path_tmp.is_empty() {
                    resolved_name = "/".to_string();
                    break;
                }
                nesting += 1;
            }
        }
    }
    let mut resolved_basedir = match expand_filepath(&local_basedir) {
        Some(b) => b,
        None => return false,
    };
    if !resolved_basedir.ends_with('/') {
        resolved_basedir.push('/');
    }
    if expanded.ends_with('/') && !resolved_name.ends_with('/') {
        resolved_name.push('/');
    }
    let rb = resolved_basedir.as_bytes();
    let rn = resolved_name.as_bytes();
    if rn.starts_with(rb) {
        return true;
    }
    // "/openbasedir/" and "/openbasedir" are the same directory
    rb.len() == rn.len() + 1 && rb.ends_with(b"/") && rb[..rn.len()] == *rn
}

/// php_check_open_basedir_ex. `func_name` is the docref prefix.
pub fn check_open_basedir_ex(vm: &mut Vm, func_name: &str, path: &str, warn: bool) -> bool {
    let basedir_str = open_basedir_value(vm);
    if basedir_str.is_empty() {
        return true;
    }
    // Only plain files are subject to open_basedir.
    let path = match path.strip_prefix("file://") {
        Some(p) => p,
        None => {
            // Wrapper URLs (and data:, which needs no "//") are not files.
            if path.contains("://") || path.starts_with("data:") {
                return true;
            }
            path
        }
    };
    let prefix = if func_name.is_empty() {
        String::new()
    } else {
        format!("{}(): ", func_name)
    };
    if path.len() > MAXPATHLEN - 1 {
        if warn {
            vm.emit_warning(&format!(
                "{}File name is longer than the maximum allowed path length on this platform ({}): {}",
                prefix, MAXPATHLEN, path
            ));
        }
        return false;
    }
    for dir in basedir_str.split(':') {
        if dir.is_empty() {
            continue;
        }
        if check_specific_open_basedir(dir, path) {
            return true;
        }
    }
    if warn {
        vm.emit_warning(&format!(
            "{}open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
            prefix, path, basedir_str
        ));
    }
    false
}

/// OnUpdateBaseDir at runtime: the new value must be non-empty and at least
/// as restrictive as the current one. Returns the value to store.
pub fn update_open_basedir(vm: &mut Vm, new_value: &str) -> Option<String> {
    let current = open_basedir_value(vm);
    if current.is_empty() {
        return Some(new_value.to_string());
    }
    if new_value.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for part in new_value.split(':') {
        if part.is_empty() {
            continue;
        }
        let resolved = expand_filepath(part)?;
        if !check_open_basedir_ex(vm, "", &resolved, false) {
            return None;
        }
        out.push(resolved);
    }
    Some(out.join(":"))
}
