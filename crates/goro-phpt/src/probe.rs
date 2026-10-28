//! Asking the binary under test which extensions it has.
//!
//! The documentation of EXTENSIONS: the listed extensions are loaded, and a
//! test is skipped when a shared module cannot be found. The runner asks the
//! binary for its built-in modules with `-n -m` (the CLI's module list), and
//! for the shared modules in its `extension_dir`. A shared one is loaded with
//! `-d extension=NAME` (`-d zend_extension=NAME` for opcache, which `-m`
//! lists as a Zend module).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::proc::{Capture, Exit, Run, run};

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    /// False when the binary did not answer `-m`: then every extension is
    /// assumed present and nothing is loaded.
    pub known: bool,
    /// Built-in modules, lower case.
    pub builtin: HashSet<String>,
    /// Shared modules found in extension_dir, lower case.
    pub shared: HashSet<String>,
}

/// What to do for one EXTENSIONS entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtAction {
    Present,
    Load(String),
    Missing,
}

const ZEND_EXTENSIONS: &[&str] = &["opcache", "xdebug"];

impl Extensions {
    pub fn probe(php: &Path, cwd: &Path) -> Extensions {
        let mut ext = Extensions::default();
        let args: Vec<OsString> = vec!["-n".into(), "-m".into()];
        let Some(out) = quick(php, cwd, &args) else {
            return ext;
        };
        let text = String::from_utf8_lossy(&out);
        if !text.contains("[PHP Modules]") {
            return ext;
        }
        ext.known = true;
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('[') {
                continue;
            }
            let l = l.to_ascii_lowercase();
            if l == "zend opcache" {
                ext.builtin.insert("opcache".into());
            }
            ext.builtin.insert(l);
        }
        let code = r#"foreach (glob(ini_get("extension_dir") . "/*.so") ?: [] as $f) { echo basename($f, ".so"), "\n"; }"#;
        let args: Vec<OsString> = vec!["-n".into(), "-r".into(), code.into()];
        if let Some(out) = quick(php, cwd, &args) {
            for line in String::from_utf8_lossy(&out).lines() {
                let l = line.trim();
                if !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    ext.shared.insert(l.to_ascii_lowercase());
                }
            }
        }
        ext
    }

    pub fn action(&self, name: &str) -> ExtAction {
        let lower = name.to_ascii_lowercase();
        if !self.known || self.builtin.contains(&lower) {
            ExtAction::Present
        } else if self.shared.contains(&lower) {
            if ZEND_EXTENSIONS.contains(&lower.as_str()) {
                ExtAction::Load(format!("zend_extension={name}"))
            } else {
                ExtAction::Load(format!("extension={name}"))
            }
        } else {
            ExtAction::Missing
        }
    }
}

fn quick(php: &Path, cwd: &Path, args: &[OsString]) -> Option<Vec<u8>> {
    let out = run(&Run {
        program: php,
        args,
        cwd,
        env: &[],
        stdin: None,
        capture: Capture::ALL,
        timeout: Duration::from_secs(60),
        mem_limit: None,
    })
    .ok()?;
    (out.exit == Exit::Code(0)).then_some(out.output)
}
