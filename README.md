# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development. Core language features, OOP, generators, exceptions, closures, namespaces, enums, and references work.

**Test Suite Progress** (PHP 8.5.4 official tests):

| Test Directory | Pass | Total | Rate |
|---|---|---|---|
| Zend/tests (top-level) | 307 | 871 | 35.2% |
| All tests (Zend+ext+standard) | ~3427 | ~11950 | ~28.7% |

**Key directory pass rates:**
- ext/standard/array: 473/842 (56.2%)
- ext/standard/strings: 323/730 (44.2%)
- ext/standard/math: 112/171 (65.5%)
- ext/spl: 152/781 (19.5%)
- ext/date: 111/688 (16.1%)
- ext/reflection: 97/493 (19.7%)
- ext/pcre: 63/163 (38.7%)
- ext/hash: 16/80 (20.0%)
- ext/json: 32/88 (36.4%)
- ext/ctype: 38/49 (77.6%)
- Zend/type_declarations: 138/496 (27.8%)
- Zend/traits: 85/216 (39.4%)
- Zend/enum: 80/151 (53.0%)
- Zend/namespaces: 76/114 (66.7%)
- Zend/generators: 51/184 (27.7%)
- Zend/closures: 45/135 (33.3%)

*Test runner supports recursive directories, SKIPIF sections, EXPECTF pattern matching with backtracking, and timeout protection.*

## Features

### Working
- Full execution pipeline: PHP source -> lexer -> parser -> AST -> bytecode -> VM
- Types: null, bool, int, float, string (binary-safe), array (ordered hash map), object, reference
- Arithmetic, comparison, bitwise, logical operators with PHP 8 semantics
- String interpolation, concatenation, type juggling, cast operators
- Control flow: if/elseif/else, while, do-while, for, foreach, switch, match, break/continue, goto
- **OOP**: classes, inheritance, interfaces, abstract classes, final classes/methods
- **Traits**: use, insteadof, as, conflict resolution
- **Enums**: backed values, from/tryFrom/cases, methods, interface implementation
- **Namespaces**: use/as imports, group imports, name resolution
- **Visibility**: public/protected/private enforcement for properties and methods
- **Readonly**: readonly properties and classes (PHP 8.1/8.2)
- **Magic methods**: `__construct`, `__toString`, `__get`, `__set`, `__isset`, `__unset`, `__call`, `__callStatic`, `__invoke`, `__clone`
- **Closures**: variable capture, arrow functions, Closure::bind/bindTo/call
- **Generators**: yield, yield from, send, throw, return, finally
- **Exceptions**: try/catch/finally, exception chaining, stack traces
- **Type system**: parameter types, return types, typed properties, union/intersection/DNF types
- **Named parameters**: positional+named mixing, builtin param registry
- **First-class callables**: `strlen(...)`, `$obj->method(...)`, `Foo::method(...)`
- **Throw expressions**: throw in &&, ||, ??, ternary
- **Reflection API**: ReflectionClass, ReflectionMethod, ReflectionFunction, ReflectionProperty, ReflectionParameter
- **SPL**: ArrayObject, SplFixedArray, SplDoublyLinkedList, SplStack, SplQueue, SplHeap, SplPriorityQueue, SplObjectStorage + iterator classes
- **ArrayAccess**: `[]` operator on objects calls offsetGet/offsetSet/offsetExists/offsetUnset
- **Iterator/IteratorAggregate**: foreach on objects implementing Iterator
- **DateTime**: DateTime, DateTimeImmutable, DateInterval, DateTimeZone with format/modify/diff
- **Regex**: Hand-written PCRE-compatible engine (preg_match/replace/split/grep)
- **Hash**: MD5, SHA1, SHA-256/384/512, CRC32, HMAC, streaming API
- eval(), list destructuring, foreach by-reference, dynamic calls
- Memory limit tracking with allocation/deallocation
- 500+ built-in functions, 300+ constants
- CLI SAPI, PHPT test runner

### Not Yet Implemented
- Fibers (PHP 8.1)
- Property hooks (PHP 8.4)
- Pipe operator (PHP 8.5)
- Lazy objects (PHP 8.4)
- Asymmetric visibility (PHP 8.4)
- Stream wrappers / file resources
- `declare(strict_types=1)` enforcement
- Many extensions (pdo, curl, etc.)

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
- `goro-ext-standard` - PHP standard library (500+ functions, regex engine)
- `goro-ext-date` - Date/time extension (DateTime, DateInterval, etc.)
- `goro-ext-json` - JSON extension (json_encode/decode)
- `goro-ext-ctype` - Character type extension
- `goro-ext-hash` - Hash extension (MD5, SHA, CRC32, HMAC)
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Minimal dependencies**: No external crates (everything hand-written)
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
