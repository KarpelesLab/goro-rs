--TEST--
Dynamic calls of strings split Class::method at the last '::', drop a leading backslash of the class, and stop undefined function names at a NUL
--FILE--
<?php
class A { static function m() { return "A::m"; } }
foreach (["strlen\0x", "A:::m", "::m", "A::", "A::m", "\\A::m", "nope\0"] as $f) {
    try { var_dump($f()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
foreach (["\\B::m", "\\A::zz", ["\\A", "m"], ["\\B", "m"], "\\\\A::m"] as $f) {
    try { var_dump($f()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(call_user_func($f)); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
Error: Call to undefined function strlen()
Error: Class "A:" not found
Error: Class "" not found
Error: Call to undefined method A::()
string(4) "A::m"
string(4) "A::m"
Error: Call to undefined function nope()
Error: Class "\B" not found
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class "\B" not found
Error: Call to undefined method A::zz()
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class A does not have a method "zz"
string(4) "A::m"
string(4) "A::m"
Error: Class "\B" not found
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class "\B" not found
Error: Class "\\A" not found
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class "\\A" not found
