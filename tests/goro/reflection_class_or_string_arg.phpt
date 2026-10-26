--TEST--
ReflectionClass::implementsInterface()/isSubclassOf() parse ReflectionClass|string: Stringable objects, scalars, ReflectionClass subclasses
--FILE--
<?php
class Str { function __toString(): string { return "Countable"; } }
class RC extends ReflectionClass {}
$r = new ReflectionClass('ArrayObject');
foreach ([new Str, new stdClass, NAN, 1, null, new ReflectionClass('Countable'), new RC('Countable'), new ReflectionObject(new ArrayObject), new ReflectionEnum('Random\IntervalBoundary')] as $a) {
    try { var_dump($r->implementsInterface($a)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump($r->isSubclassOf($a)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
// DateTimeImmutable::add()/sub()/setTimezone(): the argument count comes first
--EXPECTF--
bool(true)
bool(true)
TypeError: ReflectionClass::implementsInterface(): Argument #1 ($interface) must be of type ReflectionClass|string, stdClass given
TypeError: ReflectionClass::isSubclassOf(): Argument #1 ($class) must be of type ReflectionClass|string, stdClass given

Warning: unexpected NAN value was coerced to string in %s on line 6
ReflectionException: Interface "NAN" does not exist

Warning: unexpected NAN value was coerced to string in %s on line 7
ReflectionException: Class "NAN" does not exist
ReflectionException: Interface "1" does not exist
ReflectionException: Class "1" does not exist

Deprecated: ReflectionClass::implementsInterface(): Passing null to parameter #1 ($interface) of type ReflectionClass|string is deprecated in %s on line 6
ReflectionException: Interface "" does not exist

Deprecated: ReflectionClass::isSubclassOf(): Passing null to parameter #1 ($class) of type ReflectionClass|string is deprecated in %s on line 7
ReflectionException: Class "" does not exist
bool(true)
bool(true)
bool(true)
bool(true)
ReflectionException: ArrayObject is not an interface
bool(false)
ReflectionException: Random\IntervalBoundary is not an interface
bool(false)
