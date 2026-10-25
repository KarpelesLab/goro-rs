--TEST--
Deprecated internal methods: the deprecation from the call (no frame of the method), then the argument checks; a throwing handler skips the method
--FILE--
<?php
class A { private $x; private function m() {} }
function g(stdClass $a) {}
$m = new ReflectionMethod('A', 'm');
$p = new ReflectionProperty('A', 'x');
$f = new ReflectionFunction('strlen');
$pa = new ReflectionParameter('g', 0);
$s = new SplObjectStorage;
$o = new stdClass;
$db = new PDO('sqlite::memory:');
$calls = [
    [$m, 'setAccessible', []], [$m, 'setAccessible', [true, 2]],
    [$p, 'setAccessible', []], [$f, 'isDisabled', [1]],
    [$pa, 'getClass', [1]], [$pa, 'isArray', [1]], [$pa, 'isCallable', [1, 2]],
    [$s, 'attach', []], [$s, 'contains', [$o, 1]], [$s, 'detach', [1]],
    [new DateTime('@0'), '__wakeup', [1]], [new DateTimeZone('UTC'), '__wakeup', [1]],
    [new SplFixedArray(1), '__wakeup', [1]],
    [$db, 'sqliteCreateFunction', []],
];
foreach ($calls as [$obj, $name, $args]) {
    try { var_dump($obj->$name(...$args)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { var_dump($m->setAccessible(foo: true)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($m->setAccessible(accessible: true));
var_dump(call_user_func([$pa, 'getClass'])->name);

function frames() {
    foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS) as $f) {
        echo "  ", $f['function'], isset($f['line']) ? " @" . $f['line'] : "", "\n";
    }
}
function w($obj, $name, $a) { return $obj->$name(...$a); }
$good = [
    [$m, 'setAccessible', [true]], [$p, 'setAccessible', [true]], [$f, 'isDisabled', []],
    [$pa, 'getClass', []], [$pa, 'isArray', []], [$s, 'attach', [new stdClass]],
    [new DateInterval('P1D'), '__wakeup', []], [$db, 'sqliteCreateFunction', ['f', 'strlen']],
];
// The deprecation comes from the caller's code: no frame for the method.
set_error_handler(function ($n, $s) { echo "$s\n"; frames(); return true; });
foreach ($good as [$obj, $name, $a]) {
    $r = w($obj, $name, $a);
    echo gettype($r), "\n";
}
// A throwing handler: the method is not run.
set_error_handler(function ($n, $s) { throw new Exception($s); });
foreach ($good as [$obj, $name, $a]) {
    try { w($obj, $name, $a); echo "ran\n"; } catch (Exception $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
}
var_dump(count($s));
--EXPECTF--

Deprecated: Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 in %s on line 21
ArgumentCountError: ReflectionMethod::setAccessible() expects exactly 1 argument, 0 given

Deprecated: Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 in %s on line 21
ArgumentCountError: ReflectionMethod::setAccessible() expects exactly 1 argument, 2 given

Deprecated: Method ReflectionProperty::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 in %s on line 21
ArgumentCountError: ReflectionProperty::setAccessible() expects exactly 1 argument, 0 given

Deprecated: Method ReflectionFunction::isDisabled() is deprecated since 8.0, as ReflectionFunction can no longer be constructed for disabled functions in %s on line 21
ArgumentCountError: ReflectionFunction::isDisabled() expects exactly 0 arguments, 1 given

Deprecated: Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead in %s on line 21
ArgumentCountError: ReflectionParameter::getClass() expects exactly 0 arguments, 1 given

Deprecated: Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead in %s on line 21
ArgumentCountError: ReflectionParameter::isArray() expects exactly 0 arguments, 1 given

Deprecated: Method ReflectionParameter::isCallable() is deprecated since 8.0, use ReflectionParameter::getType() instead in %s on line 21
ArgumentCountError: ReflectionParameter::isCallable() expects exactly 0 arguments, 2 given

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 21
ArgumentCountError: SplObjectStorage::attach() expects at least 1 argument, 0 given

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 21
ArgumentCountError: SplObjectStorage::contains() expects exactly 1 argument, 2 given

Deprecated: Method SplObjectStorage::detach() is deprecated since 8.5, use method SplObjectStorage::offsetUnset() instead in %s on line 21
TypeError: SplObjectStorage::detach(): Argument #1 ($object) must be of type object, int given

Deprecated: Method DateTime::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 21
ArgumentCountError: DateTime::__wakeup() expects exactly 0 arguments, 1 given

Deprecated: Method DateTimeZone::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 21
ArgumentCountError: DateTimeZone::__wakeup() expects exactly 0 arguments, 1 given

Deprecated: Method SplFixedArray::__wakeup() is deprecated since 8.4, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 21
ArgumentCountError: SplFixedArray::__wakeup() expects exactly 0 arguments, 1 given

Deprecated: Method PDO::sqliteCreateFunction() is deprecated since 8.5, use Pdo\Sqlite::createFunction() instead in %s on line 21
ArgumentCountError: PDO::sqliteCreateFunction() expects at least 2 arguments, 0 given
Error: Unknown named parameter $foo

Deprecated: Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 in %s on line 24
NULL

Deprecated: Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead in %s on line 25
string(8) "stdClass"
Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1
  frames @39
  {closure:%s:39} @32
  w @41
NULL
Method ReflectionProperty::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1
  frames @39
  {closure:%s:39} @32
  w @41
NULL
Method ReflectionFunction::isDisabled() is deprecated since 8.0, as ReflectionFunction can no longer be constructed for disabled functions
  frames @39
  {closure:%s:39} @32
  w @41
boolean
Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead
  frames @39
  {closure:%s:39} @32
  w @41
object
Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead
  frames @39
  {closure:%s:39} @32
  w @41
boolean
Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead
  frames @39
  {closure:%s:39} @32
  w @41
NULL
Method DateInterval::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()
  frames @39
  {closure:%s:39} @32
  w @41
NULL
Method PDO::sqliteCreateFunction() is deprecated since 8.5, use Pdo\Sqlite::createFunction() instead
  frames @39
  {closure:%s:39} @32
  w @41
boolean
Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 @45
Method ReflectionProperty::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1 @45
Method ReflectionFunction::isDisabled() is deprecated since 8.0, as ReflectionFunction can no longer be constructed for disabled functions @45
Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead @45
Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead @45
Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead @45
Method DateInterval::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() @45
Method PDO::sqliteCreateFunction() is deprecated since 8.5, use Pdo\Sqlite::createFunction() instead @45
int(1)
