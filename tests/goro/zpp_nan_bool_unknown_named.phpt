--TEST--
zpp: NAN given to a bool parameter warns; unknown named args to variadic internal functions
--FILE--
<?php
$n = NAN;
var_dump(in_array(1, [1], $n));
var_dump(array_keys([1, "1", 2], 1, $n));
try { echo sprintf(format: "%s", values: 1), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo max(value: 1, values: 2), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo pack(format: "C", values: 1), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo sprintf(values: 1, format: "%s"), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo sprintf(values: 1), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo sprintf("%s", 1, x: 2), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo printf(format: "%s\n", values: 1), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { echo array_push(values: 1), "\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo sprintf(format: "ok\n");
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 3
bool(true)

Warning: unexpected NAN value was coerced to bool in %s on line 4
array(1) {
  [0]=>
  int(0)
}
ArgumentCountError: sprintf() does not accept unknown named parameters
ArgumentCountError: max() does not accept unknown named parameters
ArgumentCountError: pack() does not accept unknown named parameters
ArgumentCountError: sprintf() does not accept unknown named parameters
ArgumentCountError: sprintf() expects at least 1 argument, 0 given
ArgumentCountError: sprintf() does not accept unknown named parameters
ArgumentCountError: printf() does not accept unknown named parameters
ArgumentCountError: array_push() expects at least 1 argument, 0 given
ok
