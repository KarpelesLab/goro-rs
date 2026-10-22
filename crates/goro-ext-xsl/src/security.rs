//! Security preferences (security.c) as configured by
//! XSLTProcessor::setSecurityPrefs(): a set bit forbids the operation.

use crate::transform::{
    SEC_CREATE_DIRECTORY, SEC_READ_FILE, SEC_READ_NETWORK, SEC_WRITE_FILE, SEC_WRITE_NETWORK,
    Transform,
};

/// xmlParserGetDirectory
fn parser_get_directory(path: &str) -> Option<String> {
    match path.rfind('/') {
        None => Some(".".to_string()),
        Some(0) => Some("/".to_string()),
        Some(i) => Some(path[..i].to_string()),
    }
}

/// xsltCheckFilename: 0 missing, 1 file, 2 directory
fn check_filename(path: &str) -> i32 {
    match std::fs::metadata(path) {
        Err(_) => 0,
        Ok(m) => {
            if m.is_dir() {
                2
            } else {
                1
            }
        }
    }
}

/// xsltCheckWritePath
fn check_write_path(t: &mut Transform, path: &str) -> i32 {
    if t.sec_prefs & SEC_WRITE_FILE != 0 {
        t.error(None, &format!("File write for {} refused\n", path));
        return 0;
    }
    if let Some(dir) = parser_get_directory(path) {
        let mut ret = check_filename(&dir);
        if ret == 0 {
            if t.sec_prefs & SEC_CREATE_DIRECTORY != 0 {
                t.error(None, &format!("Directory creation for {} refused\n", path));
                return 0;
            }
            ret = check_write_path(t, &dir);
            if ret == 1 {
                ret = if std::fs::create_dir(&dir).is_ok() {
                    0
                } else {
                    -1
                };
            }
        }
        if ret < 0 {
            return ret;
        }
    }
    1
}

/// xsltCheckWrite
pub fn check_write(t: &mut Transform, url: &str) -> i32 {
    let uri = goro_ext_xml::uri::parse_uri(url);
    let (scheme, path) = match &uri {
        Some(u) => (u.scheme.clone(), u.path.clone()),
        None => (None, url.to_string()),
    };
    if scheme.is_none() || scheme.as_deref() == Some("file") {
        let r = check_write_path(t, &path);
        if r <= 0 {
            return r;
        }
    } else if t.sec_prefs & SEC_WRITE_NETWORK != 0 {
        t.error(None, &format!("File write for {} refused\n", url));
        return 0;
    }
    1
}

/// xsltCheckRead
pub fn check_read(t: &mut Transform, url: &str) -> i32 {
    if !url.contains("://") {
        if t.sec_prefs & SEC_READ_FILE != 0 {
            t.error(None, &format!("Local file read for {} refused\n", url));
            return 0;
        }
        return 1;
    }
    let uri = match goro_ext_xml::uri::parse_uri(url) {
        Some(u) => u,
        None => {
            t.error(
                None,
                &format!("xsltCheckRead: URL parsing failed for {}\n", url),
            );
            return -1;
        }
    };
    if uri.scheme.is_none() || uri.scheme.as_deref() == Some("file") {
        if t.sec_prefs & SEC_READ_FILE != 0 {
            t.error(None, &format!("Local file read for {} refused\n", url));
            return 0;
        }
    } else if t.sec_prefs & SEC_READ_NETWORK != 0 {
        t.error(None, &format!("Network file read for {} refused\n", url));
        return 0;
    }
    1
}
