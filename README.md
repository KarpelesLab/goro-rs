# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development. Core language features, OOP, generators, exceptions, closures, and references work.

**Test Suite Progress** (PHP 8.5.4 official tests):

| Test Directory | Pass | Total | Rate |
|---|---|---|---|
| Zend/tests (top-level) | 181 | 871 | 20.8% |

*Test runner now supports recursive directories, SKIPIF sections, and timeout protection.*

## Features

### Working
- Full execution pipeline: PHP source -> lexer -> parser -> AST -> bytecode -> VM
- Types: null, bool, int, float, string (binary-safe), array (ordered hash map), object, reference
- Arithmetic, comparison, bitwise, logical operators with short-circuit evaluation
- String interpolation (`"$var"`, `"$obj->prop"`, `"$arr[key]"`), concatenation, type juggling, cast operators
- Control flow: if/elseif/else, while, do-while, for, foreach, switch, match, break N/continue N
- **OOP**: classes, inheritance, static properties/methods, class constants, instanceof, abstract classes
- **Magic methods**: `__construct`, `__toString`, `__get`, `__set`, `__call`, `__invoke`, `clone`
- **Closures** (`function() use ($x) { }`) with variable capture, arrow functions (`fn() =>`)
- **Generators**: `yield`, `yield $key => $value`, foreach over generators, Generator methods
- **References**: `$b = &$a`, shared value mutation through `Rc<RefCell<Value>>`
- **Exceptions**: try/catch/finally with cross-function propagation, built-in Exception/Error classes
- **Variadic parameters**: `function foo(...$args)`, `function bar($x, ...$rest)`
- **List destructuring**: `list($a, $b) = $arr`, `[$x, $y] = [1, 2]`
- parent::method() and self::method() with correct hierarchy resolution
- Magic constants: `__CLASS__`, `__METHOD__`, `__FUNCTION__`, `__LINE__`
- Function/class hoisting (forward references via two-pass compilation)
- include/require with runtime compilation
- define()/defined()/constant() with runtime constant table
- Static variables in functions, global variables (`global $var`)
- Low-precedence `and`/`or`/`xor` operators
- 450+ built-in functions with callback support:
  - Array: array_map/filter/reduce/walk/sort/usort/splice/shift/unshift/keys/values/merge/diff/intersect/chunk/combine/search/unique/reverse/flip etc.
  - String: strlen/strpos/substr/str_replace/explode/implode/trim/strtolower/strtoupper/sprintf (with arg positions)/addslashes/strtr/soundex/levenshtein etc.
  - Math: abs/ceil/floor/round/sin/cos/tan/log/sqrt/pow/pi/random_int etc.
  - Type: gettype/settype/is_*/intval/floatval/strval/boolval etc.
  - Output: echo/print/var_dump/print_r/var_export/printf with PHP-compatible formatting
  - JSON: json_encode (full), json_decode (stub)
  - OOP: get_class/get_parent_class/get_class_methods/get_object_vars/class_exists/method_exists/property_exists/interface_exists/spl_object_hash/spl_object_id
  - Misc: call_user_func/call_user_func_array/define/defined/error_reporting/ini_set/ini_get/serialize etc.
- PHP-compatible float formatting (14 significant digits, scientific notation for large/small values)
- Fatal error output to stdout in PHP format
- CLI SAPI (`-r` code execution, file execution)
- PHPT test runner with EXPECTF pattern matching
- Virtual filesystem abstraction for security sandboxing

### Not Yet Implemented
- Full interface/trait enforcement
- Enums (parsed but not enforced)
- `__destruct`, `__isset`, `__unset`, `__debugInfo`
- Late static binding (`static::` in inherited contexts)
- Proper type declarations and enforcement
- Named arguments (parsed but not matched to params)
- Fibers
- Many extensions (pcre, pdo, curl, etc.)
- Full error/warning message output

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
- `goro-ext-standard` - PHP standard library functions (450+)
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Minimal dependencies**: No external crates (everything hand-written)
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
