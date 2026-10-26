--TEST--
__TRAIT__ in trait property, static property and constant defaults
--FILE--
<?php
namespace NS;
trait T { public $t = __TRAIT__; public static $s = __TRAIT__ . '!'; const X = __TRAIT__; public $c = __CLASS__; function f($a = __TRAIT__) { return [$a, __TRAIT__]; } }
class C { use T; public $own = __TRAIT__; }
var_dump(new C, C::$s, C::X, (new C)->f());
?>
--EXPECT--
object(NS\C)#1 (3) {
  ["own"]=>
  string(0) ""
  ["t"]=>
  string(4) "NS\T"
  ["c"]=>
  string(4) "NS\C"
}
string(5) "NS\T!"
string(4) "NS\T"
array(2) {
  [0]=>
  string(4) "NS\T"
  [1]=>
  string(4) "NS\T"
}
