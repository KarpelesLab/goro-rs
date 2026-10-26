--TEST--
newLazyGhost/newLazyProxy/resetAsLazy*(): uninstantiable classes, invalid callbacks, wrong object types
--FILE--
<?php
interface I {}
abstract class Ab {}
enum En { case A; }
trait T {}
foreach (['I', 'Ab', 'En', 'T'] as $c) {
    foreach (['newLazyGhost', 'newLazyProxy'] as $m) {
        try {
            $r = new ReflectionClass($c);
            var_dump($r->$m(function () {}));
        } catch (Throwable $e) {
            echo get_class($e), ": ", $e->getMessage(), "\n";
        }
    }
}
foreach (['nonexist', 'strlen', [1, 2], 'A::b'] as $cb) {
    try {
        (new ReflectionClass('stdClass'))->newLazyGhost($cb);
    } catch (Throwable $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
    try {
        (new ReflectionClass('stdClass'))->newLazyProxy($cb);
    } catch (Throwable $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
    try {
        (new ReflectionClass('stdClass'))->resetAsLazyProxy(new stdClass, $cb);
    } catch (Throwable $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
}
class A {}
try {
    (new ReflectionClass('stdClass'))->resetAsLazyGhost(new A, fn() => 1);
} catch (Throwable $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
try {
    (new ReflectionClass('stdClass'))->resetAsLazyProxy(new A, fn() => 1);
} catch (Throwable $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
try {
    (new ReflectionClass('A'))->resetAsLazyProxy(new stdClass, fn() => 1);
} catch (Throwable $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
foreach ([1, "x", null, [1]] as $v) {
    try { (new ReflectionClass("A"))->resetAsLazyGhost($v, fn() => 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { (new ReflectionClass("I"))->newLazyGhost(fn() => 1, 8); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new ReflectionClass("I"))->newLazyGhost("nope", 8); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new ReflectionClass("ArrayObject"))->newLazyGhost(fn() => 1, 8); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new ReflectionClass("A"))->resetAsLazyGhost(new A, "nope", 8); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
Error: Cannot instantiate interface I
Error: Cannot instantiate interface I
Error: Cannot instantiate abstract class Ab
Error: Cannot instantiate abstract class Ab
Error: Cannot instantiate enum En
Error: Cannot instantiate enum En
Error: Cannot instantiate trait T
Error: Cannot instantiate trait T
TypeError: ReflectionClass::newLazyGhost(): Argument #1 ($initializer) must be a valid callback, function "nonexist" not found or invalid function name
TypeError: ReflectionClass::newLazyProxy(): Argument #1 ($factory) must be a valid callback, function "nonexist" not found or invalid function name
TypeError: ReflectionClass::resetAsLazyProxy(): Argument #2 ($factory) must be a valid callback, function "nonexist" not found or invalid function name
TypeError: ReflectionClass::newLazyGhost(): Argument #1 ($initializer) must be a valid callback, first array member is not a valid class name or object
TypeError: ReflectionClass::newLazyProxy(): Argument #1 ($factory) must be a valid callback, first array member is not a valid class name or object
TypeError: ReflectionClass::resetAsLazyProxy(): Argument #2 ($factory) must be a valid callback, first array member is not a valid class name or object
TypeError: ReflectionClass::newLazyGhost(): Argument #1 ($initializer) must be a valid callback, class A does not have a method "b"
TypeError: ReflectionClass::newLazyProxy(): Argument #1 ($factory) must be a valid callback, class A does not have a method "b"
TypeError: ReflectionClass::resetAsLazyProxy(): Argument #2 ($factory) must be a valid callback, class A does not have a method "b"
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #1 ($object) must be of type stdClass, A given
TypeError: ReflectionClass::resetAsLazyProxy(): Argument #1 ($object) must be of type stdClass, A given
TypeError: ReflectionClass::resetAsLazyProxy(): Argument #1 ($object) must be of type A, stdClass given
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #1 ($object) must be of type A, int given
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #1 ($object) must be of type A, string given
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #1 ($object) must be of type A, null given
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #1 ($object) must be of type A, array given
Error: Cannot instantiate interface I
TypeError: ReflectionClass::newLazyGhost(): Argument #1 ($initializer) must be a valid callback, function "nope" not found or invalid function name
Error: Cannot make instance of internal class lazy: ArrayObject is internal
TypeError: ReflectionClass::resetAsLazyGhost(): Argument #2 ($initializer) must be a valid callback, function "nope" not found or invalid function name
