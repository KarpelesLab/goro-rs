--TEST--
?? binds tighter than ?: ($c ? $x : $a["k"] ?? 0 does not warn; $a ?? $b ? $c : $d tests $a ?? $b) (termwind, via Laravel)
--FILE--
<?php
$a = []; $n = null; $t = true; $f = false; $z = 0;
var_dump($z > 0 ? $z : $a['k'] ?? 5);
var_dump($n ?? $f ? 'yes' : 'no');        // (null ?? false) ? : -> no
var_dump($n ?? $t ? 'yes' : 'no');        // yes
var_dump($a['x'] ?? 1 ? 'A' : 'B');       // A
var_dump($f ?: $a['q'] ?? 'dflt');        // dflt, no warning
var_dump($t ? $a['m'] ?? 'm' : 'other');  // m
var_dump($n ?? $n ?? 3);
var_dump(1 + 2 ?? 9);
var_dump($n ?? 1 + 2);
var_dump($n || $t ?? 'x');
$x = $n ?? $f ? 1 : 2; var_dump($x);
$y = $f ? 1 : $n ?? $b = 7; var_dump($y, $b);
var_dump((fn() => $n ?? 4 ? "c" : "d")());
echo ($z ? 'a' : $a['p'] ?? 'b') . "\n";
var_dump($n ?? print 'p');
echo "\n";
var_dump(isset($a) ? $a['k'] ?? 'i' : 'j');
var_dump($f ?: $f ?: $a['zz'] ?? 'chain');

?>
--EXPECT--
int(5)
string(2) "no"
string(3) "yes"
string(1) "A"
string(4) "dflt"
string(1) "m"
int(3)
int(3)
int(3)
bool(true)
int(2)
int(7)
int(7)
string(1) "c"
b
pint(1)

string(1) "i"
string(5) "chain"
