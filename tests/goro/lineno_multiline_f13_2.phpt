--TEST--
Multi-line argument unpacking: SEND_UNPACK after the unpacked expression (f13_2)
--FILE--
<?php
class K { public $p; public int $ti = 0; public static $sp; const C = 1; function m($a = 0, $b = 0) { return $this; } static function s($a = 0, $b = 0) {} function __toString(): string { return "K"; } }
function uf($a = 0, $b = 0) { return $a; }
function thrower() { throw new Exception("t"); }
$o = new K; $n = null; $arr = []; $str = "abc"; $i = 5;
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
t(fn()
    => uf(
    ...
    [
    'zz' =>
    1
    ]
    ));
echo "\n-done-\n";
--EXPECT--
Error: Unknown named parameter $zz @12

-done-
