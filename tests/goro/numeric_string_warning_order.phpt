--TEST--
Numeric-string diagnostics order; compile-time folded operands of commutative operators
--FILE--
<?php
// "A non-numeric value" comes before the float-string deprecation.
foreach ([fn() => "9.5x" % [1], fn() => "9.5x" << [], fn() => ("19.5x") & [1], fn() => "9.5x" % 2, fn() => 2 << "1.5x"] as $f) {
    try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
// operands folded at compile time are constants for the commutative swap
foreach ([fn() => (true != "1e3") * ((fn() => "x")()), fn() => true * ~"9.5x", fn() => ("a" . "b") | [], fn() => (false && f()) & []] as $f) {
    try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--

Warning: A non-numeric value encountered in %s on line 3

Deprecated: Implicit conversion from float-string "9.5x" to int loses precision in %s on line 3
TypeError: Unsupported operand types: string % array

Warning: A non-numeric value encountered in %s on line 3

Deprecated: Implicit conversion from float-string "9.5x" to int loses precision in %s on line 3
TypeError: Unsupported operand types: string << array

Warning: A non-numeric value encountered in %s on line 3

Deprecated: Implicit conversion from float-string "19.5x" to int loses precision in %s on line 3
TypeError: Unsupported operand types: string & array

Warning: A non-numeric value encountered in %s on line 3

Deprecated: Implicit conversion from float-string "9.5x" to int loses precision in %s on line 3
int(1)

Warning: A non-numeric value encountered in %s on line 3

Deprecated: Implicit conversion from float-string "1.5x" to int loses precision in %s on line 3
int(4)
TypeError: Unsupported operand types: string * bool
TypeError: Unsupported operand types: bool * string
TypeError: Unsupported operand types: string | array
TypeError: Unsupported operand types: bool & array
