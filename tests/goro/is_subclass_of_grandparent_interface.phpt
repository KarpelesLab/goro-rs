--TEST--
is_subclass_of()/is_a() with a class string see interfaces inherited through an interface's parent
--FILE--
<?php
interface I {}
interface J extends I {}
class A implements I {}
class B extends A {}
class C implements J {}
foreach ([['A', 'I'], ['B', 'I'], ['C', 'I'], ['C', 'J'], ['B', 'A'], ['A', 'A'], ['J', 'I'], ['I', 'I']] as [$x, $y]) {
    var_dump(is_subclass_of($x, $y), is_subclass_of($x, $y, true), is_subclass_of($x, $y, false));
}
var_dump(is_subclass_of(new A, 'I'), is_subclass_of(new A, 'I', false));
interface K extends J {} class E implements K {} abstract class F implements K {} class G extends F {}
foreach (["E", "G"] as $c) { $r = new ReflectionClass($c); var_dump(is_a($c, "I", true), is_subclass_of($c, "I"), $r->isSubclassOf("I"), $r->implementsInterface("I"), is_subclass_of("K", "I"), (new ReflectionClass("K"))->isSubclassOf("I")); }

?>
--EXPECT--
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
bool(true)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
