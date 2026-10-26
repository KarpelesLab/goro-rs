--TEST--
Destructuring $GLOBALS fetches an undefined global named GLOBALS
--FILE--
<?php
$x = 5;
['x' => $v] = $GLOBALS; var_dump($v);
list('x' => $w) = $GLOBALS; var_dump($w);
function f() { ['x' => $v] = $GLOBALS; var_dump($v); }
f();
[$GLOBALS['q']] = [1]; var_dump($q);
foreach ([$GLOBALS] as ['x' => $z]) var_dump($z);
$r = (['x' => $v2] = $GLOBALS); var_dump(is_array($r), $v2);
?>
--EXPECTF--

Warning: Undefined global variable $GLOBALS in %s on line 3
NULL

Warning: Undefined global variable $GLOBALS in %s on line 4
NULL

Warning: Undefined global variable $GLOBALS in %s on line 5
NULL
int(1)
int(5)

Warning: Undefined global variable $GLOBALS in %s on line 9
bool(false)
NULL
