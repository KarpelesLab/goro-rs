--TEST--
Anonymous class name suffixes: classes bound at run time take keys too
--FILE--
<?php
function n($o) { return strstr(get_class($o), '$'); }
echo n(new class {}), "\n";
$f = function() {};
echo n(new class {}), "\n";
$g = fn() => 1;
echo n(new class {}), "\n";
class A {}
echo n(new class {}), "\n";
class B extends A {}
echo n(new class {}), "\n";
interface I {}
class C implements I {}
echo n(new class {}), "\n";
if (true) { function ff() {} }
echo n(new class {}), "\n";
trait T {}
class D { use T; }
echo n(new class {}), "\n";
enum E { case A; }
echo n(new class {}), "\n";
--EXPECT--
$0
$1
$2
$3
$4
$6
$7
$9
$b
