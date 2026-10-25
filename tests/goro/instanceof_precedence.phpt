--TEST--
instanceof binds tighter than the arithmetic, concat and pipe operators
--FILE--
<?php
$o = new stdClass;
var_dump($o instanceof stdClass . "x");
var_dump($o instanceof stdClass + 1);
var_dump($o instanceof stdClass - 1);
var_dump($o instanceof stdClass * 2);
var_dump($o instanceof stdClass % 2);
var_dump($o instanceof stdClass ** 2);
var_dump($o instanceof stdClass << 1);
var_dump($o instanceof stdClass >> 1);
var_dump($o instanceof stdClass |> strval(...));
var_dump($o instanceof stdClass == true);
var_dump(!$o instanceof stdClass);
$c = 'stdClass';
var_dump($o instanceof $c . "y");
var_dump($o instanceof $c + 1);
var_dump($o instanceof \stdClass . "z");
var_dump(2 * $o instanceof stdClass);
var_dump(2 ** $o instanceof stdClass);
var_dump((int)$o instanceof stdClass);
var_dump($o instanceof stdClass ** 2 instanceof stdClass);
var_dump(-$o instanceof stdClass);
?>
--EXPECTF--
string(2) "1x"
int(2)
int(0)
int(2)
int(1)
int(1)
int(2)
int(0)
string(1) "1"
bool(true)
bool(false)
string(2) "1y"
int(2)
string(2) "1z"
int(2)

Fatal error: Uncaught TypeError: Unsupported operand types: int ** stdClass in %s:19
Stack trace:
#0 {main}
  thrown in %s on line 19
