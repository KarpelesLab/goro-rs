--TEST--
Non-static method X::m() cannot be called statically names the declaring class; internal ones fail before their arguments
--FILE--
<?php
function a() { echo "arg evaluated\n"; return 1; }
class A { function m(){} }
class B extends A {}
try { B::m(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { RecursiveArrayIterator::count(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { DateTime::format(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$f = "B::m"; try { $f(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { call_user_func("B::m"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class X { function t() { B::m(); } }
try { (new X)->t(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Y { function t() { RecursiveArrayIterator::count(); } }
try { (new Y)->t(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
Non-static method A::m() cannot be called statically
Non-static method ArrayIterator::count() cannot be called statically
Non-static method DateTime::format() cannot be called statically
Non-static method A::m() cannot be called statically
call_user_func(): Argument #1 ($callback) must be a valid callback, non-static method B::m() cannot be called statically
Non-static method A::m() cannot be called statically
Non-static method ArrayIterator::count() cannot be called statically
