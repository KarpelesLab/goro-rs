//! php_resolve_path(): where include/require look for a relative file.
//! Paths are strings in the message encoding (`crate::fspath`).

use crate::vm::Vm;

fn canonical(p: &std::path::Path) -> Option<String> {
    if !p.is_file() {
        return None;
    }
    std::fs::canonicalize(p)
        .ok()
        .map(|c| crate::fspath::path_msg(&c))
}

/// expand_filepath(): `.` and `..` components removed lexically (the
/// plain-files wrapper opens `missing/../x` as `x`).
fn lexical_normalize(p: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Vm {
    /// Resolve a relative include path like php_resolve_path(): "./x" and
    /// "../x" are relative to the working directory only; other names are
    /// looked up along include_path, then in the directory of the running
    /// script. Returns the path the open should use (a not-found name is
    /// returned relative to the running script's directory).
    pub fn resolve_include_path(&self, path: &str) -> String {
        self.resolve_include_path_from(path, &self.current_file)
    }

    /// resolve_include_path() with `exec_file` as the running script (the
    /// file of the executing function, zend_get_executed_filename()).
    pub fn resolve_include_path_from(&self, path: &str, exec_file: &str) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        let explicit_relative =
            path.starts_with("./") || path.starts_with("../") || path == "." || path == "..";
        if explicit_relative {
            let joined = cwd.join(crate::fspath::msg_path(path));
            return canonical(&joined).unwrap_or_else(|| crate::fspath::path_msg(&joined));
        }
        let include_path = self
            .constants
            .get(b"include_path".as_slice())
            .map(|v| v.to_php_string().to_msg_string())
            .unwrap_or_else(|| ".".to_string());
        for dir in include_path.split(':') {
            if dir.is_empty() || dir.contains("://") {
                continue;
            }
            let base = if dir == "." {
                cwd.clone()
            } else {
                cwd.join(crate::fspath::msg_path(dir))
            };
            if let Some(found) = canonical(&base.join(crate::fspath::msg_path(path))) {
                return found;
            }
        }
        let script_dir = match exec_file.rfind('/') {
            Some(pos) => &exec_file[..pos],
            None => ".",
        };
        let joined = format!("{}/{}", script_dir, path);
        canonical(&crate::fspath::msg_path(&joined))
            // (not found: the plain-files wrapper opens the name itself,
            // relative to the working directory)
            .or_else(|| canonical(&lexical_normalize(&cwd.join(crate::fspath::msg_path(path)))))
            .unwrap_or(joined)
    }
}

/// The "Failed to open stream" reason of a failed include, and the path the
/// warning names: include_once/require_once name the resolved real path when
/// the name resolves (zend_resolve_path(); "" resolves to the working
/// directory). Opening a directory fails in the plain-files wrapper's
/// fstat()-less read as ENOTTY.
pub(crate) fn include_failure_reason(
    io_err: &std::io::Error,
    is_once: bool,
    path: &str,
    abs_path: &str,
) -> (String, Option<String>) {
    let reason = if io_err.kind() == std::io::ErrorKind::IsADirectory {
        "Inappropriate ioctl for device".to_string()
    } else {
        let s = io_err.to_string();
        match s.find(" (os error") {
            Some(i) => s[..i].to_string(),
            None => s,
        }
    };
    let opened = if is_once {
        let candidate = if path.is_empty() {
            std::env::current_dir().ok()
        } else {
            Some(crate::fspath::msg_path(abs_path).into_owned())
        };
        candidate
            .filter(|p| p.exists())
            .and_then(|p| std::fs::canonicalize(p).ok())
            .map(|c| crate::fspath::path_msg(&c))
    } else {
        None
    };
    (reason, opened)
}

impl Vm {
    /// zend_include_or_eval(): a file name with a NUL byte fails without
    /// opening a stream; the ZMSG_FAILED_*_FOPEN message stops at the NUL.
    /// Returns the flow when an Error was thrown (require), None when the
    /// include evaluates to false.
    #[inline(never)]
    pub(crate) fn include_nul_path_failure(
        &mut self,
        path: &str,
        include_kind: u32,
        line: u32,
        exception_handlers: &mut Vec<(u32, u32, u32, usize, usize)>,
        pending_calls_catch_depth: &mut Option<usize>,
        ip: &mut usize,
    ) -> Result<Option<crate::vm::Flow>, crate::vm::VmError> {
        let shown = &path[..path.find('\0').unwrap_or(path.len())];
        let kind_name = match include_kind {
            1 => "include_once",
            2 => "require",
            3 => "require_once",
            _ => "include",
        };
        let include_path = self
            .constants
            .get(b"include_path".as_slice())
            .map(|v| v.to_php_string().to_msg_string())
            .unwrap_or_else(|| ".".to_string());
        if include_kind == 2 || include_kind == 3 {
            let msg = format!(
                "Failed opening required '{}' (include_path='{}')",
                shown, include_path
            );
            if self.current_exception.is_none() {
                let exc = self.create_exception(b"Error", &msg, line);
                self.current_exception = Some(exc);
            }
            if let Some((catch_target, _, _, _, saved_pc)) = exception_handlers.pop() {
                *pending_calls_catch_depth = Some(saved_pc);
                *ip = catch_target as usize;
                return Ok(Some(crate::vm::Flow::Continue));
            }
            return Err(crate::vm::VmError {
                message: format!("Uncaught Error: {}", msg),
                line,
            });
        }
        let msg = format!(
            "{}(): Failed opening '{}' for inclusion (include_path='{}')",
            kind_name, shown, include_path
        );
        let file = self.current_file.clone();
        self.call_stack
            .push((kind_name.to_string(), file, line, Vec::new(), false));
        self.emit_warning_at(&msg, line);
        self.call_stack.pop();
        Ok(None)
    }
}
