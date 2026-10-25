--TEST--
GMP: the number is internal state, shown as num only in debug output
--FILE--
<?php
// The number of a GMP object is internal state: only debug output shows `num`.
$a = gmp_init(42);
var_dump(get_object_vars($a), (array)$a, json_encode($a));
var_export($a); echo "\n";
var_dump(property_exists($a, 'num'));
foreach ($a as $k => $v) echo "$k=$v\n";
$x = gmp_init(3);
$x->dyn = 1;
var_dump($x);
print_r($x); echo "\n";
var_dump(json_encode($x), serialize($x));
// clones keep their number, and are independent of the original
$b = clone $a;
gmp_setbit($b, 0);
var_dump($a == $b, (string)$a, (string)$b, clone $a == $a);
$c = $a;
gmp_setbit($c, 2);
echo $a, "\n";
// comparisons outside the operators use the GMP compare handler
$arr = [gmp_init(5), gmp_init(-3), gmp_init(10), gmp_init(0)];
sort($arr);
echo implode(",", $arr), "\n";
var_dump(in_array(10, $arr), in_array(11, $arr), array_search(gmp_init(-3), $arr));
echo max($arr), " ", min($arr), "\n";
var_dump(intval(gmp_init("12345678901234567890")), boolval(gmp_init(0)), strval(gmp_init(-5)));
--EXPECTF--
array(0) {
}
array(0) {
}
string(2) "{}"
\GMP::__set_state(array(
))
bool(false)

Deprecated: Creation of dynamic property GMP::$dyn is deprecated in %s on line 9
object(GMP)#2 (2) {
  ["dyn"]=>
  int(1)
  ["num"]=>
  string(1) "3"
}
GMP Object
(
    [dyn] => 1
    [num] => 3
)

string(9) "{"dyn":1}"
string(50) "O:3:"GMP":2:{i:0;s:1:"3";i:1;a:1:{s:3:"dyn";i:1;}}"
bool(false)
string(2) "42"
string(2) "43"
bool(true)
46
-3,0,5,10
bool(true)
bool(false)
int(0)
10 -3
int(3122306864379792082)
bool(false)
string(2) "-5"
