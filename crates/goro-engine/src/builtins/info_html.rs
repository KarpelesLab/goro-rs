//! phpinfo() and phpcredits() as web SAPIs print them: an HTML page (PHP
//! manual: phpinfo, phpcredits; tests
//! ext/standard/tests/general_functions/phpinfo2.phpt,
//! phpinfo-header-anchors.phpt, phpcredits2.phpt). The content is the
//! plain-text form's, in tables; module headings are anchors named
//! `module_<name>`.

use goro_runtime::{PhpStr, Value};

use super::info_text;
use crate::vm::{R, Vm};

const STYLE: &str = "body {background-color: #fff; color: #222; font-family: sans-serif;}
pre {margin: 0; font-family: monospace;}
a:link {color: #009; text-decoration: none; background-color: #fff;}
a:hover {text-decoration: underline;}
table {border-collapse: collapse; border: 0; width: 934px; box-shadow: 1px 2px 3px rgba(0, 0, 0, 0.2);}
.center {text-align: center;}
.center table {margin: 1em auto; text-align: left;}
.center th {text-align: center !important;}
td, th {border: 1px solid #666; font-size: 75%; vertical-align: baseline; padding: 4px 5px;}
th {position: sticky; top: 0; background: inherit;}
h1 {font-size: 150%;}
h2 {font-size: 125%;}
h2 a:link, h2 a:visited{color: inherit; background: inherit;}
.p {text-align: left;}
.e {background-color: #ccf; width: 300px; font-weight: bold;}
.h {background-color: #99c; font-weight: bold;}
.v {background-color: #ddd; max-width: 300px; overflow-x: auto; word-wrap: break-word;}
.v i {color: #999;}
img {float: right; border: 0;}
hr {width: 934px; background-color: #ccc; border: 0; height: 1px;}
:root {--php-dark-grey: #333; --php-dark-blue: #4F5B93; --php-medium-blue: #8892BF; --php-light-blue: #E2E4EF; --php-accent-purple: #793862}";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#039;"),
            c => o.push(c),
        }
    }
    o
}

/// The page's start, up to the centered body.
pub(super) fn head(title: &str) -> String {
    format!(
        "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"DTD/xhtml1-transitional.dtd\">\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\n\
         <style type=\"text/css\">\n{STYLE}\n</style>\n\
         <title>{}</title><meta name=\"ROBOTS\" content=\"NOINDEX,NOFOLLOW,NOARCHIVE\" /></head>\n\
         <body><div class=\"center\">\n",
        esc(title)
    )
}

pub(super) const FOOT: &str = "</div></body></html>";

/// Table rows from `a => b` lines (the plain-text form's).
pub(super) struct Table {
    out: String,
}

impl Table {
    pub fn new() -> Table {
        Table {
            out: String::from("<table>\n"),
        }
    }

    pub fn header(&mut self, cells: &[&str]) {
        self.out.push_str("<tr class=\"h\">");
        for c in cells {
            self.out.push_str(&format!("<th>{}</th>", esc(c)));
        }
        self.out.push_str("</tr>\n");
    }

    pub fn row(&mut self, cells: &[&str]) {
        self.out.push_str("<tr>");
        for (i, c) in cells.iter().enumerate() {
            let class = if i == 0 { "e" } else { "v" };
            let v = if *c == "no value" {
                "<i>no value</i>".to_string()
            } else {
                esc(c)
            };
            self.out
                .push_str(&format!("<td class=\"{class}\">{v} </td>"));
        }
        self.out.push_str("</tr>\n");
    }

    /// `k => v` lines; a line without `=>` is a row of one cell.
    pub fn lines(&mut self, text: &str) {
        for l in text.lines() {
            if l.trim().is_empty() {
                continue;
            }
            let cells: Vec<&str> = l.split(" => ").collect();
            self.row(&cells);
        }
    }

    pub fn finish(mut self) -> String {
        self.out.push_str("</table>\n");
        self.out
    }
}

/// A module's heading, an anchor named after it.
pub(super) fn module_heading(name: &str) -> String {
    let id: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c == ' ' { '_' } else { c })
        .collect();
    format!(
        "<h2><a name=\"module_{}\" href=\"#module_{}\">{}</a></h2>\n",
        esc(&id),
        esc(&id),
        esc(name)
    )
}

/// The credits sections the flags select, as tables.
pub(super) fn credits_body(flags: i64) -> String {
    let mut out = String::from("<h1>PHP Credits</h1>\n");
    for (f, text) in info_text::SECTIONS {
        if flags & f == 0 {
            continue;
        }
        let mut lines = text.lines();
        let Some(title) = lines.next() else { continue };
        let rest: Vec<&str> = lines.collect();
        let cols = if rest.iter().any(|l| l.contains(" => ")) {
            2
        } else {
            1
        };
        let mut t = Table::new();
        if cols == 2 {
            t.out.push_str(&format!(
                "<tr class=\"h\"><th colspan=\"2\">{}</th></tr>\n",
                esc(title.trim())
            ));
        } else {
            t.header(&[title.trim()]);
        }
        for (i, l) in rest.iter().enumerate() {
            let cells: Vec<&str> = l.split(" => ").collect();
            if i == 0 && matches!(cells.first(), Some(&"Contribution") | Some(&"Module")) {
                t.header(&cells);
            } else {
                t.row(&cells);
            }
        }
        out.push_str(&t.finish());
    }
    out
}

/// phpcredits() under a web SAPI.
pub(super) fn credits_page(flags: i64) -> String {
    let body = credits_body(flags);
    if flags & 32 != 0 {
        format!("{}{body}{FOOT}\n", head("PHP Credits"))
    } else {
        body
    }
}

/// The license, as phpinfo(INFO_LICENSE) shows it.
pub(super) fn license() -> String {
    let mut out = String::from("<h2>PHP License</h2>\n<table>\n<tr class=\"v\"><td>\n");
    for para in info_text::LICENSE.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        out.push_str(&format!("<p>\n{}\n</p>\n", esc(p)));
    }
    out.push_str("</td></tr>\n</table>\n");
    out
}

/// A superglobal's entries as `$_NAME['key']` rows.
pub(super) fn variables(vm: &mut Vm, t: &mut Table) -> R<()> {
    for g in [
        "_REQUEST", "_GET", "_POST", "_COOKIE", "_FILES", "_SERVER", "_ENV",
    ] {
        let Some(p) = vm.global_var_ptr(&PhpStr::from(g), false) else {
            continue;
        };
        // SAFETY: a live global slot, read once.
        let v = unsafe { (*p).deref() };
        let Value::Array(arr) = v else { continue };
        for (k, v) in arr.iter() {
            let k = match k {
                goro_runtime::KeyRef::Int(i) => i.to_string(),
                goro_runtime::KeyRef::Str(s) => s.to_string_lossy(),
            };
            let v = v.deref();
            let mut b = Vec::new();
            if matches!(v, Value::Array(_)) {
                b.extend_from_slice(b"<pre>");
                let mut inner = Vec::new();
                vm.print_r_into(&mut inner, &v, 0)?;
                b.extend_from_slice(esc(&String::from_utf8_lossy(&inner)).as_bytes());
                b.extend_from_slice(b"</pre>");
                t.out.push_str(&format!(
                    "<tr><td class=\"e\">${g}['{}']</td><td class=\"v\">{}</td></tr>\n",
                    esc(&k),
                    String::from_utf8_lossy(&b)
                ));
            } else {
                vm.append_to_string(&mut b, &v)?;
                let name = format!("${g}['{k}']");
                t.row(&[&name, &String::from_utf8_lossy(&b)]);
            }
        }
    }
    Ok(())
}
