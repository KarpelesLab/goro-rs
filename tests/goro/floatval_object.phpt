--TEST--
floatval()/doubleval() of an object warn like zval_get_double()
--FILE--
<?php
var_dump(intval(new stdClass), floatval(new stdClass), doubleval(new stdClass), boolval(new stdClass), (float)new stdClass, (int)new stdClass);
var_dump(intval(new ArrayObject), floatval(new ArrayObject([1])), (float)new ArrayObject);
$o = new stdClass;
var_dump($o + 1);
--EXPECTF--

Warning: Object of class stdClass could not be converted to int in %s on line 2

Warning: Object of class stdClass could not be converted to float in %s on line 2

Warning: Object of class stdClass could not be converted to float in %s on line 2

Warning: Object of class stdClass could not be converted to float in %s on line 2

Warning: Object of class stdClass could not be converted to int in %s on line 2
int(1)
float(1)
float(1)
bool(true)
float(1)
int(1)

Warning: Object of class ArrayObject could not be converted to int in %s on line 3

Warning: Object of class ArrayObject could not be converted to float in %s on line 3

Warning: Object of class ArrayObject could not be converted to float in %s on line 3
int(1)
float(1)
float(1)

Fatal error: Uncaught TypeError: Unsupported operand types: stdClass + int in %s:5
Stack trace:
#0 {main}
  thrown in %s on line 5
