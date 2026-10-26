--TEST--
strval() of a user subclass of an internal Stringable class (SplFileInfo)
--FILE--
<?php
class S extends SplFileInfo { function __construct($f, private $x = 1) { parent::__construct($f); } }
$a = [new S('/tmp/a'), new SplFileInfo('/tmp/b')];
var_dump(strval($a[0]));
var_dump(array_map('strval', $a));
var_dump(array_map(strval(...), $a));

?>
--EXPECT--
string(6) "/tmp/a"
array(2) {
  [0]=>
  string(6) "/tmp/a"
  [1]=>
  string(6) "/tmp/b"
}
array(2) {
  [0]=>
  string(6) "/tmp/a"
  [1]=>
  string(6) "/tmp/b"
}
