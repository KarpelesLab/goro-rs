--TEST--
Multi-line chains: method calls on the method name's line, property write fetches on the property's
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
class A { function m() { return $this; } }
t(fn() => (new A)
    ->m()
    ->zz());
$n = null;
t(fn() => $n
    ->m());
$o = new stdClass;
t(fn() => $o
    ?->foo()
    ->bar());
t(function () {
    $a = new class { public function __get($n) { return null; } };
    $a
        ->x
        ->y
        ->z = 1;
});
t(fn() => array_key_exists(
    [],
    []
));
foreach ([[1], [2, 3]] as [
    $p,
    $q,
]) {}
foreach ([] as $k => $v) {}
foreach (
    $undef_arr as $v
) {}
t(fn() => new
    DateTime('garbage'));
--EXPECTF--
Error: Call to undefined method A::zz() @6
Error: Call to a member function m() on null @9
Error: Call to undefined method stdClass::foo() @12

Notice: Indirect modification of overloaded property class@anonymous::$x has no effect in %s on line 17
Error: Attempt to modify property "y" on null @18
TypeError: Cannot access offset of type array on array @23

Warning: Undefined array key 1 in %s on line 26

Warning: Undefined variable $undef_arr in %s on line 31

Warning: foreach() argument must be of type array|object, null given in %s on line 31
DateMalformedStringException: Failed to parse time string (garbage) at position 0 (g): The timezone could not be found in the database @34
