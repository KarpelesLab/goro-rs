--TEST--
??= evaluates its target's keys once, in order
--FILE--
<?php
function k($n) { echo "k($n) "; return $n; }
#[AllowDynamicProperties]
class O { public $p; public static $s; public static array $t; function __get($n) { echo "__get($n) "; return null; } }
$a = null;
$a[k(1)][k(2)] ??= 'v'; echo json_encode($a), "\n";
$a[k(1)][k(2)] ??= 'w'; echo json_encode($a), "\n";
$o = new O;
$o->p[k(3)] ??= 'x'; echo json_encode($o->p), "\n";
$o->{k('q')} ??= 'y'; echo json_encode($o), "\n";
O::$s[k(4)][k(5)] ??= 'z'; echo json_encode(O::$s), "\n";
O::$t[k(6)] ??= 'z'; echo json_encode(O::$t), "\n";
$u = null;
try { $u->{k("m")}[k(7)] ??= 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo json_encode($u), "\n";

?>
--EXPECT--
k(1) k(2) {"1":{"2":"v"}}
k(1) k(2) {"1":{"2":"v"}}
k(3) {"3":"x"}
k(q) __get(q) {"p":{"3":"x"},"q":"y"}
k(4) k(5) {"4":{"5":"z"}}
k(6) {"6":"z"}
k(m) k(7) Attempt to modify property "m" on null
null
