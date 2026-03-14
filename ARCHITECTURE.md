# goro-rs Architecture

A PHP 8.5 implementation in Rust, targeting full compatibility with PHP's test suite.

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations, cache-friendly data structures
- **Completeness**: Full PHP 8.5.4 language compatibility
- **Minimal dependencies**: Avoid external crates where practical
- **Security**: Fully scopeable file/network access via a virtual filesystem layer
- **Modularity**: SAPIs and extensions are opt-in at compile time via Cargo features

## Execution Pipeline

```
PHP source → Lexer (tokens) → Parser (AST) → Compiler (bytecode) → VM (execution)
```

### 1. Lexer (`goro-parser`)

Hand-written lexer (no generator dependency). Handles PHP's complex lexer states:
- **Initial/HTML mode**: Everything before `<?php` / `<?=` is raw output
- **PHP mode**: Standard token scanning
- **String interpolation**: Variable parsing inside `"..."` and heredoc strings
- **Heredoc/Nowdoc**: Multi-line string literals

### 2. Parser (`goro-parser`)

Recursive descent parser with Pratt parsing for expressions. Produces a typed AST.
PHP's grammar is mostly LL(1) with a few exceptions handled by lookahead.

### 3. Compiler (`goro-core`)

Single-pass AST walk emitting bytecode into an `OpArray` (equivalent to `zend_op_array`).
Each opcode has: opcode type, op1, op2, result (operand slots), and extended_value.

Operand types follow PHP's model:
- **CV** (Compiled Variable): named `$variables`, persist for function scope
- **CONST**: literal values from the constant pool
- **TMP**: short-lived temporaries, no references
- **VAR**: like TMP but can hold references
- **UNUSED**: operand not used

### 4. Virtual Machine (`goro-core`)

Register-based VM with flat call frames (no C-level recursion for PHP calls).
Each call frame (`ExecuteData`) contains the instruction pointer, function reference,
return value slot, and a flat array of zval slots (args + CVs + TMPs).

## Core Data Types

### Value (zval equivalent)

```rust
enum Value {
    Undef,
    Null,
    False,
    True,
    Long(i64),
    Double(f64),
    String(PhpString),      // Rc<PhpStringInner>
    Array(PhpArray),        // Rc<RefCell<PhpArrayInner>>
    Object(PhpObject),      // Rc<RefCell<PhpObjectInner>>
    Resource(PhpResource),  // Rc<RefCell<PhpResourceInner>>
    Reference(PhpRef),      // Rc<RefCell<Value>>
}
```

- Simple types (Undef, Null, Bool, Long, Double) are inline — no allocation
- Complex types use `Rc` for reference counting with copy-on-write via `Rc::make_mut`
- `PhpString` is binary-safe (not UTF-8), caches its hash value
- `PhpArray` is an ordered hash map supporting both packed (sequential int keys)
  and hash (mixed keys) modes

### PhpString

Binary-safe string with cached hash:
```rust
struct PhpStringInner {
    hash: u64,          // cached, computed on first use
    data: Vec<u8>,      // raw bytes, not necessarily UTF-8
}
```

### PhpArray (HashTable equivalent)

Ordered hash map preserving insertion order:
- **Packed mode**: sequential integer keys 0..n, backed by a `Vec<Value>`
- **Hash mode**: arbitrary int/string keys, backed by index table + bucket array
- Automatic mode transition when keys become non-sequential

## Crate Structure

```
goro-rs/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── goro-core/                # Engine: values, VM, compiler, runtime
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── value.rs          # Value enum (zval)
│   │       ├── string.rs         # PhpString
│   │       ├── array.rs          # PhpArray (ordered hash map)
│   │       ├── object.rs         # PhpObject, class entries
│   │       ├── compiler.rs       # AST → bytecode
│   │       ├── opcode.rs         # Opcode definitions
│   │       ├── vm.rs             # Virtual machine / executor
│   │       ├── frame.rs          # Call frames (ExecuteData)
│   │       ├── function.rs       # Function representations
│   │       ├── scope.rs          # Variable scopes
│   │       ├── error.rs          # Error/exception handling
│   │       ├── convert.rs        # Type juggling / coercion
│   │       └── ini.rs            # INI settings
│   │
│   ├── goro-parser/              # Lexer + Parser → AST
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── token.rs          # Token types
│   │       ├── lexer.rs          # Hand-written lexer
│   │       ├── ast.rs            # AST node types
│   │       └── parser.rs         # Recursive descent parser
│   │
│   ├── goro-vfs/                 # Virtual filesystem abstraction
│   │   └── src/
│   │       ├── lib.rs
│   │       └── real.rs           # Real FS (with path restrictions)
│   │
│   ├── goro-sapi/                # SAPI trait + implementations
│   │   └── src/
│   │       ├── lib.rs            # SapiModule trait
│   │       └── cli.rs            # CLI SAPI
│   │
│   ├── goro-ext-standard/        # Standard extension (strings, arrays, math, etc.)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── output.rs         # echo, print, var_dump, print_r
│   │       ├── strings.rs        # String functions
│   │       ├── arrays.rs         # Array functions
│   │       ├── math.rs           # Math functions
│   │       ├── type_funcs.rs     # gettype, settype, is_*, intval, etc.
│   │       └── file.rs           # File functions (via VFS)
│   │
│   └── goro-phpt/                # PHPT test runner
│       └── src/
│           ├── lib.rs
│           └── runner.rs         # Parse and execute .phpt files
│
└── src/
    └── main.rs                   # Binary entry point
```

## SAPI Interface

```rust
trait SapiModule {
    fn name(&self) -> &str;
    fn pretty_name(&self) -> &str;
    fn startup(&mut self) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
    fn activate(&mut self) -> Result<()>;
    fn deactivate(&mut self) -> Result<()>;
    fn write_stdout(&mut self, data: &[u8]) -> Result<usize>;
    fn write_stderr(&mut self, data: &[u8]) -> Result<usize>;
    fn read_stdin(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn register_server_variables(&self, vars: &mut PhpArray);
    // ... headers, cookies, etc.
}
```

## Extension Interface

```rust
trait Extension {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn module_init(&mut self, engine: &mut Engine) -> Result<()>;     // MINIT
    fn module_shutdown(&mut self, engine: &mut Engine) -> Result<()>; // MSHUTDOWN
    fn request_init(&mut self, engine: &mut Engine) -> Result<()>;    // RINIT
    fn request_shutdown(&mut self, engine: &mut Engine) -> Result<()>;// RSHUTDOWN
    fn functions(&self) -> &[FunctionEntry];
}
```

Extensions register functions, classes, constants, and INI entries during `module_init`.
They are compiled in via Cargo features:

```toml
[features]
default = ["ext-standard"]
ext-standard = ["goro-ext-standard"]
ext-json = ["goro-ext-json"]
ext-pcre = ["goro-ext-pcre"]
# ...
```

## Virtual Filesystem

All file operations go through a `Vfs` trait:

```rust
trait Vfs {
    fn open(&self, path: &Path, mode: OpenMode) -> Result<Box<dyn VfsFile>>;
    fn stat(&self, path: &Path) -> Result<FileStat>;
    fn readdir(&self, path: &Path) -> Result<Vec<DirEntry>>;
    fn exists(&self, path: &Path) -> Result<bool>;
    fn realpath(&self, path: &Path) -> Result<PathBuf>;
    // ...
}
```

The default `RealVfs` passes through to the OS but enforces path restrictions
(allowed directories, denied patterns). A `NullVfs` denies all access.
A `MemoryVfs` can be used for testing or sandboxed execution.

## Implementation Phases

See ROADMAP.md for the phased implementation plan.
