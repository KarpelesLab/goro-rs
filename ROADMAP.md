# goro-rs Roadmap

## Current Status

**Test Suite**: 208/871 (23.9%) top-level, 1064 total (all dirs with proper timeouts)

### Recently Completed (this session)
- [x] Named arguments (basic support)
- [x] Parameter type checking with PHP coercion
- [x] MD5 and SHA1 hash from scratch (no external deps)
- [x] hash(), hash_algos(), hash_equals()
- [x] parse_url() with full URL component parsing
- [x] 20+ string functions (strrchr, stristr, strtok, strspn, etc.)
- [x] mb_* function stubs
- [x] Complete ctype extension (5 new functions)
- [x] is_scalar, is_countable, is_iterable, get_debug_type
- [x] get_declared_classes/traits/interfaces (proper implementation)
- [x] Math: log1p, expm1, number_format, intval, floatval
- [x] ??= operator fix
- [x] Test runner timeout optimization

### Completed Features

- Full execution pipeline: PHP source -> lexer -> parser -> AST -> bytecode -> VM
- Types: null, bool, int, float, string (binary-safe), array (ordered hash map), object, reference
- All arithmetic, comparison, bitwise, logical operators with type juggling
- String interpolation, concatenation, cast operators
- Control flow: if/elseif/else, while, do-while, for, foreach, switch, match, break N/continue N
- Functions: declarations, calls, default params, variadic (`...$args`), closures, arrow functions
- OOP: classes, inheritance, static props/methods, constants, instanceof, abstract classes
- Magic methods: `__construct`, `__toString`, `__get`, `__set`, `__call`, `__invoke`, `clone`, `__destruct`
- Generators: `yield`, `yield $key => $value`, foreach over generators
- References: `$b = &$a`, shared value mutation through `Rc<RefCell<Value>>`
- Exceptions: try/catch/finally with cross-function propagation
- Late static binding (`static::`)
- Interface/trait declarations (parsed, basic enforcement)
- 450+ built-in functions (array, string, math, type, output, JSON, OOP introspection)
- include/require, define/defined/constant, static/global variables
- PHPT test runner with EXPECTF pattern matching, SKIPIF support, timeout protection

---

## Next Steps (Priority Order)

### P0: Error/Warning Output (highest impact - ~163 tests)

The single highest-impact improvement area. Many tests expect PHP's diagnostic output.

- [x] **Fatal error / Uncaught exception formatting** (~108 tests)
  - Output `Fatal error: Uncaught ExceptionType: message in file:line\nStack trace:\n#0 {main}\n  thrown in file on line N`
  - Test runner captures error output and includes it in test comparison
- [ ] **Warning emission** (~30 tests)
  - `Undefined variable`, `Undefined array key`, `Attempt to read property on null`
  - `Division by zero` warning (not fatal), `array_key_exists` warnings
  - `Illegal string offset` warnings
- [ ] **Notice/Deprecated emission** (~25 tests)
  - `Deprecated: ... is deprecated` messages for deprecated features
  - Proper `E_NOTICE`, `E_WARNING`, `E_DEPRECATED` level handling
- [ ] **set_error_handler()** / **set_exception_handler()**
- [x] **@ error suppression operator** - ErrorSuppress/ErrorRestore opcodes
- [x] **trigger_error()** / **user_error()** - E_USER_ERROR/WARNING/NOTICE/DEPRECATED

### P1: Core Language Gaps (high impact - ~80 tests)

- [x] **Variable variables** (`$$var`, `${$expr}`) - VarVarGet/VarVarSet opcodes
- [x] **Argument unpacking** in calls (`func(...$args)`) - SendUnpack opcode
- [x] **Array spread** in literals (`[...$a, ...$b]`) - ArraySpread opcode
- [x] **Proper array copy-on-write semantics** - clone array on CV assignment
- [x] **`::class` constant** on variables/expressions (`$obj::class`, `ClassName::class`)
- [ ] **Dynamic member access** (`$class::$prop`, `Class::{$expr}`)
- [x] **Heredoc / Nowdoc** strings - variable interpolation and escape sequences
- [ ] **`goto` / labels**
- [ ] **`declare(strict_types=1)`**
- [ ] **String offset access** (`$str[0]` read and write)

### P2: OOP Completions (~50 tests)

- [x] **`__callStatic`** magic method
- [ ] **`__isset` / `__unset`** magic methods
- [ ] **`__debugInfo`** magic method
- [ ] **Proper visibility enforcement** (protected/private access checks)
- [ ] **Abstract method enforcement** in child classes
- [ ] **Interface method signature enforcement**
- [ ] **Trait conflict resolution** (`insteadof`, `as`)
- [ ] **Enum enforcement** (backed values, methods, implements)
- [ ] **Readonly properties**
- [x] **Constructor promotion** (`public function __construct(public $x)`)
- [ ] **Anonymous classes** (runtime)
- [ ] **Property hooks** (PHP 8.4+ - `get {}` / `set {}` blocks)

### P3: Type System (~40 tests)

- [ ] **Type declarations** on parameters and return types (parse + enforce)
- [ ] **Union types** (`int|string`)
- [ ] **Intersection types** (`Foo&Bar`)
- [ ] **Nullable types** (`?int`)
- [ ] **`strict_types` enforcement**
- [ ] **TypeError** for type mismatches
- [ ] **`void`, `never`, `null`, `false`, `true`** return types

### P4: Missing Built-in Functions (~30 tests)

Functions needed by tests (ordered by test count):

- [ ] `stream_wrapper_register` (11 tests) - needs stream wrapper infrastructure
- [ ] `parse_ini_file` / `parse_ini_string` (7 tests)
- [ ] `Closure::fromCallable` / `Closure::bind` / `Closure::bindTo` (4 tests)
- [ ] `array_merge_recursive`, `array_fill_keys`, `array_multisort` (3 tests)
- [ ] `strtok`, `stristr`, `getcwd`, `set_include_path` (1 each)
- [ ] `register_shutdown_function` (needs shutdown hook infrastructure)
- [ ] `class_alias`
- [ ] `compact` / `extract`
- [ ] `range`

### P5: String Operations (~15 tests)

- [ ] **Bitwise operations on strings** (`$str & $str` produces string, not int)
- [x] **String increment** (`$str++` follows PHP rules: "a" -> "b", "z" -> "aa")
- [ ] **Proper string offset** read/write with bounds checking

### P6: Generators & Fibers (~10 tests)

- [ ] **`yield from`** delegation
- [ ] **Generator::throw()**
- [ ] **Generator::getReturn()**
- [ ] **Fibers** (`Fiber` class, `Fiber::suspend()`, etc.)

### P7: Advanced Features

- [ ] **Named arguments** (`func(name: value)`)
- [ ] **First-class callable syntax** (`strlen(...)`)
- [ ] **Attributes** (`#[Attribute]` - currently skipped in lexer)
- [ ] **`eval()`**
- [ ] **Pipe operator** (`|>`, PHP 8.5)
- [ ] **Namespaces** (runtime resolution, autoloading)
- [ ] **Output buffering** (`ob_start`, `ob_get_contents`, etc.)

### P8: Extensions

- [ ] **ext-pcre**: `preg_match`, `preg_replace`, `preg_split` (hand-written regex engine)
- [ ] **ext-json**: Full `json_decode` (currently stub)
- [ ] **ext-spl**: Iterators, `ArrayAccess`, `Countable`, `Iterator`
- [ ] **ext-ctype**: `ctype_alpha`, `ctype_digit`, etc.
- [ ] **ext-mbstring**: Multi-byte string support

---

## Test Strategy

1. Run PHP 8.5.4 official `.phpt` test suite (11,950 tests total)
2. Focus on Zend/tests (871 top-level + subdirectories) for core language
3. Prioritize fixes by test count impact
4. Track pass rate continuously, target 100%
5. Use SKIPIF sections to skip tests requiring unimplemented extensions

## Architecture Constraints

- **No external crates** - everything hand-written
- **Binary-safe strings** throughout
- **Virtual filesystem** for security sandboxing
- **Modular SAPIs and extensions** - opt-in at compile time
