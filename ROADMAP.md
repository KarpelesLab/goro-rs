# goro-rs Roadmap

## Current Status

**Test Suite**: 288/871 (33.1%) top-level, ~3004 total (25.1% of PHP 8.5.4 suite)

### Major Features Implemented
- [x] Core language: variables, arrays, objects, closures, generators, exceptions
- [x] Namespaces with use/as imports and name resolution
- [x] Enums with backed values, from/tryFrom, cases, methods
- [x] Reflection API (ReflectionClass/Method/Function/Property/Parameter)
- [x] DateTime/DateTimeImmutable/DateInterval/DateTimeZone
- [x] Hand-written regex engine (preg_match/replace/split/grep)
- [x] Visibility enforcement (public/protected/private)
- [x] Closure::bind/bindTo/call with scope
- [x] Iterator/IteratorAggregate for foreach
- [x] First-class callables (strlen(...))
- [x] Readonly properties and classes
- [x] Named parameters with builtin param registry
- [x] Throw expressions (&&, ||, ??, ternary)
- [x] Typed property enforcement
- [x] Method signature compatibility checking
- [x] eval(), goto/label, list() destructuring
- [x] try/catch/finally with proper exception propagation
- [x] SHA-256/384/512, hash_hmac, hash streaming API
- [x] 20+ mb_* multibyte functions
- [x] User error handlers (set_error_handler)
- [x] Trait insteadof/as conflict resolution
- [x] PHP 8 comparison semantics
- [x] Memory limit tracking with allocation/deallocation
- [x] SPL classes (ArrayObject, SplFixedArray, SplDoublyLinkedList, etc.)
- [x] Extension split (goro-ext-date/json/ctype/hash)
- [x] 300+ built-in constants
- [x] 400+ built-in functions

### Next Priorities (to reach 50%+)
- [ ] Fibers (PHP 8.1) - stackful coroutines
- [ ] Property hooks (PHP 8.4) - get/set on properties
- [ ] Pipe operator (PHP 8.5) - $x |> fn()
- [ ] Streams/file resources - fopen/fread/fwrite resource type
- [ ] ArrayAccess [] operator on objects
- [ ] foreach by-reference
- [ ] More complete type covariance/contravariance checking
- [ ] Autoloading (spl_autoload_register callbacks)
- [ ] More Reflection edge cases
- [ ] More DateTime format support
- [ ] Full mbstring encoding conversion
- [ ] More SPL iterators (RecursiveIteratorIterator, etc.)
