//! The configuration options: names, types, defaults and one-line help.
//!
//! Option names and the values the tests show come from the ext/tidy tests;
//! the rest of the list and every description except `ncr` (test 021.phpt)
//! are written from general knowledge of Tidy's documented options.

use std::fmt::Write;

/// How an option's value is typed and parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// no, yes or auto, stored as 0, 1 or 2 (test 030.phpt shows
    /// `indent => true` reading back as `int(1)`).
    AutoBool,
    Int,
    /// One of the names, stored as its index.
    Pick(&'static [&'static str]),
    /// A character encoding name, stored as [`Encoding`]'s number.
    Encoding,
    Str,
    /// Tag names separated by commas or spaces.
    TagList,
    /// `doctype`: a keyword or a quoted formal public identifier.
    Doctype,
}

#[derive(Clone, Copy, Debug)]
pub enum Def {
    B(bool),
    I(i64),
    S(&'static str),
}

/// One option's description.
pub struct Opt {
    pub name: &'static str,
    pub kind: Kind,
    pub default: Def,
    pub doc: &'static str,
    pub readonly: bool,
}

/// An option's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    Bool(bool),
    Int(i64),
    Str(String),
}

/// Character encodings, numbered as `input-encoding` reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Raw = 0,
    Ascii = 1,
    Latin0 = 2,
    Latin1 = 3,
    Utf8 = 4,
    Iso2022 = 5,
    Mac = 6,
    Win1252 = 7,
    Ibm858 = 8,
    Utf16Le = 9,
    Utf16Be = 10,
    Utf16 = 11,
    Big5 = 12,
    ShiftJis = 13,
}

const ENCODINGS: [(&str, Encoding); 15] = [
    ("raw", Encoding::Raw),
    ("ascii", Encoding::Ascii),
    ("latin0", Encoding::Latin0),
    ("latin1", Encoding::Latin1),
    ("utf8", Encoding::Utf8),
    ("iso2022", Encoding::Iso2022),
    ("mac", Encoding::Mac),
    ("win1252", Encoding::Win1252),
    ("ibm858", Encoding::Ibm858),
    ("utf16le", Encoding::Utf16Le),
    ("utf16be", Encoding::Utf16Be),
    ("utf16", Encoding::Utf16),
    ("big5", Encoding::Big5),
    ("shiftjis", Encoding::ShiftJis),
    ("utf-8", Encoding::Utf8),
];

impl Encoding {
    pub fn from_name(name: &str) -> Option<Encoding> {
        let l = name.trim().to_ascii_lowercase();
        ENCODINGS.iter().find(|(n, _)| *n == l).map(|(_, e)| *e)
    }

    pub fn from_int(i: i64) -> Encoding {
        ENCODINGS
            .iter()
            .find(|(_, e)| *e as i64 == i)
            .map_or(Encoding::Utf8, |(_, e)| *e)
    }

    /// The name in an XML declaration.
    pub fn iana(self) -> &'static str {
        match self {
            Encoding::Raw | Encoding::Iso2022 => "",
            Encoding::Ascii => "us-ascii",
            Encoding::Latin0 => "iso-8859-15",
            Encoding::Latin1 => "iso-8859-1",
            Encoding::Utf8 => "utf-8",
            Encoding::Mac => "macintosh",
            Encoding::Win1252 => "windows-1252",
            Encoding::Ibm858 => "ibm00858",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Utf16Be => "utf-16be",
            Encoding::Utf16 => "utf-16",
            Encoding::Big5 => "big5",
            Encoding::ShiftJis => "shift_jis",
        }
    }
}

const NO_YES_PRESERVE: &[&str] = &["no", "yes", "preserve"];
const NO_YES_AUTO: &[&str] = &["no", "yes", "auto"];
const CUSTOM_TAGS: &[&str] = &["no", "blocklevel", "empty", "inline", "pre"];
const NEWLINES: &[&str] = &["lf", "crlf", "cr"];
const REPEATED: &[&str] = &["keep-first", "keep-last"];
const SORT: &[&str] = &["none", "alpha"];
const ACCESS: &[&str] = &["0", "1", "2", "3"];
const DOCTYPE_MODES: &[&str] = &["html5", "omit", "auto", "strict", "loose", "user"];

macro_rules! opt {
    ($n:literal, $k:expr, $d:expr, $doc:literal) => {
        Opt {
            name: $n,
            kind: $k,
            default: $d,
            doc: $doc,
            readonly: false,
        }
    };
}

use Def::{B, I, S};
use Kind::{AutoBool, Bool, Doctype, Int, Pick, Str, TagList};

/// Every option, sorted by name (what `tidy::getConfig()` returns).
#[rustfmt::skip]
pub static OPTIONS: &[Opt] = &[
    opt!("accessibility-check", Pick(ACCESS), I(0), "This option specifies what level of accessibility checks should run: 0 for none, up to 3 for all of them."),
    opt!("add-meta-charset", Bool, B(false), "This option specifies if Tidy should add a meta element declaring the output encoding."),
    opt!("add-xml-decl", Bool, B(false), "This option specifies if Tidy should add the XML declaration when outputting XML or XHTML."),
    opt!("add-xml-space", Bool, B(false), "This option specifies if Tidy should add xml:space=\"preserve\" to elements such as pre, style and script."),
    opt!("alt-text", Str, S(""), "This option specifies the default alt text Tidy gives to img elements lacking one."),
    opt!("anchor-as-name", Bool, B(true), "This option controls the deletion or addition of the name attribute in elements where it can serve as an anchor."),
    opt!("ascii-chars", Bool, B(false), "This option specifies if the typographic characters are kept as they are when cleaning."),
    opt!("assume-xml-procins", Bool, B(false), "This option specifies if Tidy should change the parsing of processing instructions to require ?> as the terminator."),
    opt!("bare", Bool, B(false), "This option specifies if Tidy should strip Microsoft specific HTML from Word 2000 documents."),
    opt!("break-before-br", Bool, B(false), "This option specifies if Tidy should output a line break before each br element."),
    opt!("char-encoding", Kind::Encoding, I(4), "This option specifies the character encoding used for both the input and the output."),
    opt!("clean", Bool, B(false), "This option specifies if Tidy should strip out surplus presentational tags and attributes replacing them by style rules."),
    opt!("coerce-endtags", Bool, B(true), "This option specifies if Tidy should coerce a start tag into an end tag where an end tag was probably intended."),
    opt!("css-prefix", Str, S("c"), "This option specifies the prefix Tidy uses for the style rules it creates."),
    opt!("custom-tags", Pick(CUSTOM_TAGS), I(0), "This option enables autonomous custom tags, and says what content model they have: blocklevel, empty, inline or pre."),
    opt!("decorate-inferred-ul", Bool, B(false), "This option specifies if Tidy should decorate inferred ul elements with some CSS markup to avoid indentation to the right."),
    opt!("doctype", Doctype, S("auto"), "This option specifies the DOCTYPE declaration generated by Tidy: html5, omit, auto, strict, loose, or a quoted formal public identifier."),
    Opt { name: "doctype-mode", kind: Pick(DOCTYPE_MODES), default: I(2), doc: "This read-only option shows the current DOCTYPE mode.", readonly: true },
    opt!("drop-empty-elements", Bool, B(true), "This option specifies if Tidy should discard empty elements."),
    opt!("drop-empty-paras", Bool, B(true), "This option specifies if Tidy should discard empty paragraphs."),
    opt!("drop-font-tags", Bool, B(false), "This option specifies if Tidy should discard font and center tags without creating the corresponding style rules."),
    opt!("drop-proprietary-attributes", Bool, B(false), "This option specifies if Tidy should strip out proprietary attributes."),
    opt!("enclose-block-text", Bool, B(false), "This option specifies if Tidy should insert a p element to enclose text found in elements allowing mixed content."),
    opt!("enclose-text", Bool, B(false), "This option specifies if Tidy should enclose any text found in the body element within a p element."),
    opt!("error-file", Str, S(""), "This option specifies the error file Tidy uses for errors and warnings."),
    opt!("escape-cdata", Bool, B(false), "This option specifies if Tidy should convert CDATA sections to normal text."),
    opt!("escape-scripts", Bool, B(true), "This option specifies if Tidy should escape the end tag look-alikes in script contents."),
    opt!("fix-backslash", Bool, B(true), "This option specifies if Tidy should replace backslash characters in URLs by forward slashes."),
    opt!("fix-bad-comments", AutoBool, I(2), "This option specifies if Tidy should replace unexpected hyphens with = characters when it comes across adjacent hyphens."),
    opt!("fix-style-tags", Bool, B(true), "This option specifies if Tidy should move all style tags to the head of the document."),
    opt!("fix-uri", Bool, B(true), "This option specifies if Tidy should check attribute values that carry URIs for illegal characters and escape them."),
    opt!("force-output", Bool, B(false), "This option specifies if Tidy should produce output even if errors are encountered."),
    opt!("gdoc", Bool, B(false), "This option specifies if Tidy should enable specific behavior for cleaning up HTML exported from Google Docs."),
    opt!("gnu-emacs", Bool, B(false), "This option specifies if Tidy should change the format for reporting errors and warnings to one GNU Emacs can parse."),
    opt!("gnu-emacs-file", Str, S(""), "This internal option holds the file name used with gnu-emacs."),
    opt!("hide-comments", Bool, B(false), "This option specifies if Tidy should print out comments."),
    opt!("hide-endtags", Bool, B(false), "This option is an alias for omit-optional-tags."),
    opt!("indent", AutoBool, I(0), "This option specifies if Tidy should indent block-level tags: no, yes, or auto to indent only where needed."),
    opt!("indent-attributes", Bool, B(false), "This option specifies if Tidy should begin each attribute on a new line."),
    opt!("indent-cdata", Bool, B(false), "This option specifies if Tidy should indent CDATA sections."),
    opt!("indent-spaces", Int, I(2), "This option specifies the number of spaces or tabs that Tidy uses to indent content."),
    opt!("indent-with-tabs", Bool, B(false), "This option specifies if Tidy should indent with tabs instead of spaces."),
    opt!("input-encoding", Kind::Encoding, I(4), "This option specifies the character encoding Tidy uses for the input."),
    opt!("input-xml", Bool, B(false), "This option specifies if Tidy should use the XML parser rather than the error correcting HTML parser."),
    opt!("join-classes", Bool, B(false), "This option specifies if Tidy should combine class names to generate a single new class name if multiple class assignments are detected on an element."),
    opt!("join-styles", Bool, B(true), "This option specifies if Tidy should combine styles to generate a single new style if multiple style values are detected on an element."),
    opt!("keep-tabs", Bool, B(false), "This option specifies if Tidy should keep tabs in the input instead of replacing them by spaces."),
    opt!("keep-time", Bool, B(false), "This option specifies if Tidy should keep the original modification time of files it writes back."),
    opt!("language", Str, S(""), "This option specifies the language Tidy uses for its messages."),
    opt!("literal-attributes", Bool, B(false), "This option specifies how Tidy deals with whitespace characters within attribute values."),
    opt!("logical-emphasis", Bool, B(false), "This option specifies if Tidy should replace any occurrence of i with em and any occurrence of b with strong."),
    opt!("lower-literals", Bool, B(true), "This option specifies if Tidy should convert the value of an attribute that takes a list of predefined values to lower case."),
    opt!("markup", Bool, B(true), "This option specifies if Tidy should generate a pretty printed version of the markup."),
    opt!("merge-divs", AutoBool, I(2), "This option specifies if Tidy should merge nested div elements such as <div><div>...</div></div>, used with the clean option."),
    opt!("merge-emphasis", Bool, B(true), "This option specifies if Tidy should merge nested b and i elements."),
    opt!("merge-spans", AutoBool, I(2), "This option specifies if Tidy should merge nested span elements such as <span><span>...</span></span>, used with the clean option."),
    opt!("mute", Str, S(""), "This option lists the message keys Tidy should not output."),
    opt!("mute-id", Bool, B(false), "This option specifies if Tidy should display message keys with its messages."),
    opt!("ncr", Bool, B(true), "This option specifies if Tidy should allow numeric character references. "),
    opt!("new-blocklevel-tags", TagList, S(""), "This option specifies new block-level tags. This option takes a space or comma separated list of tag names."),
    opt!("new-empty-tags", TagList, S(""), "This option specifies new empty inline tags. This option takes a space or comma separated list of tag names."),
    opt!("new-inline-tags", TagList, S(""), "This option specifies new non-empty inline tags. This option takes a space or comma separated list of tag names."),
    opt!("new-pre-tags", TagList, S(""), "This option specifies new tags that are to be processed in exactly the same way as the pre element."),
    opt!("newline", Pick(NEWLINES), I(0), "The default is appropriate to the current platform: LF, CRLF or CR."),
    opt!("numeric-entities", Bool, B(false), "This option specifies if Tidy should output entities other than the built-in HTML entities in the numeric rather than the named form."),
    opt!("omit-optional-tags", Bool, B(false), "This option specifies if Tidy should omit optional start tags and end tags when generating output."),
    opt!("output-bom", AutoBool, I(2), "This option specifies if Tidy should write a Unicode Byte Order Mark character to the beginning of the output."),
    opt!("output-encoding", Kind::Encoding, I(4), "This option specifies the character encoding Tidy uses for the output."),
    opt!("output-file", Str, S(""), "This option specifies the output file Tidy uses for markup."),
    opt!("output-html", Bool, B(false), "This option specifies if Tidy should generate pretty printed output, writing it as HTML."),
    opt!("output-xhtml", Bool, B(false), "This option specifies if Tidy should generate pretty printed output, writing it as extensible HTML."),
    opt!("output-xml", Bool, B(false), "This option specifies if Tidy should pretty print output, writing it as well-formed XML."),
    opt!("preserve-entities", Bool, B(false), "This option specifies if Tidy should preserve well-formed entities as found in the input."),
    opt!("priority-attributes", TagList, S(""), "This option lists the attributes to output before all the others."),
    opt!("punctuation-wrap", Bool, B(false), "This option specifies if Tidy should line wrap after some Unicode or Chinese punctuation characters."),
    opt!("quiet", Bool, B(false), "This option specifies if Tidy should output the summary of the numbers of errors and warnings, or the welcome or informational messages."),
    opt!("quote-ampersand", Bool, B(true), "This option specifies if Tidy should output unadorned & characters as &amp;."),
    opt!("quote-marks", Bool, B(false), "This option specifies if Tidy should output \" characters as &quot; in text."),
    opt!("quote-nbsp", Bool, B(true), "This option specifies if Tidy should output non-breaking space characters as entities, rather than as the character itself."),
    opt!("repeated-attributes", Pick(REPEATED), I(1), "This option specifies if Tidy should keep the first or last attribute, if an attribute is repeated, e.g. has two align attributes."),
    opt!("replace-color", Bool, B(false), "This option specifies if Tidy should replace numeric values in color attributes with HTML/XHTML color names where defined."),
    opt!("show-body-only", AutoBool, I(0), "This option specifies if Tidy should print only the contents of the body tag as an HTML fragment."),
    opt!("show-errors", Int, I(6), "This option specifies the number of errors Tidy shows; use 0 to hide all errors."),
    opt!("show-filename", Bool, B(false), "This option specifies if Tidy should show the filename in messages."),
    opt!("show-info", Bool, B(true), "This option specifies if Tidy should display info-level messages."),
    opt!("show-meta-change", Bool, B(false), "This option specifies if Tidy should show when it changes a meta element declaring the encoding."),
    opt!("show-warnings", Bool, B(true), "This option specifies if Tidy should suppress warnings."),
    opt!("skip-nested", Bool, B(true), "This option specifies that Tidy should skip nested tags when parsing script and style data."),
    opt!("sort-attributes", Pick(SORT), I(0), "This option specifies that Tidy should sort attributes within an element using the specified sort algorithm: none or alpha."),
    opt!("strict-tags-attributes", Bool, B(false), "This option specifies if Tidy should ensure tags and attributes are valid for the document's version of HTML."),
    opt!("tab-size", Int, I(8), "This option specifies the number of columns that Tidy uses between successive tab stops."),
    opt!("tidy-mark", Bool, B(true), "This option specifies if Tidy should add a meta element to the document head to indicate that the document has been tidied."),
    opt!("uppercase-attributes", Pick(NO_YES_PRESERVE), I(0), "This option specifies if Tidy should output attribute names in upper case."),
    opt!("uppercase-tags", Bool, B(false), "This option specifies if Tidy should output tag names in upper case."),
    opt!("vertical-space", Pick(NO_YES_AUTO), I(0), "This option specifies if Tidy should add some extra empty lines for readability."),
    opt!("warn-proprietary-attributes", Bool, B(true), "This option specifies if Tidy should warn about proprietary attributes."),
    opt!("word-2000", Bool, B(false), "This option specifies if Tidy should go to great pains to strip out all the surplus stuff Microsoft Word inserts when saving as Web pages."),
    opt!("wrap", Int, I(68), "This option specifies the right margin Tidy uses for line wrapping. Tidy tries to wrap lines so that they do not exceed this length. Set wrap to 0 to disable line wrapping."),
    opt!("wrap-asp", Bool, B(true), "This option specifies if Tidy should line wrap text contained within ASP pseudo elements."),
    opt!("wrap-attributes", Bool, B(false), "This option specifies if Tidy should line-wrap attribute values."),
    opt!("wrap-jste", Bool, B(true), "This option specifies if Tidy should line wrap text contained within JSTE pseudo elements."),
    opt!("wrap-php", Bool, B(false), "This option specifies if Tidy should line wrap text contained within PHP pseudo elements."),
    opt!("wrap-script-literals", Bool, B(false), "This option specifies if Tidy should line wrap string literals that appear in script attributes."),
    opt!("wrap-sections", Bool, B(true), "This option specifies if Tidy should line wrap text contained within <![ ... ]> section tags."),
    opt!("write-back", Bool, B(false), "This option specifies if Tidy should write back the tidied markup to the same file it read from."),
];

/// The option's index in [`OPTIONS`].
pub fn find_option(name: &str) -> Option<usize> {
    OPTIONS.iter().position(|o| o.name == name)
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "t" | "true" | "1" => Some(true),
        "n" | "no" | "f" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// A set of option values.
#[derive(Clone, Debug)]
pub struct Config {
    vals: Vec<Val>,
    /// Problems met while reading configuration files.
    pub errors: u32,
    /// Their messages, for the error buffer.
    pub messages: String,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Config {
        let vals = OPTIONS
            .iter()
            .map(|o| match o.default {
                B(b) => Val::Bool(b),
                I(i) => Val::Int(i),
                S(s) => Val::Str(s.to_string()),
            })
            .collect();
        Config {
            vals,
            errors: 0,
            messages: String::new(),
        }
    }

    pub fn value(&self, i: usize) -> &Val {
        &self.vals[i]
    }

    fn idx(name: &str) -> usize {
        find_option(name).unwrap_or_else(|| panic!("unknown tidy option {name}"))
    }

    pub fn bool(&self, name: &str) -> bool {
        matches!(self.vals[Self::idx(name)], Val::Bool(true))
    }

    pub fn int(&self, name: &str) -> i64 {
        match self.vals[Self::idx(name)] {
            Val::Int(i) => i,
            Val::Bool(b) => b as i64,
            Val::Str(_) => 0,
        }
    }

    pub fn str(&self, name: &str) -> &str {
        match &self.vals[Self::idx(name)] {
            Val::Str(s) => s,
            _ => "",
        }
    }

    pub fn encoding(&self, name: &str) -> Encoding {
        Encoding::from_int(self.int(name))
    }

    /// Sets the option to a value of its storage type, as is.
    pub fn set_raw(&mut self, i: usize, v: Val) {
        self.vals[i] = v;
        if OPTIONS[i].name == "char-encoding" {
            let v = self.vals[i].clone();
            self.vals[Self::idx("input-encoding")] = v.clone();
            self.vals[Self::idx("output-encoding")] = v;
        }
    }

    pub fn set_bool(&mut self, name: &str, v: bool) {
        self.set_raw(Self::idx(name), Val::Bool(v));
    }

    /// Sets the option from its text form; `false` when the text isn't a
    /// value of the option.
    pub fn set_parsed(&mut self, i: usize, text: &str) -> bool {
        let t = text.trim();
        let v = match OPTIONS[i].kind {
            Kind::Bool => parse_bool(t).map(Val::Bool),
            Kind::AutoBool => {
                if t.eq_ignore_ascii_case("auto") {
                    Some(Val::Int(2))
                } else {
                    parse_bool(t).map(|b| Val::Int(b as i64))
                }
            }
            Kind::Int => t.parse::<i64>().ok().map(Val::Int),
            Kind::Pick(names) => names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(t))
                .map(|p| Val::Int(p as i64)),
            Kind::Encoding => Encoding::from_name(t).map(|e| Val::Int(e as i64)),
            Kind::Str => Some(Val::Str(t.trim_matches('"').to_string())),
            Kind::TagList => Some(Val::Str(
                t.split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
            Kind::Doctype => {
                let l = t.to_ascii_lowercase();
                let keyword = [
                    "html5",
                    "omit",
                    "auto",
                    "strict",
                    "loose",
                    "transitional",
                    "user",
                ]
                .contains(&l.as_str());
                if keyword {
                    Some(Val::Str(l))
                } else if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                    Some(Val::Str(t.to_string()))
                } else {
                    None
                }
            }
        };
        match v {
            Some(v) => {
                self.set_raw(i, v);
                true
            }
            None => false,
        }
    }

    /// Reads a configuration file's text: `name: value` lines, comments
    /// starting with `#` or `//`, and values continued on indented lines
    /// (test 016.tcfg shows the `name: value` form). Problems are counted
    /// in [`Config::errors`] (test gh10636.phpt).
    pub fn load_text(&mut self, text: &str) {
        let mut entries: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
                continue;
            }
            if line.starts_with([' ', '\t'])
                && let Some(last) = entries.last_mut()
            {
                last.1.push(' ');
                last.1.push_str(t);
                continue;
            }
            match t.split_once(':') {
                Some((n, v)) => entries.push((n.trim().to_string(), v.trim().to_string())),
                None => {
                    self.errors += 1;
                    let _ = writeln!(self.messages, "Warning: unknown option: {t}");
                }
            }
        }
        for (n, v) in entries {
            match find_option(&n) {
                None => {
                    self.errors += 1;
                    let _ = writeln!(self.messages, "Warning: unknown option: {n}");
                }
                Some(i) if OPTIONS[i].readonly || !self.set_parsed(i, &v) => {
                    self.errors += 1;
                    let _ = writeln!(
                        self.messages,
                        "Warning: invalid value for option {n}: {v}"
                    );
                }
                Some(_) => {}
            }
        }
    }

    /// The tags listed in a `new-*-tags` option.
    pub fn tag_list(&self, name: &str) -> Vec<String> {
        self.str(name)
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
            .collect()
    }
}
