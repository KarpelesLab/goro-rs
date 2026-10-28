//! Parser and processing errors in the shape PHP surfaces them
//! (`LibXMLError`: level, code, column, message, file, line).
//!
//! Codes, levels, messages and positions follow what PHP 8.5 reports with
//! libxml2 2.15 (observed on PHP 8.5; the fixtures in `tests/fixtures`).
//! The numeric codes are libxml2's documented `xmlParserErrors` values and
//! domains its `xmlErrorDomain` values (libxml2 API documentation).

/// `LIBXML_ERR_WARNING`
pub const LEVEL_WARNING: u8 = 1;
/// `LIBXML_ERR_ERROR`
pub const LEVEL_ERROR: u8 = 2;
/// `LIBXML_ERR_FATAL`
pub const LEVEL_FATAL: u8 = 3;

/// Error domains (libxml2 `xmlErrorDomain`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Domain {
    /// XML_FROM_PARSER
    Parser = 1,
    /// XML_FROM_TREE
    Tree = 2,
    /// XML_FROM_NAMESPACE
    Namespace = 3,
    /// XML_FROM_DTD
    Dtd = 4,
    /// XML_FROM_HTML
    Html = 5,
    /// XML_FROM_OUTPUT
    Output = 7,
    /// XML_FROM_IO
    Io = 8,
    /// XML_FROM_XINCLUDE
    XInclude = 11,
    /// XML_FROM_XPATH
    XPath = 12,
    /// XML_FROM_C14N
    C14n = 21,
    /// XML_FROM_VALID
    Valid = 23,
}

/// One error or warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    /// Where it comes from.
    pub domain: Domain,
    /// The libxml error code (`LibXMLError::$code`).
    pub code: u32,
    /// 1 warning, 2 error, 3 fatal.
    pub level: u8,
    /// The message, ending with a newline as libxml formats it.
    pub message: String,
    /// The file (document URL), if known.
    pub file: Option<String>,
    /// 1-based line (0 when not applicable).
    pub line: u32,
    /// 1-based column in characters (0 when not applicable).
    pub column: u32,
}

impl XmlError {
    /// The message without its trailing newline (what PHP's warnings print).
    pub fn text(&self) -> &str {
        self.message.strip_suffix('\n').unwrap_or(&self.message)
    }

    /// The warning text PHP prints for a DOM load (`DOMDocument::loadXML()`
    /// and friends): "msg in Entity, line: N" for strings, "msg in FILE,
    /// line: N" for files (observed on PHP 8.5).
    pub fn dom_warning(&self) -> String {
        let file = self.file.as_deref().unwrap_or("Entity");
        if self.line == 0 && self.file.is_none() {
            return self.text().to_string();
        }
        format!("{} in {}, line: {}", self.text(), file, self.line)
    }
}

/// Collected errors of one operation.
#[derive(Clone, Debug, Default)]
pub struct Errors {
    /// In order of occurrence.
    pub list: Vec<XmlError>,
}

impl Errors {
    /// True if any error is fatal (the document is not well-formed).
    pub fn has_fatal(&self) -> bool {
        self.list.iter().any(|e| e.level == LEVEL_FATAL)
    }
}
