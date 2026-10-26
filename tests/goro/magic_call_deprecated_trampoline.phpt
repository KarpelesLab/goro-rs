--TEST--
Deprecated __call()/__callStatic(): magic calls name the declaring class; trampolines also report the magic method
--FILE--
<?php
class K { #[\Deprecated("magic")] function __call($n, $a) { return 1; } #[\Deprecated] static function __callStatic($n, $a) { return 2; } }
class L extends K {}
$l = new L; $l->zz(); L::ss(); $f = $l->yy(...); $f(); $g = L::tt(...); $g(); $h = Closure::fromCallable([$l, "ww"]); $h();class K2 { static function __callStatic($n, $a) { return "s$n"; } }
class L2 extends K2 {}
var_dump(call_user_func(K2::cc(...), 9));
var_dump(call_user_func(L2::cc(...), 9));

?>
--EXPECTF--

Deprecated: Method K::zz() is deprecated, magic in %s on line 4

Deprecated: Method K::ss() is deprecated in %s on line 4

Deprecated: Method K::yy() is deprecated, magic in %s on line 4

Deprecated: Method K::__call() is deprecated, magic in %s on line 4

Deprecated: Method K::tt() is deprecated in %s on line 4

Deprecated: Method K::__callStatic() is deprecated in %s on line 4

Deprecated: Method K::ww() is deprecated, magic in %s on line 4

Deprecated: Method K::__call() is deprecated, magic in %s on line 4
string(3) "scc"
string(3) "scc"
