--TEST--
isset(C::$p[k]) fetches the static property quietly: an uninitialized typed, undeclared or inaccessible one is not set
--FILE--
<?php
trait T { protected static array $all; function f($n) { var_dump(isset(static::$all[$n]), isset(self::$all[$n]), isset(static::$all), empty(static::$all[$n]), static::$all[$n] ?? "dflt"); return static::$all[$n] = [1]; } }
class A { use T; }
class B { protected static array $x; static function g() { var_dump(isset(self::$x["a"]), isset(B::$x["a"]["b"]), self::$x["a"] ?? 1); } }
var_dump((new A)->f("k"));
B::g();
class P { private static $priv = ["a" => 1]; public static ?array $n = null; public static array $set = ["a" => ["b" => 2]]; }
var_dump(isset(P::$priv["a"]), isset(P::$nope["a"]), isset(P::$n["a"]), isset(P::$set["a"]["b"]), isset(P::$set["a"]["c"]), isset(P::$set["x"]["b"]));

?>
--EXPECT--
bool(false)
bool(false)
bool(false)
bool(true)
string(4) "dflt"
array(1) {
  [0]=>
  int(1)
}
bool(false)
bool(false)
int(1)
bool(false)
bool(false)
bool(false)
bool(true)
bool(false)
bool(false)
