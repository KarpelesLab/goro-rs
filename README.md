# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Early development. Core language features and basic OOP work.

**Test Suite Progress** (PHP 8.5.4 official tests):

| Test Directory | Pass | Total | Rate |
|---|---|---|---|
| tests/lang | 42 | 213 | 19.7% |
| Zend/tests (500) | 75 | 500 | 15.0% |
| **Sampled Total** | **117** | **713** | **16.4%** |

## Features

### Working
- Full execution pipeline: PHP source -> lexer -> parser -> AST -> bytecode -> VM
- Types: null, bool, int, float, string (binary-safe), array (ordered hash map), object
- Arithmetic, comparison, bitwise, logical operators with short-circuit evaluation
- String interpolation, concatenation, type juggling
- Control flow: if/elseif/else, while, do-while, for, foreach, switch, match, break N/continue N
- User-defined functions with parameters, return values, recursion
- Static variables in functions
- Global variables (`global $var`)
- 200+ built-in functions (string, array, math, type, output, JSON, date, etc.)
- `var_dump`, `print_r`, `var_export` with PHP-compatible formatting
- PHP-compatible float formatting (14 significant digits / serialize_precision=-1)
- CLI SAPI (`-r` code execution, file execution)
- PHPT test runner
- Virtual filesystem abstraction for security sandboxing

### Not Yet Implemented
- Full OOP (inheritance, interfaces, traits, enums, magic methods)
- Exceptions (try/catch/throw with proper error types)
- Closures and arrow functions
- References (`&`)
- include/require
- Namespaces
- Many extensions (pcre, pdo, curl, etc.)

## Building

```bash
cargo build --release
```

## Usage

```bash
# Run a PHP file
./target/release/goro script.php

# Run inline PHP code
./target/release/goro -r 'echo "Hello, World!\n";'

# Run PHPT tests
./target/release/goro --test /path/to/tests/
```

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the detailed design document.

Workspace crates:
- `goro-parser` - Hand-written lexer and recursive descent parser
- `goro-core` - Bytecode compiler and register-based VM
- `goro-vfs` - Virtual filesystem abstraction
- `goro-sapi` - Server API trait and CLI implementation
- `goro-ext-standard` - PHP standard library functions
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Minimal dependencies**: No external crates (everything hand-written)
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
