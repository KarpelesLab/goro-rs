//! The fonts of the system (what `-list font` / MagickQueryFonts() list):
//! TrueType / OpenType / Type 1 files found in the usual font directories,
//! named after their file names.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn scan(dir: &Path, out: &mut Vec<(String, PathBuf)>, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            scan(&p, out, depth + 1);
            continue;
        }
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "pfb" | "pfa") {
            continue;
        }
        if let Some(stem) = p.file_stem() {
            out.push((stem.to_string_lossy().into_owned(), p.clone()));
        }
    }
}

/// The fonts, sorted by name (name, file).
pub fn list() -> &'static [(String, PathBuf)] {
    static FONTS: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut out = Vec::new();
        let mut dirs = vec![
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join(".fonts"));
            dirs.push(home.join(".local/share/fonts"));
        }
        for d in dirs {
            scan(&d, &mut out, 0);
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        out
    })
}

/// The file of a font name, or of a font file path.
pub fn resolve(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    let l = list();
    l.binary_search_by(|e| e.0.as_str().cmp(name))
        .ok()
        .map(|i| l[i].1.clone())
        .or_else(|| {
            l.iter()
                .find(|e| e.0.eq_ignore_ascii_case(name))
                .map(|e| e.1.clone())
        })
}
