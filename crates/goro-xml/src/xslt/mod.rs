//! XSLT 1.0 (W3C Recommendation, 16 November 1999) on goro-xml's tree and
//! XPath engine, for PHP's `XSLTProcessor`, with the EXSLT modules
//! (exslt.org) common, dates-and-times, functions, math, sets and strings.
//!
//! A stylesheet is compiled ([`compile::compile`]) into a [`Stylesheet`]
//! independent of the tree it was read from; [`transform::transform`] runs
//! it on a source document in the arena and builds a result document there;
//! [`output::serialize`] writes a result as `xsl:output` asks.
//!
//! Everything PHP-specific goes through [`Host`]: diagnostics, resource
//! loading, extension functions and security decisions.

pub mod compile;
mod exslt;
pub mod format;
pub mod output;
pub mod transform;

pub use compile::{OutputSpec, Stylesheet};

use crate::tree::{Arena, NodeId};
use crate::xpath::{Value, XPathError};

/// The XSLT namespace.
pub const XSL_NS: &str = "http://www.w3.org/1999/XSL/Transform";

/// What the transformation engine needs from its embedder.
pub trait Host {
    /// Reports one diagnostic line (PHP: a warning).
    fn report(&mut self, arena: &mut Arena, message: &str);

    /// The absolute form of `href` against `base`.
    fn resolve(&mut self, href: &str, base: Option<&str>) -> String;

    /// Parses the resource at `url` (already resolved) into a new document
    /// of the arena; `None` after reporting why not. `stylesheet` tells
    /// `xsl:include`/`xsl:import` from `document()`.
    fn load(&mut self, arena: &mut Arena, url: &str, stylesheet: bool) -> Option<NodeId>;

    /// Why reading `url` is refused by the security preferences.
    fn read_refusal(&mut self, url: &str) -> Option<String>;

    /// Why writing `url` is refused by the security preferences.
    fn write_refusal(&mut self, url: &str) -> Option<String>;

    /// Writes an `xsl:document` result; false on failure.
    fn write(&mut self, arena: &mut Arena, url: &str, data: &[u8]) -> bool;

    /// Whether `{ns}local` is a function of the embedder.
    fn has_function(&self, ns: &str, local: &str) -> bool;

    /// Calls a function of the embedder.
    fn call_function(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        args: Vec<Value>,
    ) -> Result<Value, XPathError>;

    /// True when the embedder wants the transformation abandoned (a PHP
    /// exception is pending).
    fn stop_requested(&self) -> bool;
}
