--TEST--
GMP TypeErrors name an anonymous class object as zend_zval_value_name() does (up to its NUL)
--EXTENSIONS--
gmp
--FILE--
<?php
$o = new class {};
try { var_dump(gmp_add($o, 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(gmp_init($o)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(gmp_strval(new class extends ArrayObject {})); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
TypeError: gmp_add(): Argument #1 ($num1) must be of type GMP|string|int, class@anonymous given
TypeError: gmp_init(): Argument #1 ($num) must be of type string|int, class@anonymous given
TypeError: gmp_strval(): Argument #1 ($num) must be of type GMP|string|int, ArrayObject@anonymous given
