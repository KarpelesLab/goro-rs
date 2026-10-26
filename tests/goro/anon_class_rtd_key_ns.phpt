--TEST--
Anonymous class names count the runtime definition keys of non-early-bound classes
--FILE--
<?php
namespace NS;
function n($o) { return strstr(get_class($o), '$'); }
function f() { return new class {}; }
class K { function m() { return new class {}; } }
interface J extends \Countable {}
class L extends K {}
class M extends Q {}
class Q {}
function g() { if (!class_exists('NS\Inner', false)) { class Inner {} } return new class {}; }
enum En: string { case A = 'a'; }
trait T1 {}
trait T2 { use T1; }
echo n(f()), n((new K)->m()), n(g()), n(new class extends K {}), "\n";
$c = function () { return new class {}; };
echo n($c()), "\n";
--EXPECT--
$0$1$5$8
$9
