//! php_resolve_path(): where include/require look for a relative file.

use crate::vm::Vm;

fn canonical(p: &std::path::Path) -> Option<String> {
    if !p.is_file() {
        return None;
    }
    std::fs::canonicalize(p)
        .ok()
        .map(|c| c.to_string_lossy().into_owned())
}

impl Vm {
    /// Resolve a relative include path like php_resolve_path(): "./x" and
    /// "../x" are relative to the working directory only; other names are
    /// looked up along include_path, then in the directory of the running
    /// script. Returns the path the open should use (a not-found name is
    /// returned relative to the running script's directory).
    pub fn resolve_include_path(&self, path: &str) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        let explicit_relative =
            path.starts_with("./") || path.starts_with("../") || path == "." || path == "..";
        if explicit_relative {
            let joined = cwd.join(path);
            return canonical(&joined).unwrap_or_else(|| joined.to_string_lossy().into_owned());
        }
        let include_path = self
            .constants
            .get(b"include_path".as_slice())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_else(|| ".".to_string());
        for dir in include_path.split(':') {
            if dir.is_empty() || dir.contains("://") {
                continue;
            }
            let base = if dir == "." {
                cwd.clone()
            } else {
                cwd.join(dir)
            };
            if let Some(found) = canonical(&base.join(path)) {
                return found;
            }
        }
        let script_dir = match self.current_file.rfind('/') {
            Some(pos) => &self.current_file[..pos],
            None => ".",
        };
        let joined = format!("{}/{}", script_dir, path);
        canonical(std::path::Path::new(&joined)).unwrap_or(joined)
    }
}
