--TEST--
DateInterval::__unserialize() gives undeclared properties to a subclass's __set()
--FILE--
<?php
error_reporting(E_ALL);
set_error_handler(function ($no, $str) { echo "ERR $no $str\n"; return true; });
class S extends DateInterval {
    public $declared = 0;
    public function __set($n, $v) { echo "__set($n, ", json_encode($v), ")\n"; }
}
$s = new S('P1D');
$s->__unserialize(['y' => 1, 'm' => 0, 'd' => 0, 'h' => 0, 'i' => 0, 's' => 0, 'f' => 0.0, 'invert' => 0, 'days' => false, 'from_string' => false, " * foo" => 1, "declared" => 5]);
var_dump($s->declared, $s->y);
class X extends DateInterval {}
$x = new X('P1D');
$x->__unserialize(['y' => 1, 'm' => 0, 'd' => 0, 'h' => 0, 'i' => 0, 's' => 0, 'f' => 0.0, 'invert' => 0, 'days' => false, 'from_string' => false, "bar" => 2]);
var_dump($x->bar);

?>
--EXPECT--
__set( * foo, 1)
int(5)
int(1)
ERR 8192 Creation of dynamic property X::$bar is deprecated
int(2)
