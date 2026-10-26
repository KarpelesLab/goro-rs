--TEST--
assert() $description is Throwable|string|null: scalars and Stringable objects are converted
--FILE--
<?php
class S { function __toString(): string { return "s!"; } }
foreach ([5, 1.5, true, NAN, new S, new stdClass, [1], null, "str", new LogicException("le")] as $d) {
    try { var_dump(assert(false, $d)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(assert(true, $d)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
AssertionError: 5
bool(true)
AssertionError: 1.5
bool(true)
AssertionError: 1
bool(true)

Warning: unexpected NAN value was coerced to string in %s on line 4
AssertionError: NAN

Warning: unexpected NAN value was coerced to string in %s on line 5
bool(true)
AssertionError: s!
bool(true)
TypeError: assert(): Argument #2 ($description) must be of type Throwable|string|null, stdClass given
TypeError: assert(): Argument #2 ($description) must be of type Throwable|string|null, stdClass given
TypeError: assert(): Argument #2 ($description) must be of type Throwable|string|null, array given
TypeError: assert(): Argument #2 ($description) must be of type Throwable|string|null, array given
AssertionError: 
bool(true)
AssertionError: str
bool(true)
LogicException: le
bool(true)
