--TEST--
var_export() of an object with hooked properties keeps the object's handle
--FILE--
<?php
class A { public $v = 1 { get => $this->v; } }
$a = new A; $s = var_export($a, true); $b = new A;
echo spl_object_id($a), ' ', spl_object_id($b), "\n";
echo $s, "\n";
--EXPECTF--
1 2
\A::__set_state(array(
   'v' => 1,
))
