--TEST--
debug_zval_dump() of a backed enum case: its value is the declaration's literal
--FILE--
<?php
enum S: string { case H = "h"; case Long = "hello world"; }
enum I: int { case A = 5; }
debug_zval_dump(S::H, S::Long, I::A, [S::Long]);
--EXPECT--
object(S)#1 (2) refcount(2){
  ["name"]=>
  string(1) "H" interned
  ["value"]=>
  string(1) "h" interned
}
object(S)#2 (2) refcount(3){
  ["name"]=>
  string(4) "Long" interned
  ["value"]=>
  string(11) "hello world" refcount(2)
}
object(I)#3 (2) refcount(2){
  ["name"]=>
  string(1) "A" interned
  ["value"]=>
  int(5)
}
array(1) packed refcount(1){
  [0]=>
  object(S)#2 (2) refcount(3){
    ["name"]=>
    string(4) "Long" interned
    ["value"]=>
    string(11) "hello world" refcount(2)
  }
}
