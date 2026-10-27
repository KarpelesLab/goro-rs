--TEST--
Write and unset fetches of a property named by an undefined variable warn
--FILE--
<?php
$o = new stdClass;
try { $o->{$u1}->s = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo "-\n";
$x = $o->{$u2}->s ?? 0;
echo "-\n";
$o = new stdClass; $o->{$u3}[] = 1;
echo "-\n";
$o = new stdClass; $o->{$u4}['k'] = 1;
echo "-\n";
try { $o->{$u5}->t[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo "-\n";
$o = new stdClass; $r = &$o->{$u6};
echo "-\n";
$o = new stdClass; $o->{$u7}++;
echo "-\n";
unset($o->{$u8});
echo "-\n";
var_dump(isset($o->{$u9}));
$a = [];
try { $a[$u10]->p = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo "-\n";
$o = new stdClass; $o->{$w1} .= 'x';
echo "-\n";
$o = new stdClass; $o->{$w2}['k'] .= 'x';
echo "-\n";
$o = new stdClass; $o->{$w3}->a ??= 1;
echo "-\n";
$o = new stdClass; foreach ([1] as $o->{$w4}) {}
echo "-\n";
$o = new stdClass; [$o->{$w5}] = [1];
echo "-\n";
$o = new stdClass; $o->{$w6}['a']['b'] = 2;
echo "-\n";
$o = new stdClass; g($o->{$w7});
function g(&$x) {}
echo "-\n";
$o = new stdClass; $o->{$w8}--;
echo "-\n";
var_dump(empty($o->{$w9}->x));
$o = new stdClass;
unset($o->{$v1});
$o->{''} = 1;
unset($o->{$v2});
var_dump($o);
class M { function __unset($n) { echo "__unset($n)\n"; } }
$m = new M;
unset($m->{$v3});
$a = ['' => new stdClass];
unset($a[$v4]->{$v5});
unset($a[$v6][$v7]);
$n = null;
unset($n->{$v8});
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 3
Attempt to assign property "s" on null
-

Warning: Undefined variable $u2 in %s on line 5
-

Warning: Undefined variable $u3 in %s on line 7
-

Warning: Undefined variable $u4 in %s on line 9
-

Warning: Undefined variable $u5 in %s on line 11
Attempt to modify property "t" on array
-

Warning: Undefined variable $u6 in %s on line 13
-

Warning: Undefined variable $u7 in %s on line 15

Warning: Undefined property: stdClass::$ in %s on line 15
-

Warning: Undefined variable $u8 in %s on line 17
-

Warning: Undefined variable $u9 in %s on line 19
bool(false)

Warning: Undefined variable $u10 in %s on line 21

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 21
Attempt to assign property "p" on null
-

Warning: Undefined variable $w1 in %s on line 23

Warning: Undefined property: stdClass::$ in %s on line 23
-

Warning: Undefined variable $w2 in %s on line 25

Warning: Undefined property: stdClass::$ in %s on line 25

Warning: Undefined array key "k" in %s on line 25
-

Warning: Undefined variable $w3 in %s on line 27

Warning: Undefined variable $w3 in %s on line 27

Fatal error: Uncaught Error: Attempt to assign property "a" on null in %s:27
Stack trace:
#0 {main}
  thrown in %s on line 27
