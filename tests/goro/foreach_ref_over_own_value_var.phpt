--TEST--
foreach ($v as &$v) keeps iterating the array after the variable is rebound to an element
--FILE--
<?php
function t() {
    $out = [];
    if ($v = ['a' => 1, 'b' => 2, 'c' => [3]]) {
        foreach ($v as $k => &$v) {
            $out[$k] = &$v;
        }
        unset($v);
    }
    var_dump($out);
}
t();
$v = [1, 2, 3];
foreach ($v as $k => &$v) { echo $k, ' '; }
echo "\n";
var_dump($v);
$w = [1, 2, 3];
foreach ($w as $k => $w) { echo $k, ' '; }
echo "\n";
var_dump($w);

?>
--EXPECT--
array(3) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  array(1) {
    [0]=>
    int(3)
  }
}
0 1 2 
int(3)
0 1 2 
int(3)
