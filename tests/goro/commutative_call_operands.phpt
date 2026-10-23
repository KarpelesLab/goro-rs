--TEST--
Operand order of commutative operators with call, cast and ternary operands
--FILE--
<?php
// Commutative operators swap a CONST/TMP op1 with a higher-typed op2; the
// results of specially compiled and frameless calls are TMP_VARs, casts
// and ternaries are never constant.
$e = ''; $arr = [1];
$tests = [
    fn() => 1 | $e,
    fn() => $e | 1,
    fn() => $arr | 'x',
    fn() => 'x' | $arr,
    fn() => (1 | 0) | strval($e),
    fn() => (array)1 & gettype(1),
    fn() => (0 ? "a" : [1]) ^ (PHP_INT_MAX << "1e3"),
    fn() => ((string)[1] | (PHP_INT_MAX << 1)),
    fn() => ("1e3" | (array)true),
    fn() => (1 + 1) * strtoupper('x'),
    fn() => (1 + 1) * trim('x'),
    fn() => (1 + 1) * \trim('x'),
    fn() => (1 + 1) * implode(',', [[]]),
];
foreach ($tests as $f) {
    try { var_dump($f()); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
}
--EXPECTF--
TypeError: Unsupported operand types: string | int
TypeError: Unsupported operand types: string | int
TypeError: Unsupported operand types: array | string
TypeError: Unsupported operand types: array | string
TypeError: Unsupported operand types: string | int
TypeError: Unsupported operand types: array & string
TypeError: Unsupported operand types: array ^ int

Warning: Array to string conversion in %s on line 14
TypeError: Unsupported operand types: string | int
TypeError: Unsupported operand types: array | string
TypeError: Unsupported operand types: string * int
TypeError: Unsupported operand types: string * int
TypeError: Unsupported operand types: string * int

Warning: Array to string conversion in %s on line 19
TypeError: Unsupported operand types: string * int
