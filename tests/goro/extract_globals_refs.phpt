--TEST--
extract(): GLOBALS key, EXTR_REFS variables, frame of the thrown Error
--FILE--
<?php
// extract(): a "GLOBALS" key creates a local variable, EXTR_REFS binds
// new variables by reference, and the Error for "this" is thrown from
// inside extract() (its frame is in the trace).
function f($flag) {
    $a = 'orig';
    $src = ['a' => 1, 'c' => 3, '0' => 'zero', 'GLOBALS' => 'G', 'this' => 'T'];
    try {
        var_dump(extract($src, $flag, 'pfx'));
    } catch (\Throwable $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
        echo $e->getTraceAsString(), "\n";
    }
    $vars = get_defined_vars();
    unset($vars['src'], $vars['flag'], $vars['e']);
    ksort($vars);
    var_dump($vars);
    var_dump($src);
}
f(EXTR_REFS);
f(EXTR_PREFIX_ALL | EXTR_REFS);
f(EXTR_SKIP);
f(EXTR_PREFIX_ALL);
f(EXTR_OVERWRITE);
--EXPECTF--
Error: Cannot re-assign $this
#0 %s(9): extract(Array, 256, 'pfx')
#1 %s(20): f(256)
#2 {main}
array(3) {
  ["GLOBALS"]=>
  &string(1) "G"
  ["a"]=>
  &int(1)
  ["c"]=>
  &int(3)
}
array(5) {
  ["a"]=>
  &int(1)
  ["c"]=>
  &int(3)
  [0]=>
  string(4) "zero"
  ["GLOBALS"]=>
  &string(1) "G"
  ["this"]=>
  string(1) "T"
}
int(5)
array(6) {
  ["a"]=>
  string(4) "orig"
  ["pfx_0"]=>
  &string(4) "zero"
  ["pfx_GLOBALS"]=>
  &string(1) "G"
  ["pfx_a"]=>
  &int(1)
  ["pfx_c"]=>
  &int(3)
  ["pfx_this"]=>
  &string(1) "T"
}
array(5) {
  ["a"]=>
  &int(1)
  ["c"]=>
  &int(3)
  [0]=>
  &string(4) "zero"
  ["GLOBALS"]=>
  &string(1) "G"
  ["this"]=>
  &string(1) "T"
}
int(2)
array(3) {
  ["GLOBALS"]=>
  string(1) "G"
  ["a"]=>
  string(4) "orig"
  ["c"]=>
  int(3)
}
array(5) {
  ["a"]=>
  int(1)
  ["c"]=>
  int(3)
  [0]=>
  string(4) "zero"
  ["GLOBALS"]=>
  string(1) "G"
  ["this"]=>
  string(1) "T"
}
int(5)
array(6) {
  ["a"]=>
  string(4) "orig"
  ["pfx_0"]=>
  string(4) "zero"
  ["pfx_GLOBALS"]=>
  string(1) "G"
  ["pfx_a"]=>
  int(1)
  ["pfx_c"]=>
  int(3)
  ["pfx_this"]=>
  string(1) "T"
}
array(5) {
  ["a"]=>
  int(1)
  ["c"]=>
  int(3)
  [0]=>
  string(4) "zero"
  ["GLOBALS"]=>
  string(1) "G"
  ["this"]=>
  string(1) "T"
}
Error: Cannot re-assign $this
#0 %s(9): extract(Array, 0, 'pfx')
#1 %s(24): f(0)
#2 {main}
array(3) {
  ["GLOBALS"]=>
  string(1) "G"
  ["a"]=>
  int(1)
  ["c"]=>
  int(3)
}
array(5) {
  ["a"]=>
  int(1)
  ["c"]=>
  int(3)
  [0]=>
  string(4) "zero"
  ["GLOBALS"]=>
  string(1) "G"
  ["this"]=>
  string(1) "T"
}
