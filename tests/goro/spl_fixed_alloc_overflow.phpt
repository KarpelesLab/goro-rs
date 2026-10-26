--TEST--
SplFixedArray: an oversized allocation is fatal (integer overflow / memory limit)
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
--FILE--
<?php try { new SplFixedArray(PHP_INT_MAX); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } echo "after\n";
?>
--EXPECTF--

Fatal error: Possible integer overflow in memory allocation (9223372036854775807 * 16 + 0) in %s on line 1
