---
name: goro-rs project overview
description: PHP 8.5.4 implementation in Rust - architecture, goals, and current state
type: project
---

goro-rs is a PHP 8.5.4 implementation in Rust, targeting full compatibility with PHP's official test suite.

**Why:** Enable running PHP in secure, sandboxed environments with Rust's safety guarantees. Allow selective compilation of SAPIs and extensions.

**How to apply:** When implementing features, follow PHP 8.5.4 semantics exactly. Use the PHPT test suite as the correctness benchmark. Minimize external dependencies. All file access goes through the VFS trait.

**Current state (as of 2026-03-15):**
- ~230 official PHP tests passing (lang 48/213, Zend 137/871, ext/standard ~35/400)
- Workspace: goro-parser, goro-core, goro-vfs, goro-sapi, goro-ext-standard, goro-phpt
- Pipeline: source → lexer → parser → AST → compiler → bytecodes → VM
- Working: variables, arrays, objects, classes with inheritance, closures, exceptions, static props, default params, 400+ built-in functions
- Missing: full interface/trait support, references, include/require, proper error messages

**Remote:** github.com:KarpelesLab/goro-rs.git, branch: master
**PHP test suite:** /tmp/php-8.5-tests/ (cloned from php-src)
**Test runner:** run bash loops extracting --FILE-- and --EXPECT-- sections
