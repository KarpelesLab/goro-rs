# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Early development. Core language features and basic OOP work.

**Test Suite Progress** (PHP 8.5.4 official tests):

| Test Directory | Pass | Total | Rate |
|---|---|---|---|
| tests/lang | 50 | 213 | 23.5% |
| tests/basic | 11 | 110 | 10.0% |
| Zend/tests | 167 | 871 | 19.2% |
| ext/standard (sampled) | ~40 | 400 | ~10.0% |
| **Total** | **~268** | **~1594** | **~16.8%** |

## Features

### Working
- Full execution pipeline: PHP source -> lexer -> parser -> AST -> bytecode -> VM
- Types: null, bool, int, float, string (binary-safe), array (ordered hash map), object
- Arithmetic, comparison, bitwise, logical operators with short-circuit evaluation
- String interpolation, concatenation, type juggling, cast operators
- Control flow: if/elseif/else, while, do-while, for, foreach, switch, match, break N/continue N
- Classes with properties, methods, constructors, inheritance, instanceof
- Closures (`function() { }`) and arrow functions (`fn() =>`)
- parent::method() calls with correct hierarchy resolution
- Magic constants: __CLASS__, __METHOD__, __FUNCTION__, __LINE__
- Exception handling: try/catch/finally with cross-function propagation
- Built-in Exception/Error classes with getMessage(), getCode()
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
- Full OOP (interfaces, traits, enums, abstract classes, magic methods)
- Full exception hierarchy (only basic Exception/Error supported)
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
