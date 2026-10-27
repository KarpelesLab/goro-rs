--TEST--
ReflectionClass::getMethods() methods invoke static methods with the reflected class as the called scope
--FILE--
<?php
trait T { public static function bootT() { echo "T: ", static::class, "\n"; } }
class A { use T; public static function s() { echo "s: ", static::class, "\n"; } }
class B extends A {}
foreach ((new ReflectionClass("B"))->getMethods() as $m) { echo $m->class, " "; $m->invoke(null); $m->invokeArgs(null, []); }
$m = new ReflectionMethod("B", "s"); $m->invoke(null);

(new ReflectionClass("B"))->getMethod("s")->invoke(null);
echo (new ReflectionClass("B"))->getMethods()[0];
var_dump((new ReflectionClass("B"))->getMethods()[0]);

?>
--EXPECTF--
A s: B
s: B
A T: B
T: B
s: B
s: B
Method [ <user, inherits A> static public method s ] {
  @@ %s 3 - 3
}
object(ReflectionMethod)#4 (2) {
  ["name"]=>
  string(1) "s"
  ["class"]=>
  string(1) "A"
}
