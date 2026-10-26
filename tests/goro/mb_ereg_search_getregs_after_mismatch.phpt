--TEST--
mb_ereg_search_getregs() is false after a failed mb_ereg_search*()
--FILE--
<?php
// php-src frees the search region when the search fails, so the registers
// of the previous successful search are gone too
mb_ereg_search_init("abc 123 def");
var_dump(mb_ereg_search("[a-z]+"), mb_ereg_search_getregs());
var_dump(mb_ereg_search_pos("\d+"), mb_ereg_search_getregs());
var_dump(mb_ereg_search("\d+"), mb_ereg_search_getpos(), mb_ereg_search_getregs());
var_dump(mb_ereg_search_setpos(0), mb_ereg_search_regs("(d)(e)"), mb_ereg_search_getregs());
?>
--EXPECT--
bool(true)
array(1) {
  [0]=>
  string(3) "abc"
}
array(2) {
  [0]=>
  int(4)
  [1]=>
  int(3)
}
array(1) {
  [0]=>
  string(3) "123"
}
bool(false)
int(11)
bool(false)
bool(true)
array(3) {
  [0]=>
  string(2) "de"
  [1]=>
  string(1) "d"
  [2]=>
  string(1) "e"
}
array(3) {
  [0]=>
  string(2) "de"
  [1]=>
  string(1) "d"
  [2]=>
  string(1) "e"
}
