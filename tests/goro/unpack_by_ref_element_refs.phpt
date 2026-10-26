--TEST--
Argument unpacking leaves by-reference bound elements as references; named by-reference parameters of internal functions bind
--FILE--
<?php
// ZEND_SEND_UNPACK leaves the elements bound by reference as references:
// call_user_func_array() then passes them by reference without a warning.
$fn = 'array_pop';
$args = [[1, 2]];
var_dump($fn(...$args));
var_dump(call_user_func_array($fn, $args));
$args = ['array' => [1, 2]];
var_dump($fn(...$args));
var_dump(call_user_func_array($fn, $args));
var_dump($args);
// named by-reference parameters of internal functions bind to the element
$a = ['pattern' => '/a/', 'subject' => 'a', 'matches' => null];
var_dump(preg_match(...$a), $a);
var_dump(call_user_func_array('preg_match', $a));
$a = ['subject' => 'aa', 'replacement' => 'b', 'pattern' => '/a/', 'count' => 0];
var_dump(preg_replace(...$a), $a['count']);
$a = ['/a/', 'a', null];
var_dump(preg_match(...$a), $a);
var_dump(call_user_func_array('preg_match', $a));
$copy = ['search' => 'a', 'replace' => 'b', 'subject' => 'aaa', 'count' => 0];
$orig = $copy;
str_replace(...$copy);
var_dump($copy['count'], $orig['count']);
--EXPECTF--
int(2)
int(1)
int(2)
int(1)
array(1) {
  ["array"]=>
  array(0) {
  }
}
int(1)
array(3) {
  ["pattern"]=>
  string(3) "/a/"
  ["subject"]=>
  string(1) "a"
  ["matches"]=>
  array(1) {
    [0]=>
    string(1) "a"
  }
}
int(1)
string(2) "bb"
int(2)
int(1)
array(3) {
  [0]=>
  string(3) "/a/"
  [1]=>
  string(1) "a"
  [2]=>
  array(1) {
    [0]=>
    string(1) "a"
  }
}
int(1)
int(3)
int(0)
