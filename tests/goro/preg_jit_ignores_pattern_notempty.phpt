--TEST--
pcre.jit: a leading (*NOTEMPTY)/(*NOTEMPTY_ATSTART) is ignored by pcre2_jit_match(), honoured by pcre2_match()
--FILE--
<?php
// JIT (pcre.jit=1): php calls pcre2_jit_match() directly, which ignores a
// leading (*NOTEMPTY) / (*NOTEMPTY_ATSTART); pcre2_match() honours them.
var_dump(preg_match('/(*NOTEMPTY)/', 'abc', $m), $m);
var_dump(preg_match('/(*NOTEMPTY)x?/', 'abc', $m, PREG_OFFSET_CAPTURE), $m);
var_dump(preg_match('/(*NOTEMPTY)/u', 'abc'));
var_dump(preg_match_all('/(*NOTEMPTY_ATSTART)x?/', 'ab', $m), $m);
var_dump(preg_replace('/(*NOTEMPTY)x?/', '-', 'ab'));
var_dump(preg_split('/(*NOTEMPTY)/', 'ab'));
var_dump(preg_match('/(*NO_JIT)(*NOTEMPTY)/', 'abc'));
ini_set('pcre.jit', 0);
var_dump(preg_match('/(*NOTEMPTY)y?/', 'abc'));
?>
--EXPECT--
int(1)
array(1) {
  [0]=>
  string(0) ""
}
int(1)
array(1) {
  [0]=>
  array(2) {
    [0]=>
    string(0) ""
    [1]=>
    int(0)
  }
}
int(0)
int(3)
array(1) {
  [0]=>
  array(3) {
    [0]=>
    string(0) ""
    [1]=>
    string(0) ""
    [2]=>
    string(0) ""
  }
}
string(5) "-a-b-"
array(4) {
  [0]=>
  string(0) ""
  [1]=>
  string(1) "a"
  [2]=>
  string(1) "b"
  [3]=>
  string(0) ""
}
int(0)
int(0)
