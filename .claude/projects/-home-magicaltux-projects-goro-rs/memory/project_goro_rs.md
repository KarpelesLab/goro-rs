---
name: goro-rs project overview
description: PHP 8.5.4 implementation in Rust - architecture, goals, and current state
type: project
---

goro-rs is a PHP 8.5.4 implementation in Rust, targeting full compatibility with PHP's official test suite.

**Why:** Enable running PHP in secure, sandboxed environments with Rust's safety guarantees. Allow selective compilation of SAPIs and extensions.

**How to apply:** When implementing features, follow PHP 8.5.4 semantics exactly. Use the PHPT test suite as the correctness benchmark. Minimize external dependencies. All file access goes through the VFS trait.

Key design:
- Workspace with crates: goro-parser, goro-core, goro-vfs, goro-sapi, goro-ext-standard, goro-phpt
- Pipeline: source → lexer (tokens) → parser (AST) → compiler (bytecodes) → VM (execution)
- Value type is a Rust enum (not heap-allocated for simple types)
- Arrays are ordered hash maps (PhpArray) with packed optimization planned
- Functions registered via `vm.register_function(name, fn_ptr)`
- Remote: github.com:KarpelesLab/goro-rs.git, branch: master
