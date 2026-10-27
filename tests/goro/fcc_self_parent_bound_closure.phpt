--TEST--
parent::m(...) / self::m(...) in a closure compiled outside a class resolve in its bound scope
--FILE--
<?php
class P { public static function s() { return static::class; } }
class C extends P { }
class D extends C {}
$f = (function() { return parent::s(...); })->bindTo(new D, C::class)();
var_dump($f());
$g = (static function() { return self::s(...); })->bindTo(null, D::class)();
var_dump($g());
$h = function() { return parent::s(...); };
try { $h(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$i = Closure::bind(function() { return parent::s(...); }, null, P::class);
try { $i(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$j = function() { return self::s(...); };
try { $j(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
string(1) "D"
string(1) "D"
Cannot access "parent" when no class scope is active
Cannot access "parent" when current class scope has no parent
Cannot access "self" when no class scope is active
