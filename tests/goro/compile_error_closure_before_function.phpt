--TEST--
A closure's compile error comes before a later function's
--FILE--
<?php
// never / void
function v(): void { return; }
var_dump(v());
function nv(): never { throw new Exception("x"); }
try { nv(); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
function nv2(): never { echo "falls\n"; }
try { nv2(); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
function nv3(): never { exit("bye from never\n"); }
class C { function m(): void {} static function s(): never { throw new LogicException("s"); } }
var_dump((new C)->m());
try { C::s(); } catch (LogicException $e) { echo $e->getMessage(), "\n"; }
$f = function(): void { yield 1; };
function gv(): never { yield 1; }
nv3();
--EXPECTF--

Fatal error: Generator return type must be a supertype of Generator, void given in %s on line 13
