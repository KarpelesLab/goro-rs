# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development. Core language features, OOP, generators, exceptions, closures, and references work.

**Test Suite Progress** (PHP 8.5.4 official tests):

| Test Directory | Pass | Total | Rate |
|---|---|---|---|
| Zend/tests (top-level) | 264 | 871 | 30.3% |
| All tests (Zend+ext+standard) | ~2405 | ~11950 | ~20.1% |

**Key directory pass rates:**
- ext/standard/strings: 185/730 (25.3%)
- ext/standard/math: 70/171 (40.9%)
- ext/standard/array: 316/842 (37.5%)
- ext/standard/general_functions: 57/324 (17.6%)
- ext/standard/file: 41/897 (4.6%)
- ext/ctype: 35/49 (71.4%)
- ext/date: 26/688 (3.8%)
- ext/spl: 68/781 (8.7%)
- Zend/traits: 58/216 (26.9%)
- Zend/type_declarations: 86/496 (17.3%)
- Zend/try: 20/80 (25.0%)
- Zend/match: 20/35 (57.1%)
- Zend/closures: 32/135 (23.7%)
- Zend/magic_methods: 29/157 (18.5%)
- Zend/inheritance: 25/70 (35.7%)
- Zend/generators: 46/184 (25.0%)
- Zend/return_types: 20/89 (22.5%)
- Zend/foreach: 14/58 (24.1%)

Best categories: ctype (71%), match (57%), nullable_types (64%), math (41%), inheritance (36%), temporary_cleaning (59%).

*Test runner supports recursive directories, SKIPIF sections, EXPECTF pattern matching with backtracking, and timeout protection.*

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
- `__isset`, `__unset`, `__debugInfo` magic methods
- Full visibility enforcement (protected/private access checks)
- `declare(strict_types=1)` enforcement
- Fibers
- Many extensions (pcre, pdo, curl, etc.)
- Full error/warning message output
- `foreach` by-reference modification
- Namespace resolution at runtime

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
