--TEST--
newInstanceWithoutConstructor() on GMP, BcMath\Number, Random and HashContext classes
--FILE--
<?php
foreach (['GMP', 'BcMath\Number', 'Random\Randomizer', 'Random\Engine\Mt19937', 'Random\Engine\PcgOneseq128XslRr64', 'Random\Engine\Xoshiro256StarStar', 'Random\Engine\Secure', 'HashContext'] as $c) {
  try { var_dump((new ReflectionClass($c))->newInstanceWithoutConstructor()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECT--
ReflectionException: Class GMP is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class BcMath\Number is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Random\Randomizer is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Random\Engine\Mt19937 is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Random\Engine\PcgOneseq128XslRr64 is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Random\Engine\Xoshiro256StarStar is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class Random\Engine\Secure is an internal class marked as final that cannot be instantiated without invoking its constructor
ReflectionException: Class HashContext is an internal class marked as final that cannot be instantiated without invoking its constructor
