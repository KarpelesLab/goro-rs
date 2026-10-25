--TEST--
BcMath\Number: sort() comparisons; GMP on the left uses GMP's handlers
--FILE--
<?php
use BcMath\Number;
function t($f) { try { $r = $f(); var_dump($r instanceof GMP || $r instanceof Number ? get_class($r) . ":$r" : $r); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// sort() and friends compare BcMath\Number through its compare handler
$a = [new Number("3"), "2", 1, new Number("1.5")]; sort($a); echo implode(",", $a), "\n";
$a = [new Number("1.5"), 1]; sort($a); echo implode(",", $a), "\n";
$a = [1, new Number("1.5")]; rsort($a); echo implode(",", $a), "\n";
$a = ["b" => new Number("0.5"), "a" => new Number("-1")]; asort($a); echo implode(",", array_keys($a)), "\n";
// GMP on the left of a BcMath\Number: GMP's handlers run first
$g = gmp_init(7);
$n = new Number("2.5");
t(fn() => $g <=> $n);
t(fn() => $n <=> $g);
t(fn() => $g + $n);
t(fn() => $n + $g);
t(fn() => $g == $n);
t(fn() => max($g, $n));
--EXPECT--
1,1.5,2,3
1,1.5
1.5,1
a,b
TypeError: Number must be of type GMP|string|int, BcMath\Number given
int(1)
TypeError: Number must be of type GMP|string|int, BcMath\Number given
TypeError: Unsupported operand types: BcMath\Number + GMP
TypeError: Number must be of type GMP|string|int, BcMath\Number given
TypeError: Number must be of type GMP|string|int, BcMath\Number given
