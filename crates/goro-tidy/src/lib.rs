//! A pure Rust port of tidy-html5 5.8.0 (libtidy), the HTML repair and
//! pretty-printing library behind PHP's ext/tidy.
//!
//! The modules follow tidy's source files; see `doc.rs` for the porting
//! conventions.

#![allow(clippy::too_many_arguments)]

pub mod access;
pub mod attrdefs;
pub mod attrdict;
pub mod attrs;
pub mod clean;
pub mod config;
pub mod doc;
pub mod entities;
pub mod entitydefs;
pub mod enums;
pub mod gdoc;
pub mod istack;
pub mod lexer;
pub mod message;
pub mod parser;
pub mod pprint;
pub mod streamio;
pub mod tagdefs;
pub mod tags;
pub mod tidylib;
pub mod tmbstr;
pub mod utf8;

pub use doc::{AttId, Doc, Node, NodeId, NodeType};
pub use enums::{AttrId, OptId, TagId};
