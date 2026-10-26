--TEST--
explode() / str_word_count() results over memory_limit: the fatal error from inside the call
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
var_dump(explode(",", "a,b,c", -1), explode("ab", "xabyabz", 2), str_word_count("a b-c 'd", 2));
$s = str_repeat(",", 50000000);
function f($s) { return explode(",", $s); }
f($s);
echo "not reached\n";
--EXPECTF--
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  string(4) "yabz"
}
array(3) {
  [0]=>
  string(1) "a"
  [2]=>
  string(3) "b-c"
  [6]=>
  string(2) "'d"
}

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate %d bytes) in %s on line 4
Stack trace:
#0 %s(4): explode(',', ',,,,,,,,,,,,,,,...')
#1 %s(5): f(',,,,,,,,,,,,,,,...')
#2 {main}
