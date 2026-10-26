--TEST--
BcMath\Number: write fetches go through read_property (get_property_ptr_ptr is NULL)
--FILE--
<?php
$n = new BcMath\Number("1.5");
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(function () use ($n) { $n->b .= "1"; });
t(function () use ($n) { $n->b++; });
t(function () use ($n) { $n->b[] = 1; var_dump($n); });
t(function () use ($n) { $n->b['x'] = 1; });
t(function () use ($n) { $n->value[] = 1; });
t(function () use ($n) { $n->value[0] = 1; });
t(function () use ($n) { $n->value++; });
t(function () use ($n) { $n->value .= "1"; });
t(function () use ($n) { $r = &$n->value; $r = 5; var_dump($n->value); });
t(function () use ($n) { $r = &$n->b; $r = 5; });
t(function () use ($n) { $n->scale = 3; });
t(function () use ($n) { $n->b = 3; });
t(function () use ($n) { unset($n->value); });
t(function () use ($n) { unset($n->b); });
t(function () use ($n) { var_dump(isset($n->value), isset($n->b), empty($n->scale)); });
var_dump($n);
--EXPECTF--

Warning: Undefined property: BcMath\Number::$b in %s on line 4
Error: Cannot create dynamic property BcMath\Number::$b

Warning: Undefined property: BcMath\Number::$b in %s on line 5
Error: Cannot create dynamic property BcMath\Number::$b

Warning: Undefined property: BcMath\Number::$b in %s on line 6
object(BcMath\Number)#1 (2) {
  ["value"]=>
  string(3) "1.5"
  ["scale"]=>
  int(1)
}

Warning: Undefined property: BcMath\Number::$b in %s on line 7
Error: [] operator not supported for strings
Error: Cannot modify readonly property BcMath\Number::$value
Error: Cannot modify readonly property BcMath\Number::$value
string(3) "1.5"

Warning: Undefined property: BcMath\Number::$b in %s on line 13
Error: Cannot modify readonly property BcMath\Number::$scale
Error: Cannot create dynamic property BcMath\Number::$b
Error: Cannot unset readonly property BcMath\Number::$value
bool(true)
bool(false)
bool(false)
object(BcMath\Number)#1 (2) {
  ["value"]=>
  string(3) "1.5"
  ["scale"]=>
  int(1)
}
