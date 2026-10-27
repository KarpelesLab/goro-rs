--TEST--
A backed enum case fetched in a constant expression (property/param default) creates only that case
--FILE--
<?php
enum F: string { case X = 'x'; case Y = 'y'; }
class K { public $p = F::Y; }
class K2 { const C = F::Y; }
class K3 { public static $s = F::Y; }
function id($x) { return spl_object_id($x); }
$k = new K; echo id($k), " ", id($k->p), "\n";
echo id(F::X), "\n";
enum G: int { case X = 1; case Y = 2; }
echo id(K2::C === null ? null : constant('G::Y')), "\n";
echo id(G::X), "\n";
enum H: int { case X = 1; case Y = 2; }
function f($a = H::Y) { return $a; }
echo id(f()), " ", id(H::X), "\n";
enum I: int { case X = 1; case Y = 2; }
echo id(K3::$s), " ", id(I::Y), " ", id(I::X), "\n";
--EXPECT--
2 1
3
4
5
6 7
1 9 8
