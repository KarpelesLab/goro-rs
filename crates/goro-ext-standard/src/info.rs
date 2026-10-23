//! phpinfo() and phpcredits() (ext/standard/info.c, credits.c), in the
//! text format of the CLI and the HTML format of a web SAPI.

use std::cell::Cell;

use goro_core::array::ArrayKey;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub const PHP_INFO_GENERAL: i64 = 1;
pub const PHP_INFO_CREDITS: i64 = 2;
pub const PHP_INFO_CONFIGURATION: i64 = 4;
pub const PHP_INFO_MODULES: i64 = 8;
pub const PHP_INFO_ENVIRONMENT: i64 = 16;
pub const PHP_INFO_VARIABLES: i64 = 32;
pub const PHP_INFO_LICENSE: i64 = 64;

pub const PHP_CREDITS_GROUP: i64 = 1;
pub const PHP_CREDITS_GENERAL: i64 = 2;
pub const PHP_CREDITS_SAPI: i64 = 4;
pub const PHP_CREDITS_MODULES: i64 = 8;
pub const PHP_CREDITS_DOCS: i64 = 16;
pub const PHP_CREDITS_FULLPAGE: i64 = 32;
pub const PHP_CREDITS_QA: i64 = 64;
pub const PHP_CREDITS_WEB: i64 = 128;

const ZEND_VERSION: &str = "4.5.4";
const PHP_API_VERSION: &str = "20250925";
const ZEND_MODULE_API_NO: &str = "20250925";
const ZEND_EXTENSION_API_NO: &str = "420250925";

thread_local! {
    /// The request runs under a web SAPI (CGI emulation): HTML output.
    static WEB_SAPI: Cell<bool> = const { Cell::new(false) };
}

/// Set by the SAPI at request startup (sapi_module.phpinfo_as_text is
/// false for web SAPIs).
pub fn set_web_sapi(web: bool) {
    WEB_SAPI.with(|w| w.set(web));
}

/// Whether the request is served by a web SAPI (cgi-fcgi) rather than the CLI.
pub fn is_web_sapi() -> bool {
    WEB_SAPI.with(|w| w.get())
}

struct Info {
    html: bool,
    out: Vec<u8>,
}

fn html_esc(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'&' => o.extend_from_slice(b"&amp;"),
            b'<' => o.extend_from_slice(b"&lt;"),
            b'>' => o.extend_from_slice(b"&gt;"),
            b'"' => o.extend_from_slice(b"&quot;"),
            b'\'' => o.extend_from_slice(b"&#039;"),
            _ => o.push(c),
        }
    }
    o
}

impl Info {
    fn new() -> Self {
        Info {
            html: WEB_SAPI.with(|w| w.get()),
            out: Vec::new(),
        }
    }

    fn print(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn print_bytes(&mut self, s: &[u8]) {
        self.out.extend_from_slice(s);
    }

    fn print_esc(&mut self, s: &[u8]) {
        let e = html_esc(s);
        self.out.extend_from_slice(&e);
    }

    fn table_start(&mut self) {
        self.print(if self.html { "<table>\n" } else { "\n" });
    }

    fn table_end(&mut self) {
        if self.html {
            self.print("</table>\n");
        }
    }

    fn box_start(&mut self, flag: bool) {
        self.table_start();
        if flag {
            if self.html {
                self.print("<tr class=\"h\"><td>\n");
            }
        } else if self.html {
            self.print("<tr class=\"v\"><td>\n");
        } else {
            self.print("\n");
        }
    }

    fn box_end(&mut self) {
        if self.html {
            self.print("</td></tr>\n");
        }
        self.table_end();
    }

    fn hr(&mut self) {
        if self.html {
            self.print("<hr />\n");
        } else {
            self.print(
                "\n\n _______________________________________________________________________\n\n",
            );
        }
    }

    fn colspan_header(&mut self, cols: usize, header: &str) {
        if self.html {
            self.print(&format!(
                "<tr class=\"h\"><th colspan=\"{}\">{}</th></tr>\n",
                cols, header
            ));
        } else {
            let spaces = 74usize.saturating_sub(header.len()) / 2;
            let pad = " ".repeat(spaces.max(1));
            self.print(&format!("{}{}{}\n", pad, header, pad));
        }
    }

    fn header(&mut self, cols: &[&str]) {
        if self.html {
            self.print("<tr class=\"h\">");
        }
        for (i, c) in cols.iter().enumerate() {
            let c = if c.is_empty() { " " } else { c };
            if self.html {
                self.print("<th>");
                self.print(c);
                self.print("</th>");
            } else {
                self.print(c);
                self.print(if i + 1 < cols.len() { " => " } else { "\n" });
            }
        }
        if self.html {
            self.print("</tr>\n");
        }
    }

    fn row(&mut self, cols: &[&[u8]]) {
        if self.html {
            self.print("<tr>");
        }
        for (i, c) in cols.iter().enumerate() {
            if self.html {
                self.print(&format!(
                    "<td class=\"{}\">",
                    if i == 0 { "e" } else { "v" }
                ));
            }
            if c.is_empty() {
                self.print(if self.html { "<i>no value</i>" } else { " " });
            } else if self.html {
                self.print_esc(c);
            } else {
                self.print_bytes(c);
                if i + 1 < cols.len() {
                    self.print(" => ");
                }
            }
            if self.html {
                self.print(" </td>");
            } else if i + 1 == cols.len() {
                self.print("\n");
            }
        }
        if self.html {
            self.print("</tr>\n");
        }
    }

    fn row2(&mut self, a: &str, b: &str) {
        self.row(&[a.as_bytes(), b.as_bytes()]);
    }

    fn section(&mut self, name: &str) {
        if self.html {
            self.print(&format!("<h2>{}</h2>\n", name));
        } else {
            self.table_start();
            self.header(&[name]);
            self.table_end();
        }
    }

    fn htmlhead(&mut self, version: &str) {
        self.print("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"DTD/xhtml1-transitional.dtd\">\n");
        self.print("<html xmlns=\"http://www.w3.org/1999/xhtml\">");
        self.print("<head>\n");
        self.print("<style type=\"text/css\">\n");
        self.print(CSS);
        self.print("</style>\n");
        self.print(&format!("<title>PHP {} - phpinfo()</title>", version));
        self.print("<meta name=\"ROBOTS\" content=\"NOINDEX,NOFOLLOW,NOARCHIVE\" />");
        self.print("</head>\n");
        self.print("<body><div class=\"center\">\n");
    }

    /// Text produced by an extension's phpinfo hook (in the text format),
    /// rendered in the current format.
    fn hook_text(&mut self, text: &str) {
        if !self.html {
            self.print(text);
            return;
        }
        let mut in_table = false;
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            match line.split_once(" => ") {
                Some(_) => {
                    if !in_table {
                        self.print("<table>\n");
                        in_table = true;
                    }
                    let cols: Vec<&[u8]> = line.split(" => ").map(|c| c.as_bytes()).collect();
                    self.row(&cols);
                }
                None => {
                    if in_table {
                        self.print("</table>\n");
                        in_table = false;
                    }
                    self.module_heading(line);
                }
            }
        }
        if in_table {
            self.print("</table>\n");
        }
    }

    fn module_heading(&mut self, name: &str) {
        if self.html {
            let url: String = name
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                        (b.to_ascii_lowercase() as char).to_string()
                    } else if b == b' ' {
                        "+".to_string()
                    } else {
                        format!("%{:02X}", b)
                    }
                })
                .collect();
            self.print(&format!(
                "<h2><a name=\"module_{}\" href=\"#module_{}\">{}</a></h2>\n",
                url, url, name
            ));
        } else {
            self.table_start();
            self.header(&[name]);
            self.table_end();
        }
    }
}

const CSS: &str = "body {background-color: #fff; color: #222; font-family: sans-serif;}
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
";

fn call_str_fn(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    f(vm, args).ok()
}

fn list_fn(vm: &mut Vm, name: &[u8]) -> Vec<Vec<u8>> {
    match call_str_fn(vm, name, &[]) {
        Some(Value::Array(a)) => a
            .borrow()
            .values()
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .collect(),
        _ => Vec::new(),
    }
}

fn print_stream_hash(info: &mut Info, name: &str, items: &[Vec<u8>]) {
    if items.is_empty() {
        info.row2(&format!("Registered {}", name), "none registered");
        return;
    }
    if info.html {
        info.print(&format!(
            "<tr><td class=\"e\">Registered {}</td><td class=\"v\">",
            name
        ));
    } else {
        info.print(&format!("\nRegistered {} => ", name));
    }
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            info.print(", ");
        }
        if info.html {
            info.print_esc(it);
        } else {
            info.print_bytes(it);
        }
    }
    if info.html {
        info.print("</td></tr>\n");
    }
}

/// Display name of a loaded extension (zend_module_entry::name).
fn module_display_name(lower: &str) -> String {
    match lower {
        "core" => "Core".into(),
        "spl" => "SPL".into(),
        "reflection" => "Reflection".into(),
        "simplexml" => "SimpleXML".into(),
        "pdo" => "PDO".into(),
        "phar" => "Phar".into(),
        "zend opcache" => "Zend OPcache".into(),
        other => other.to_string(),
    }
}

/// Split phpinfo hook output into (lowercase module name, body) chunks.
fn split_hook_sections(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let is_heading = !l.is_empty()
            && !l.contains(" => ")
            && i > 0
            && lines[i - 1].is_empty()
            && lines.get(i + 1).is_some_and(|n| n.is_empty());
        if is_heading {
            out.push((l.to_ascii_lowercase(), String::new()));
            i += 1;
            continue;
        }
        if let Some(last) = out.last_mut() {
            last.1.push_str(l);
            last.1.push('\n');
        }
        i += 1;
    }
    for c in out.iter_mut() {
        // Each body starts with the blank line of its table and must not
        // carry the blank line that precedes the next heading.
        while c.1.ends_with("\n\n") {
            c.1.pop();
        }
    }
    out
}

fn print_gpcse_array(vm: &mut Vm, info: &mut Info, name: &str) {
    let Some(Value::Array(arr)) = vm.global_lookup(name.as_bytes()).map(|v| v.deref()) else {
        return;
    };
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in entries {
        if info.html {
            info.print("<tr><td class=\"e\">");
        }
        info.print("$");
        info.print(name);
        info.print("['");
        match &k {
            ArrayKey::String(s) => {
                if info.html {
                    info.print_esc(s.as_bytes());
                } else {
                    info.print_bytes(s.as_bytes());
                }
            }
            ArrayKey::Int(n) => info.print(&n.to_string()),
        }
        info.print("']");
        info.print(if info.html {
            "</td><td class=\"v\">"
        } else {
            " => "
        });
        let v = v.deref();
        if matches!(v, Value::Array(_)) {
            let s = call_str_fn(vm, b"print_r", &[v.clone(), Value::True])
                .map(|r| r.to_php_string().as_bytes().to_vec())
                .unwrap_or_default();
            if info.html {
                info.print("<pre>");
                info.print_esc(&s);
                info.print("</pre>");
            } else {
                info.print_bytes(&s);
            }
        } else {
            let s = v.to_php_string().as_bytes().to_vec();
            if info.html {
                if s.is_empty() {
                    info.print("<i>no value</i>");
                } else {
                    info.print_esc(&s);
                }
            } else {
                info.print_bytes(&s);
            }
        }
        info.print(if info.html { "</td></tr>\n" } else { "\n" });
    }
}

fn credit(info: &mut Info, module: &str, authors: &str) {
    info.row2(module, authors);
}

/// php_print_credits()
fn print_credits(info: &mut Info, flag: i64, version: &str) {
    if info.html && flag & PHP_CREDITS_FULLPAGE != 0 {
        info.htmlhead(version);
    }
    info.print(if info.html {
        "<h1>PHP Credits</h1>\n"
    } else {
        "PHP Credits\n"
    });
    if flag & PHP_CREDITS_GROUP != 0 {
        info.table_start();
        info.header(&["PHP Group"]);
        info.row(&[b"Thies C. Arntzen, Stig Bakken, Shane Caraveo, Andi Gutmans, Rasmus Lerdorf, Sam Ruby, Sascha Schumann, Zeev Suraski, Jim Winstead, Andrei Zmievski"]);
        info.table_end();
    }
    if flag & PHP_CREDITS_GENERAL != 0 {
        info.table_start();
        if info.html {
            info.header(&["Language Design &amp; Concept"]);
        } else {
            info.header(&["Language Design & Concept"]);
        }
        info.row(&[b"Andi Gutmans, Rasmus Lerdorf, Zeev Suraski, Marcus Boerger"]);
        info.table_end();

        info.table_start();
        info.colspan_header(2, "PHP Authors");
        info.header(&["Contribution", "Authors"]);
        for (m, a) in CREDITS_GENERAL {
            credit(info, m, a);
        }
        info.table_end();
    }
    if flag & PHP_CREDITS_SAPI != 0 {
        info.table_start();
        info.colspan_header(2, "SAPI Modules");
        info.header(&["Contribution", "Authors"]);
        for (m, a) in CREDITS_SAPI {
            credit(info, m, a);
        }
        info.table_end();
    }
    if flag & PHP_CREDITS_MODULES != 0 {
        info.table_start();
        info.colspan_header(2, "Module Authors");
        info.header(&["Module", "Authors"]);
        for (m, a) in CREDITS_EXT {
            credit(info, m, a);
        }
        info.table_end();
    }
    if flag & PHP_CREDITS_DOCS != 0 {
        info.table_start();
        info.colspan_header(2, "PHP Documentation");
        credit(
            info,
            "Authors",
            "Mehdi Achour, Friedhelm Betz, Antony Dovgal, Nuno Lopes, Hannes Magnusson, Philip Olson, Georg Richter, Damien Seguy, Jakub Vrana, Adam Harvey",
        );
        credit(info, "Editor", "Peter Cowburn");
        credit(
            info,
            "User Note Maintainers",
            "Daniel P. Brown, Thiago Henrique Pojda",
        );
        credit(
            info,
            "Other Contributors",
            "Previously active authors, editors and other contributors are listed in the manual.",
        );
        info.table_end();
    }
    if flag & PHP_CREDITS_QA != 0 {
        info.table_start();
        info.header(&["PHP Quality Assurance Team"]);
        info.row(&[b"Ilia Alshanetsky, Joerg Behrens, Antony Dovgal, Stefan Esser, Moriyoshi Koizumi, Magnus Maatta, Sebastian Nohn, Derick Rethans, Melvyn Sopacua, Pierre-Alain Joye, Dmitry Stogov, Felipe Pena, David Soria Parra, Stanislav Malyshev, Julien Pauli, Stephen Zarkos, Anatol Belski, Remi Collet, Ferenc Kovacs"]);
        info.table_end();
    }
    if flag & PHP_CREDITS_WEB != 0 {
        info.table_start();
        info.colspan_header(2, "Websites and Infrastructure team");
        credit(
            info,
            "PHP Websites Team",
            "Rasmus Lerdorf, Hannes Magnusson, Philip Olson, Lukas Kahwe Smith, Pierre-Alain Joye, Kalle Sommer Nielsen, Peter Cowburn, Adam Harvey, Ferenc Kovacs, Levi Morrison",
        );
        credit(info, "Event Maintainers", "Damien Seguy, Daniel P. Brown");
        credit(info, "Network Infrastructure", "Daniel P. Brown");
        credit(info, "Windows Infrastructure", "Alex Schoenmaker");
        info.table_end();
    }
    if info.html && flag & PHP_CREDITS_FULLPAGE != 0 {
        info.print("</div></body></html>\n");
    }
}

const CREDITS_GENERAL: &[(&str, &str)] = &[
    (
        "Zend Scripting Language Engine",
        "Andi Gutmans, Zeev Suraski, Stanislav Malyshev, Marcus Boerger, Dmitry Stogov, Xinchen Hui, Nikita Popov",
    ),
    (
        "Extension Module API",
        "Andi Gutmans, Zeev Suraski, Andrei Zmievski",
    ),
    (
        "UNIX Build and Modularization",
        "Stig Bakken, Sascha Schumann, Jani Taskinen, Peter Kokot",
    ),
    (
        "Windows Support",
        "Shane Caraveo, Zeev Suraski, Wez Furlong, Pierre-Alain Joye, Anatol Belski, Kalle Sommer Nielsen",
    ),
    (
        "Server API (SAPI) Abstraction Layer",
        "Andi Gutmans, Shane Caraveo, Zeev Suraski",
    ),
    ("Streams Abstraction Layer", "Wez Furlong, Sara Golemon"),
    (
        "PHP Data Objects Layer",
        "Wez Furlong, Marcus Boerger, Sterling Hughes, George Schlossnagle, Ilia Alshanetsky",
    ),
    (
        "Output Handler",
        "Zeev Suraski, Thies C. Arntzen, Marcus Boerger, Michael Wallner",
    ),
    (
        "Consistent 64 bit support",
        "Anthony Ferrara, Anatol Belski",
    ),
];

const CREDITS_SAPI: &[(&str, &str)] = &[
    (
        "Apache 2 Handler",
        "Ian Holsman, Justin Erenkrantz (based on Apache 2 Filter code)",
    ),
    (
        "CGI / FastCGI",
        "Rasmus Lerdorf, Stig Bakken, Shane Caraveo, Dmitry Stogov",
    ),
    (
        "CLI",
        "Edin Kadribasic, Marcus Boerger, Johannes Schlueter, Moriyoshi Koizumi, Xinchen Hui",
    ),
    ("Embed", "Edin Kadribasic"),
    (
        "FastCGI Process Manager",
        "Andrei Nigmatulin, dreamcat4, Antony Dovgal, Jerome Loyet",
    ),
    ("litespeed", "George Wang"),
    ("phpdbg", "Felipe Pena, Joe Watkins, Bob Weinand"),
];

const CREDITS_EXT: &[(&str, &str)] = &[
    ("BC Math", "Andi Gutmans"),
    ("Bzip2", "Sterling Hughes"),
    (
        "Calendar",
        "Shane Caraveo, Colin Viebrock, Hartmut Holzgraefe, Wez Furlong",
    ),
    ("COM and .Net", "Wez Furlong"),
    ("ctype", "Hartmut Holzgraefe"),
    ("cURL", "Sterling Hughes"),
    ("Date/Time Support", "Derick Rethans"),
    (
        "DB-LIB (MS SQL, Sybase)",
        "Wez Furlong, Frank M. Kromann, Adam Baratz",
    ),
    ("DBA", "Sascha Schumann, Marcus Boerger"),
    (
        "DOM",
        "Christian Stocker, Rob Richards, Marcus Boerger, Niels Dossche",
    ),
    ("enchant", "Pierre-Alain Joye, Ilia Alshanetsky"),
    ("EXIF", "Rasmus Lerdorf, Marcus Boerger"),
    ("FFI", "Dmitry Stogov"),
    (
        "fileinfo",
        "Ilia Alshanetsky, Pierre Alain Joye, Scott MacVicar, Derick Rethans, Anatol Belski",
    ),
    ("Firebird driver for PDO", "Ard Biesheuvel"),
    ("FTP", "Stefan Esser, Andrew Skalski"),
    (
        "GD imaging",
        "Rasmus Lerdorf, Stig Bakken, Jim Winstead, Jouni Ahto, Ilia Alshanetsky, Pierre-Alain Joye, Marcus Boerger, Mark Randall",
    ),
    ("GetText", "Alex Plotnick"),
    ("GNU GMP support", "Stanislav Malyshev"),
    ("Iconv", "Rui Hirokawa, Stig Bakken, Moriyoshi Koizumi"),
    (
        "Input Filter",
        "Rasmus Lerdorf, Derick Rethans, Pierre-Alain Joye, Ilia Alshanetsky",
    ),
    (
        "Internationalization",
        "Ed Batutis, Vladimir Iordanov, Dmitry Lakhtyuk, Stanislav Malyshev, Vadim Savchuk, Kirti Velankar",
    ),
    ("JSON", "Jakub Zelenka, Omar Kilani, Scott MacVicar"),
    (
        "LDAP",
        "Amitay Isaacs, Eric Warnke, Rasmus Lerdorf, Gerrit Thomson, Stig Venaas",
    ),
    (
        "LIBXML",
        "Christian Stocker, Rob Richards, Marcus Boerger, Wez Furlong, Shane Caraveo",
    ),
    ("Multibyte String Functions", "Tsukada Takuya, Rui Hirokawa"),
    (
        "MySQL driver for PDO",
        "George Schlossnagle, Wez Furlong, Ilia Alshanetsky, Johannes Schlueter",
    ),
    (
        "MySQLi",
        "Zak Greant, Georg Richter, Andrey Hristov, Ulf Wendel",
    ),
    (
        "MySQLnd",
        "Andrey Hristov, Ulf Wendel, Georg Richter, Johannes Schlüter",
    ),
    ("ODBC driver for PDO", "Wez Furlong"),
    (
        "ODBC",
        "Stig Bakken, Andreas Karajannis, Frank M. Kromann, Daniel R. Kalowsky",
    ),
    (
        "Opcache",
        "Andi Gutmans, Zeev Suraski, Stanislav Malyshev, Dmitry Stogov, Xinchen Hui",
    ),
    (
        "OpenSSL",
        "Stig Venaas, Wez Furlong, Sascha Kettler, Scott MacVicar, Eliot Lear",
    ),
    ("pcntl", "Jason Greene, Arnaud Le Blanc"),
    ("Perl Compatible Regexps", "Andrei Zmievski"),
    ("PHP Archive", "Gregory Beaver, Marcus Boerger"),
    (
        "PHP Data Objects",
        "Wez Furlong, Marcus Boerger, Sterling Hughes, George Schlossnagle, Ilia Alshanetsky",
    ),
    (
        "PHP hash",
        "Sara Golemon, Rasmus Lerdorf, Stefan Esser, Michael Wallner, Scott MacVicar",
    ),
    ("Posix", "Kristian Koehntopp"),
    (
        "PostgreSQL driver for PDO",
        "Edin Kadribasic, Ilia Alshanetsky",
    ),
    (
        "PostgreSQL",
        "Jouni Ahto, Zeev Suraski, Yasuo Ohgaki, Chris Kings-Lynne",
    ),
    (
        "random",
        "Go Kudo, Tim Düsterhus, Guilliam Xavier, Christoph M. Becker, Jakub Zelenka, Bob Weinand, Máté Kocsis, and Original RNG implementators",
    ),
    ("Readline", "Thies C. Arntzen"),
    (
        "Reflection",
        "Marcus Boerger, Timm Friebe, George Schlossnagle, Andrei Zmievski, Johannes Schlueter",
    ),
    ("Sessions", "Sascha Schumann, Andrei Zmievski"),
    (
        "Shared Memory Operations",
        "Slava Poliakov, Ilia Alshanetsky",
    ),
    ("SimpleXML", "Sterling Hughes, Marcus Boerger, Rob Richards"),
    (
        "SNMP",
        "Rasmus Lerdorf, Harrie Hazewinkel, Mike Jackson, Steven Lawrance, Johann Hanne, Boris Lytochkin",
    ),
    ("SOAP", "Brad Lafountain, Shane Caraveo, Dmitry Stogov"),
    (
        "Sockets",
        "Chris Vandomelen, Sterling Hughes, Daniel Beulshausen, Jason Greene",
    ),
    ("Sodium", "Frank Denis"),
    ("SPL", "Marcus Boerger, Etienne Kneuss"),
    ("SQLite 3.x driver for PDO", "Wez Furlong"),
    ("SQLite3", "Scott MacVicar, Ilia Alshanetsky, Brad Dewar"),
    ("System V Message based IPC", "Wez Furlong"),
    ("System V Semaphores", "Tom May"),
    ("System V Shared Memory", "Christian Cartus"),
    ("tidy", "John Coggeshall, Ilia Alshanetsky"),
    ("tokenizer", "Andrei Zmievski, Johannes Schlueter"),
    (
        "uri",
        "Máté Kocsis, Tim Düsterhus, Ignace Nyamagana Butera, Arnaud Le Blanc, Dennis Snell, Niels Dossche, Nicolas Grekas",
    ),
    ("XML", "Stig Bakken, Thies C. Arntzen, Sterling Hughes"),
    ("XMLReader", "Rob Richards"),
    ("XMLWriter", "Rob Richards, Pierre-Alain Joye"),
    ("XSL", "Christian Stocker, Rob Richards"),
    ("Zip", "Pierre-Alain Joye, Remi Collet"),
    (
        "Zlib",
        "Rasmus Lerdorf, Stefan Roehrich, Zeev Suraski, Jade Nicoletti, Michael Wallner",
    ),
];

fn php_version(vm: &Vm) -> String {
    vm.constants
        .get(b"PHP_VERSION".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "8.5.4".to_string())
}

/// php_print_info()
fn print_info(vm: &mut Vm, info: &mut Info, flag: i64, module_hooks: &[(String, String)]) {
    let version = php_version(vm);
    if info.html {
        info.htmlhead(&version);
    } else {
        info.print("phpinfo()\n");
    }
    if flag & PHP_INFO_GENERAL != 0 {
        let uname = php_uname_a().into_bytes();
        if info.html {
            info.box_start(true);
            info.print(&format!("<h1 class=\"p\">PHP Version {}</h1>\n", version));
        } else {
            info.row2("PHP Version", &version);
        }
        info.box_end();
        info.table_start();
        info.row(&[b"System", &uname]);
        info.row2("Build Date", "Jan  1 2026 00:00:00");
        info.row2("Configure Command", " './configure' ");
        let sapi = vm
            .constants
            .get(b"PHP_SAPI".as_slice())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        let sapi_name = if sapi == "fpm-fcgi" {
            "FPM/FastCGI"
        } else if info.html {
            "CGI/FastCGI"
        } else if sapi == "cli-server" {
            "Built-in HTTP server"
        } else {
            "Command Line Interface"
        };
        info.row2("Server API", sapi_name);
        info.row2("Virtual Directory Support", "disabled");
        info.row2("Configuration File (php.ini) Path", "/usr/local/lib");
        info.row2("Loaded Configuration File", "(none)");
        info.row2("Scan this dir for additional .ini files", "(none)");
        info.row2("Additional .ini files parsed", "(none)");
        info.row2("PHP API", PHP_API_VERSION);
        info.row2("PHP Extension", ZEND_MODULE_API_NO);
        info.row2("Zend Extension", ZEND_EXTENSION_API_NO);
        info.row2(
            "Zend Extension Build",
            &format!("API{},NTS", ZEND_EXTENSION_API_NO),
        );
        info.row2(
            "PHP Extension Build",
            &format!("API{},NTS", ZEND_MODULE_API_NO),
        );
        info.row2("PHP Integer Size", "64 bits");
        info.row2("Debug Build", "no");
        info.row2("Thread Safety", "disabled");
        info.row2("Zend Signal Handling", "enabled");
        info.row2("Zend Memory Manager", "enabled");
        let mb = if vm.loaded_extensions.contains(b"mbstring".as_slice()) {
            "provided by mbstring"
        } else {
            "disabled"
        };
        info.row2("Zend Multibyte Support", mb);
        info.row2("Zend Max Execution Timers", "disabled");
        info.row2("IPv6 Support", "enabled");
        info.row2("DTrace Support", "disabled");
        let streams = list_fn(vm, b"stream_get_wrappers");
        let transports = list_fn(vm, b"stream_get_transports");
        let filters = list_fn(vm, b"stream_get_filters");
        print_stream_hash(info, "PHP Streams", &streams);
        print_stream_hash(info, "Stream Socket Transports", &transports);
        print_stream_hash(info, "Stream Filters", &filters);
        info.table_end();

        info.box_start(false);
        info.print("This program makes use of the Zend Scripting Language Engine:");
        info.print(if info.html { "<br />" } else { "\n" });
        info.print(&format!(
            "Zend Engine v{}, Copyright (c) Zend Technologies\n",
            ZEND_VERSION
        ));
        info.box_end();
    }

    if flag & PHP_INFO_CONFIGURATION != 0 {
        info.hr();
        if info.html {
            info.print("<h1>Configuration</h1>\n");
        } else {
            info.section("Configuration");
        }
        if flag & PHP_INFO_MODULES == 0 {
            info.section("PHP Core");
        }
    }

    if flag & PHP_INFO_MODULES != 0 {
        let mut modules: Vec<String> = vm
            .loaded_extensions
            .iter()
            .map(|m| String::from_utf8_lossy(m).into_owned())
            .collect();
        for m in module_hooks {
            if !modules.contains(&m.0) {
                modules.push(m.0.clone());
            }
        }
        modules.sort_by_key(|a| a.to_ascii_lowercase());
        for m in &modules {
            let name = module_display_name(m);
            match module_hooks.iter().find(|(n, _)| n == m) {
                Some((_, body)) => {
                    info.module_heading(&name);
                    info.hook_text(body);
                }
                None => {
                    info.module_heading(&name);
                    info.table_start();
                    if m == "core" {
                        info.row2("PHP Version", &version);
                        info.table_end();
                        core_ini_entries(vm, info);
                    } else {
                        info.row2("Version", &version);
                        info.table_end();
                    }
                }
            }
        }
        info.section("Additional Modules");
        info.table_start();
        info.header(&["Module Name"]);
        info.table_end();
    }

    if flag & PHP_INFO_ENVIRONMENT != 0 {
        use std::os::unix::ffi::OsStringExt;
        info.section("Environment");
        info.table_start();
        info.header(&["Variable", "Value"]);
        for (k, v) in std::env::vars_os() {
            let (k, v) = (k.into_vec(), v.into_vec());
            info.row(&[&k, &v]);
        }
        info.table_end();
    }

    if flag & PHP_INFO_VARIABLES != 0 {
        info.section("PHP Variables");
        info.table_start();
        info.header(&["Variable", "Value"]);
        for name in ["PHP_SELF", "PHP_AUTH_TYPE", "PHP_AUTH_USER", "PHP_AUTH_PW"] {
            if let Some(Value::String(s)) = vm.global_lookup(name.as_bytes()).map(|v| v.deref()) {
                info.row(&[name.as_bytes(), s.as_bytes()]);
            }
        }
        for name in [
            "_REQUEST", "_GET", "_POST", "_FILES", "_COOKIE", "_SERVER", "_ENV",
        ] {
            print_gpcse_array(vm, info, name);
        }
        info.table_end();
    }

    if flag & PHP_INFO_CREDITS != 0 {
        info.hr();
        print_credits(info, !PHP_CREDITS_FULLPAGE, &version);
    }

    if flag & PHP_INFO_LICENSE != 0 {
        if info.html {
            info.section("PHP License");
            info.box_start(false);
            info.print("<p>\n");
            info.print("This program is free software; you can redistribute it and/or modify ");
            info.print("it under the terms of the PHP License as published by the PHP Group ");
            info.print("and included in the distribution in the file:  LICENSE\n");
            info.print("</p>\n");
            info.print("<p>");
            info.print("This program is distributed in the hope that it will be useful, ");
            info.print("but WITHOUT ANY WARRANTY; without even the implied warranty of ");
            info.print("MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n");
            info.print("</p>\n");
            info.print("<p>");
            info.print(
                "If you did not receive a copy of the PHP license, or have any questions about ",
            );
            info.print("PHP licensing, please contact license@php.net.\n");
            info.print("</p>\n");
            info.box_end();
        } else {
            info.print("\nPHP License\n");
            info.print("This program is free software; you can redistribute it and/or modify\n");
            info.print("it under the terms of the PHP License as published by the PHP Group\n");
            info.print("and included in the distribution in the file:  LICENSE\n");
            info.print("\n");
            info.print("This program is distributed in the hope that it will be useful,\n");
            info.print("but WITHOUT ANY WARRANTY; without even the implied warranty of\n");
            info.print("MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n");
            info.print("\n");
            info.print("If you did not receive a copy of the PHP license, or have any\n");
            info.print("questions about PHP licensing, please contact license@php.net.\n");
        }
    }

    if info.html {
        info.print("</div></body></html>");
    }
}

/// php_get_uname('a')
fn php_uname_a() -> String {
    // SAFETY: uname() fills the zeroed struct; its fields are NUL-terminated.
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) == 0 {
            let f = |b: &[libc::c_char]| {
                std::ffi::CStr::from_ptr(b.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };
            format!(
                "{} {} {} {} {}",
                f(&u.sysname),
                f(&u.nodename),
                f(&u.release),
                f(&u.version),
                f(&u.machine)
            )
        } else {
            "Linux".to_string()
        }
    }
}

/// display_ini_entries() of the Core module.
fn core_ini_entries(vm: &Vm, info: &mut Info) {
    info.table_start();
    info.header(&["Directive", "Local Value", "Master Value"]);
    for (k, d) in [
        ("allow_url_fopen", "On"),
        ("allow_url_include", "Off"),
        ("arg_separator.input", "&"),
        ("arg_separator.output", "&"),
        ("auto_append_file", ""),
        ("auto_prepend_file", ""),
        ("default_charset", "UTF-8"),
        ("default_mimetype", "text/html"),
        ("disable_functions", ""),
        ("display_errors", "1"),
        ("display_startup_errors", "On"),
        ("doc_root", ""),
        ("enable_dl", "On"),
        ("error_log", ""),
        ("error_reporting", ""),
        ("extension_dir", "/usr/lib/php/extensions"),
        ("file_uploads", "On"),
        ("html_errors", "Off"),
        ("include_path", ".:/usr/share/php"),
        ("log_errors", "Off"),
        ("max_execution_time", "0"),
        ("max_input_time", "-1"),
        ("memory_limit", "128M"),
        ("open_basedir", ""),
        ("output_buffering", "0"),
        ("post_max_size", "8M"),
        ("short_open_tag", "Off"),
        ("upload_max_filesize", "2M"),
        ("upload_tmp_dir", ""),
        ("variables_order", "EGPCS"),
    ] {
        let v = vm
            .constants
            .get(k.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_else(|| d.to_string());
        let v = if v.is_empty() {
            "no value".to_string()
        } else {
            v
        };
        info.row(&[k.as_bytes(), v.as_bytes(), v.as_bytes()]);
    }
    info.table_end();
}

/// PHP_MINFO of the extensions implemented here (text format).
fn builtin_module_info(vm: &mut Vm) -> String {
    let mut out = String::new();
    if vm.loaded_extensions.contains(b"zlib".as_slice()) {
        // PHP_MINFO_FUNCTION(zlib)
        let ini = |k: &[u8], default: &str| {
            let v = vm
                .constants
                .get(k)
                .map(|v| v.to_php_string().to_string_lossy())
                .unwrap_or_else(|| default.to_string());
            if v.is_empty() {
                "no value".to_string()
            } else {
                v
            }
        };
        let oc = ini(b"zlib.output_compression", "0");
        let oc = if oc == "0" || oc.eq_ignore_ascii_case("off") {
            "Off".to_string()
        } else if oc == "1" || oc.eq_ignore_ascii_case("on") {
            "On".to_string()
        } else {
            oc
        };
        let lvl = ini(b"zlib.output_compression_level", "-1");
        let oh = ini(b"zlib.output_handler", "");
        out.push_str(&format!(
            "\nzlib\n\nZLib Support => enabled\nStream Wrapper => compress.zlib://\nStream Filter => zlib.inflate, zlib.deflate\nCompiled Version => {v}\nLinked Version => {v}\n\nDirective => Local Value => Master Value\nzlib.output_compression => {oc} => {oc}\nzlib.output_compression_level => {lvl} => {lvl}\nzlib.output_handler => {oh} => {oh}\n",
            v = goro_zlib::ZLIB_VERSION,
        ));
    }
    {
        // PHP_MINFO_FUNCTION(pcre)
        let ini = |k: &[u8], default: &str| {
            vm.constants
                .get(k)
                .map(|v| v.to_php_string().to_string_lossy())
                .unwrap_or_else(|| default.to_string())
        };
        let jit = ini(b"pcre.jit", "1");
        let jit = if matches!(
            jit.to_ascii_lowercase().as_str(),
            "1" | "on" | "yes" | "true"
        ) {
            "On"
        } else {
            "Off"
        };
        out.push_str(&format!(
            "\npcre\n\nPCRE (Perl Compatible Regular Expressions) Support => enabled\nPCRE Library Version => {}\nPCRE Unicode Version => {}\nPCRE JIT Support => enabled\nPCRE JIT Target => x86 64bit (little endian + unaligned)\n\nDirective => Local Value => Master Value\npcre.backtrack_limit => {bl} => {bl}\npcre.jit => {jit} => {jit}\npcre.recursion_limit => {rl} => {rl}\n",
            goro_pcre::VERSION,
            goro_pcre::UNICODE_VERSION,
            bl = ini(b"pcre.backtrack_limit", "1000000"),
            rl = ini(b"pcre.recursion_limit", "100000"),
        ));
    }
    out
}

/// PHP_FUNCTION(phpinfo)
pub fn phpinfo(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let flag = args.first().map(|v| v.to_long()).unwrap_or(0xFFFF_FFFF);
    let mut hooks_text = builtin_module_info(vm);
    if flag & PHP_INFO_MODULES != 0 {
        for hook in vm.phpinfo_hooks.clone() {
            hooks_text.push_str(&hook(vm));
        }
    }
    let sections = split_hook_sections(&hooks_text);
    let mut info = Info::new();
    print_info(vm, &mut info, flag, &sections);
    vm.write_output(&info.out);
    Ok(Value::True)
}

/// PHP_FUNCTION(phpcredits)
pub fn phpcredits(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let flag = args.first().map(|v| v.to_long()).unwrap_or(0xFFFF_FFFF);
    let version = php_version(vm);
    let mut info = Info::new();
    print_credits(&mut info, flag, &version);
    vm.write_output(&info.out);
    Ok(Value::True)
}
