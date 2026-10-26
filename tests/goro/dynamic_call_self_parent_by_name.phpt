--TEST--
Dynamic calls ("parent::m"(), ["parent", "m"](), $c::m(), FCCs) look self/parent/static up as class names
--FILE--
<?php
class A { function m() { return "A::m"; } static function s() { return "A::s " . static::class; } }
class B extends A {
  function m() { return "B::m"; }
  static function s() { return "B::s"; }
  function t() {
    try { var_dump("parent::s"()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(["parent", "s"]()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump("A::s"()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(parent::s()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(parent::m()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(call_user_func(["parent", "s"])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(call_user_func_array("parent::m", [])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { $f = "parent::s"; $g = $f(...); var_dump($g()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { $g = "parent::s"(...); var_dump($g()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(array_map("parent::s", [1])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    $c = "parent"; try { var_dump($c::s()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  }
}
(new B)->t();
function f() { try { $x = "self::s"; $x(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
f();
--EXPECTF--
Error: Class "parent" not found
Error: Class "parent" not found
string(6) "A::s A"
string(6) "A::s B"
string(4) "A::m"

Deprecated: Use of "parent" in callables is deprecated in %s on line 12
string(6) "A::s B"

Deprecated: Use of "parent" in callables is deprecated in %s on line 13
string(4) "A::m"
Error: Class "parent" not found
Error: Class "parent" not found

Deprecated: Use of "parent" in callables is deprecated in %s on line 16
array(1) {
  [0]=>
  string(6) "A::s B"
}
Error: Class "parent" not found
Error: Class "self" not found
