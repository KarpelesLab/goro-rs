--TEST--
BcMath\Number methods parse their arguments per the caller's strict_types
--FILE--
<?php
declare(strict_types=1);
foreach ([true, 1.5, null, 'foo', 5, '1.5'] as $v) {
    try { $n = new BcMath\Number($v); echo "ok ", $n, "\n"; }
    catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { bcadd(true, '1'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }

?>
--EXPECT--
TypeError: BcMath\Number::__construct(): Argument #1 ($num) must be of type string|int, true given
TypeError: BcMath\Number::__construct(): Argument #1 ($num) must be of type string|int, float given
TypeError: BcMath\Number::__construct(): Argument #1 ($num) must be of type string|int, null given
ValueError: BcMath\Number::__construct(): Argument #1 ($num) is not well-formed
ok 5
ok 1.5
TypeError: bcadd(): Argument #1 ($num1) must be of type string, true given
