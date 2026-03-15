---
name: goro-rs project overview
description: PHP 8.5.4 implementation in Rust - architecture, goals, and current state
type: project
---

goro-rs is a PHP 8.5.4 implementation in Rust.

**Current state (2026-03-16):**
- 209/1084 core tests passing (19.3%): lang 50/213, Zend 159/871
- Zend(500) sample: 111/500 (22.2%)
- ~260 total across all test suites

**Key features:** Complete pipeline (lexer→parser→AST→compiler→bytecodes→VM),
PHP references, OOP with inheritance, exceptions, closures with capture,
list/[] destructuring, 450+ built-in functions with callback support,
function/class hoisting, include/require, define/defined.

**Remote:** github.com:KarpelesLab/goro-rs.git, branch: master
