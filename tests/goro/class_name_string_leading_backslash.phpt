--TEST--
A class name string with a leading "\" names the class: $c::CONST, $c::method(), property_exists(), get_class_methods() (autoloaded too)
--FILE--
<?php
namespace N;
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if ($c === 'P\X') { eval('namespace P; class X { const K = 5; public static $s = 6; static function m() { return 7; } }'); }
    if ($c === 'P\Y') { eval('namespace P; class Y { const K = 8; }'); }
    if ($c === 'P\W') { eval('namespace P; class W { static function m() { return 10; } }'); }
});
class Loc { const K = 1; public static $s = 2; static function m() { return 3; } }
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
$c = '\\N\\Loc';
var_dump($c::K, $c::$s, $c::m());
t(function () use ($c) { return $c::NOPE; });
t(function () use ($c) { return $c::nope(); });
$c = '\\P\\X';
var_dump($c::m());
$c = '\\P\\W';
var_dump($c::m());
$c = '\\P\\Y';
var_dump($c::K);
$c = '\\Nope';
try { $c::K; } catch (\Error $e) { echo $e->getMessage(), "\n"; }
try { $c::m(); } catch (\Error $e) { echo $e->getMessage(), "\n"; }
var_dump(\property_exists('\\N\\Loc', 's'), \get_class_methods('\\N\\Loc'));
?>
--EXPECT--
int(1)
int(2)
int(3)
Error: Undefined constant N\Loc::NOPE
Error: Call to undefined method N\Loc::nope()
autoload(P\X)
int(7)
autoload(P\W)
int(10)
autoload(P\Y)
int(8)
autoload(Nope)
Class "\Nope" not found
autoload(Nope)
Class "\Nope" not found
bool(true)
array(1) {
  [0]=>
  string(1) "m"
}
