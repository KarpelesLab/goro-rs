--TEST--
String offset reads and element overwrites
--FILE--
<?php
function run() {
    $s = "hello"; $o = "";
    for ($i = -6; $i < 7; $i++) { $o .= @$s[$i] . "|"; }
    echo $o, "\n";
    echo $s[0], $s[-1], "abc"[1], $s["1"], "\n";
    echo $s[5] ?? "none", "\n";
    try { echo $s[[]]; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    echo $s[1.7], "\n";
    echo $s[10], "|\n";
    $a = [[1,2],[3]]; $b = $a; $a[0] = [9]; var_dump($a, $b);
    $c = ['o' => new stdClass]; $c['o'] = 5; var_dump($c);
    $r = [1]; $x = &$r[0]; $r[0] = [7]; var_dump($x);
}
run();
--EXPECTF--
|h|e|l|l|o|h|e|l|l|o|||
hobe
none
Cannot access offset of type array on string

Warning: String offset cast occurred in %s on line 9
e

Warning: Uninitialized string offset 10 in %s on line 10
|
array(2) {
  [0]=>
  array(1) {
    [0]=>
    int(9)
  }
  [1]=>
  array(1) {
    [0]=>
    int(3)
  }
}
array(2) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
  [1]=>
  array(1) {
    [0]=>
    int(3)
  }
}
array(1) {
  ["o"]=>
  int(5)
}
array(1) {
  [0]=>
  int(7)
}
