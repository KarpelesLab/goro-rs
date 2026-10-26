--TEST--
Comparing an object with a resource: the object is greater
--FILE--
<?php
$o = new stdClass; $r = STDIN;
var_dump($r < $o, $o > $r, $r == $o, $r <=> $o, $o <=> $r, $o <=> [1], [1] <=> $o, $o <=> "a", "a" <=> $o, $o <=> null, null <=> $o);
var_dump(max($r, $o), max($o, $r), min($r, $o), min($o, $r), max([$r, $o]), max([1], $o), max($o, [1]), max("abc", $o));
$a = [$o, $r, 5, "x", [1]]; sort($a); var_dump($a);
--EXPECTF--
bool(true)
bool(true)
bool(false)
int(-1)
int(1)
int(1)
int(-1)
int(1)
int(-1)
int(1)
int(-1)
object(stdClass)#1 (0) {
}
object(stdClass)#1 (0) {
}
resource(1) of type (stream)
resource(1) of type (stream)
object(stdClass)#1 (0) {
}
object(stdClass)#1 (0) {
}
object(stdClass)#1 (0) {
}
object(stdClass)#1 (0) {
}

Notice: Object of class stdClass could not be converted to int in %s on line 5
array(5) {
  [0]=>
  resource(1) of type (stream)
  [1]=>
  object(stdClass)#1 (0) {
  }
  [2]=>
  int(5)
  [3]=>
  string(1) "x"
  [4]=>
  array(1) {
    [0]=>
    int(1)
  }
}
