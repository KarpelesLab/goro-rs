# goro-rs Roadmap

## Phase 1: Minimum Viable Engine (Target: "Hello World")

Get `<?php echo "Hello, World!\n";` working end-to-end.

- [x] Project structure (Cargo workspace)
- [x] **Lexer**: Tokenize basic PHP (`<?php`, strings, integers, identifiers, operators, `;`)
- [x] **Parser**: Parse `echo` statement, string/integer literals, basic expressions
- [x] **AST**: Define initial node types (Echo, Literal, BinaryOp)
- [x] **Compiler**: Compile echo + literals to bytecodes
- [x] **Opcodes**: ECHO, RETURN, NOP, basic arithmetic (ADD, SUB, MUL, DIV, MOD, CONCAT)
- [x] **VM**: Execute opcodes, manage a single call frame
- [x] **Value**: Null, Bool, Long, Double, String (basic)
- [x] **CLI SAPI**: Read file or `-r` string, run it, print output
- [x] **PHPT runner**: Parse .phpt files, run --FILE--, compare with --EXPECT--

## Phase 2: Variables & Expressions

- [x] Variable assignment (`$x = ...`)
- [x] All arithmetic operators (`+`, `-`, `*`, `/`, `%`, `**`)
- [x] String concatenation (`.`)
- [x] Comparison operators (`==`, `===`, `!=`, `!==`, `<`, `>`, `<=`, `>=`, `<=>`)
- [x] Logical operators (`&&`, `||`, `!`, `and`, `or`, `xor`)
- [x] Bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`)
- [x] Assignment operators (`+=`, `-=`, `.=`, etc.)
- [x] Increment/decrement (`++`, `--`)
- [x] Type juggling / coercion (string↔int, int↔float, truthy/falsy)
- [x] String interpolation (`"Hello $name"`, `"Hello {$name}"`)
- [ ] Heredoc / Nowdoc
- [ ] Constants (`define()`, `const`)
- [x] Ternary (`?:`), null coalescing (`??`), null coalescing assignment (`??=`)

## Phase 3: Control Flow

- [x] `if` / `elseif` / `else`
- [x] `while` / `do-while`
- [x] `for`
- [x] `foreach` (arrays)
- [x] `switch` / `case` / `default`
- [x] `match` expression
- [x] `break` / `continue` (with levels)
- [x] `return`
- [ ] `goto` / labels
- [ ] `declare(strict_types=1)`

## Phase 4: Functions

- [x] Function declarations
- [x] Function calls (user-defined and built-in)
- [ ] Default parameter values
- [ ] Variadic parameters (`...$args`)
- [ ] Argument unpacking (`func(...$args)`)
- [ ] Pass by reference (`&$param`)
- [ ] Return types
- [ ] Named arguments
- [ ] `global` and `static` variables
- [ ] Closures / anonymous functions
- [ ] Arrow functions (`fn($x) => $x * 2`)
- [ ] First-class callable syntax (`strlen(...)`)
- [ ] Recursion
- [ ] Variable functions (`$func()`)

## Phase 5: Arrays

- [ ] Array literals (`[1, 2, 3]`, `['a' => 1]`)
- [ ] Packed arrays (sequential int keys, optimized storage)
- [ ] Hash arrays (mixed string/int keys)
- [ ] Array access (`$arr[$key]`, `$arr[]`)
- [ ] Nested arrays
- [ ] `foreach` with keys and values
- [ ] Array unpacking (`[...$a, ...$b]`)
- [ ] `list()` / `[]` destructuring
- [ ] Array functions from ext-standard (`count`, `array_push`, `array_map`, etc.)

## Phase 6: Strings

- [ ] Single-quoted strings
- [ ] Double-quoted strings with interpolation
- [ ] Heredoc / Nowdoc
- [ ] String access by offset (`$str[0]`)
- [ ] Binary-safe string operations
- [ ] String functions from ext-standard (`strlen`, `strpos`, `substr`, `str_replace`, etc.)
- [ ] `printf` / `sprintf` family

## Phase 7: Object-Oriented Programming

- [ ] Class declarations
- [ ] Properties (typed, untyped, default values)
- [ ] Methods
- [ ] Constructors / destructors
- [ ] `$this` and `self`
- [ ] Visibility (`public`, `protected`, `private`)
- [ ] Inheritance (`extends`)
- [ ] `parent::` calls
- [ ] `static` properties and methods
- [ ] Abstract classes and methods
- [ ] Interfaces
- [ ] Traits (use, conflict resolution)
- [ ] Class constants
- [ ] `instanceof`
- [ ] Enums (basic, backed, methods)
- [ ] Readonly properties
- [ ] Constructor promotion
- [ ] Named arguments in constructors
- [ ] `::class` constant
- [ ] Anonymous classes
- [ ] Magic methods (`__construct`, `__destruct`, `__get`, `__set`, `__call`,
  `__callStatic`, `__toString`, `__invoke`, `__clone`, `__debugInfo`, etc.)
- [ ] Object cloning (`clone`, `clone with` in 8.5)
- [ ] Late static binding (`static::`)
- [ ] Property hooks (PHP 8.4+)
- [ ] Asymmetric visibility (PHP 8.4+)

## Phase 8: Error Handling

- [ ] `try` / `catch` / `finally`
- [ ] `throw` expression
- [ ] Exception hierarchy (`Throwable`, `Exception`, `Error`)
- [ ] Custom exception classes
- [ ] `set_error_handler()`, `set_exception_handler()`
- [ ] Error levels (E_ERROR, E_WARNING, E_NOTICE, etc.)
- [ ] `@` error suppression operator
- [ ] Error → Exception conversion

## Phase 9: Type System

- [ ] Scalar type declarations (`int`, `float`, `string`, `bool`)
- [ ] `array`, `callable`, `iterable`, `object`, `mixed`
- [ ] `void`, `never`, `null`, `false`, `true`
- [ ] Union types (`int|string`)
- [ ] Intersection types (`Foo&Bar`)
- [ ] DNF types (`(Foo&Bar)|null`)
- [ ] Nullable types (`?int`)
- [ ] `strict_types` enforcement
- [ ] Return type checking
- [ ] Property type enforcement

## Phase 10: Standard Extension (ext-standard)

- [ ] Output: `echo`, `print`, `var_dump`, `print_r`, `var_export`
- [ ] Type functions: `gettype`, `settype`, `is_*`, `intval`, `floatval`, `strval`, `boolval`
- [ ] String functions: `strlen`, `strpos`, `substr`, `str_replace`, `strtolower`, `strtoupper`,
  `trim`, `ltrim`, `rtrim`, `explode`, `implode`, `sprintf`, `printf`, `number_format`,
  `str_pad`, `str_repeat`, `str_word_count`, `str_contains`, `str_starts_with`, `str_ends_with`,
  `nl2br`, `wordwrap`, `chunk_split`, `quoted_printable_encode/decode`, `base64_encode/decode`,
  `urlencode/decode`, `rawurlencode/decode`, `html_entity_decode`, `htmlspecialchars`,
  `htmlspecialchars_decode`, `htmlentities`, `crc32`, `md5`, `sha1`, `hex2bin`, `bin2hex`,
  `ord`, `chr`, `pack`, `unpack`, etc.
- [ ] Array functions: `count`, `array_push`, `array_pop`, `array_shift`, `array_unshift`,
  `array_merge`, `array_keys`, `array_values`, `array_map`, `array_filter`, `array_reduce`,
  `array_walk`, `array_slice`, `array_splice`, `array_search`, `in_array`, `array_unique`,
  `array_flip`, `array_reverse`, `array_combine`, `array_chunk`, `array_column`,
  `array_diff`, `array_intersect`, `sort`, `rsort`, `asort`, `arsort`, `ksort`, `krsort`,
  `usort`, `uasort`, `uksort`, `array_multisort`, `array_rand`, `shuffle`,
  `array_first`, `array_last` (8.5), `compact`, `extract`, `range`, etc.
- [ ] Math functions: `abs`, `ceil`, `floor`, `round`, `max`, `min`, `pow`, `sqrt`,
  `fmod`, `intdiv`, `rand`, `mt_rand`, `random_int`, `random_bytes`, etc.
- [ ] File functions (via VFS): `fopen`, `fclose`, `fread`, `fwrite`, `fgets`, `feof`,
  `file_get_contents`, `file_put_contents`, `file_exists`, `is_file`, `is_dir`,
  `mkdir`, `rmdir`, `unlink`, `rename`, `copy`, `realpath`, `dirname`, `basename`,
  `pathinfo`, `glob`, `scandir`, `tempnam`, `sys_get_temp_dir`, etc.
- [ ] Date/time: `time`, `microtime`, `date`, `mktime`, `strtotime`, `gmdate`, etc.
- [ ] Misc: `sleep`, `usleep`, `exit`/`die`, `phpversion`, `phpinfo`, `php_uname`,
  `memory_get_usage`, `memory_get_peak_usage`, `gc_collect_cycles`, etc.

## Phase 11: Core Extensions

- [ ] **ext-json**: `json_encode`, `json_decode`, `json_last_error`
- [ ] **ext-pcre**: `preg_match`, `preg_match_all`, `preg_replace`, `preg_split`
- [ ] **ext-ctype**: `ctype_alpha`, `ctype_digit`, etc.
- [ ] **ext-mbstring**: Multi-byte string support
- [ ] **ext-tokenizer**: `token_get_all`, `token_name`
- [ ] **ext-filter**: `filter_var`, `filter_input`
- [ ] **ext-hash**: `hash`, `hash_hmac`, various algorithms
- [ ] **ext-spl**: Iterators, data structures, autoloading, exceptions
- [ ] **ext-reflection**: `ReflectionClass`, `ReflectionFunction`, etc.

## Phase 12: Advanced Language Features

- [ ] Generators (`yield`, `yield from`)
- [ ] Fibers (`Fiber` class)
- [ ] References (`&`)
- [ ] `include` / `require` / `include_once` / `require_once`
- [ ] Namespaces
- [ ] Autoloading (`spl_autoload_register`)
- [ ] Attributes (`#[...]`)
- [ ] `eval()`
- [ ] Pipe operator (`|>`, PHP 8.5)
- [ ] Output buffering (`ob_start`, `ob_get_contents`, etc.)

## Phase 13: Additional SAPIs

- [ ] **CLI Server** (`php -S`): Built-in development web server
- [ ] **Embed**: Library mode for embedding in other Rust applications
- [ ] **FPM-like**: FastCGI process manager (async I/O)
- [ ] **CGI**: Traditional CGI interface

## Phase 14: Remaining Extensions

- [ ] **ext-pdo** + drivers
- [ ] **ext-mysqli** / **ext-mysqlnd**
- [ ] **ext-curl**
- [ ] **ext-openssl**
- [ ] **ext-zlib**
- [ ] **ext-xml** / **ext-dom** / **ext-simplexml** / **ext-libxml**
- [ ] **ext-session**
- [ ] **ext-socket**
- [ ] **ext-fileinfo**
- [ ] **ext-intl**
- [ ] **ext-gd**
- [ ] And all remaining extensions...

## Test Strategy

Use PHP 8.5.4's official `.phpt` test suite:
1. Build a PHPT runner (`goro-phpt`) that parses test files and executes them
2. Start with `Zend/tests/` (core language tests)
3. Expand to `ext/standard/tests/` and other extension test directories
4. Track pass rate continuously, target 100%
5. Prioritize fixing failures in order of the phases above
