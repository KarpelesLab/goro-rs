--TEST--
A reference to a WeakMap value binds the stored value
--FILE--
<?php
$wm = new WeakMap;
$o = new stdClass;
$wm[$o] = [1];
$wm[$o][] = 2;
$r = &$wm[$o];
$r[] = 3;
var_dump($wm[$o]);
$r = 'replaced';
var_dump($wm[$o], count($wm));
foreach ($wm as $k => $v) { var_dump($v); }
unset($r);
$wm[$o] = 5;
var_dump($wm[$o]);
$o2 = new stdClass;
try { $x = &$wm[$o2]; } catch (Error $e) { echo $e->getMessage(), "\n"; }
function addTo(array &$a) { $a[] = 'x'; }
$wm[$o2] = [];
addTo($wm[$o2]);
var_dump($wm[$o2]);
var_dump($wm);
--EXPECT--
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
string(8) "replaced"
int(1)
string(8) "replaced"
int(5)
Object stdClass#3 not contained in WeakMap
array(1) {
  [0]=>
  string(1) "x"
}
object(WeakMap)#1 (2) {
  [0]=>
  array(2) {
    ["key"]=>
    object(stdClass)#2 (0) {
    }
    ["value"]=>
    int(5)
  }
  [1]=>
  array(2) {
    ["key"]=>
    object(stdClass)#3 (0) {
    }
    ["value"]=>
    &array(1) {
      [0]=>
      string(1) "x"
    }
  }
}
