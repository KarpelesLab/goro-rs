--TEST--
Arguments unpacked into an internal function's by-reference parameters bind to the array's elements
--FILE--
<?php
$a = ["a", "b", "aaa", 5]; str_replace(...$a); var_dump($a[3]);
$f = "str_replace"; $b = ["a", "b", "aaa", 5]; $f(...$b); var_dump($b[3]);
$e = [[3, 1, 2]]; sort(...$e); var_dump($e);
$h = ["/(a)/", "xa", null]; preg_match(...$h); var_dump($h[2]);
$s = ["World", "Word", 0]; similar_text(...$s); var_dump($s[2]);
$sc = ["12 34", "%d %d", null, null]; var_dump(sscanf(...$sc), $sc[2], $sc[3]);
$p = [[1, 2], 3]; array_push(...$p); var_dump($p);
$k = ["a", "b", "aaa", 5]; call_user_func_array("str_replace", $k); var_dump($k[3]);
$m = ["/a/", "aa", null]; preg_match_all(...$m); var_dump($m[2]);
function g() { yield "a"; yield "b"; yield "aaa"; yield 5; }
var_dump(str_replace(...g()));
--EXPECTF--
int(3)
int(3)
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    int(2)
    [2]=>
    int(3)
  }
}
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "a"
}
float(88.88888888888889)
int(2)
int(12)
int(34)
array(2) {
  [0]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    int(2)
    [2]=>
    int(3)
  }
  [1]=>
  int(3)
}

Warning: str_replace(): Argument #4 ($count) must be passed by reference, value given in %s on line 9
int(5)
array(1) {
  [0]=>
  array(2) {
    [0]=>
    string(1) "a"
    [1]=>
    string(1) "a"
  }
}

Warning: Cannot pass by-reference argument 4 of str_replace() by unpacking a Traversable, passing by-value instead in %s on line 12
string(3) "bbb"
