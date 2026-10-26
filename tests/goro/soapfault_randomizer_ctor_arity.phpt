--TEST--
SoapFault / Random\Randomizer constructors check their argument count first
--FILE--
<?php
foreach ([fn() => new SoapFault(new stdClass), fn() => new SoapFault("a"), fn() => new SoapFault("a","b",1,2,3,4,5,6), fn() => new Random\Randomizer(null, 1), fn() => (new SoapFault("Server", "m"))->getMessage()] as $f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
--EXPECT--
ArgumentCountError: SoapFault::__construct() expects at least 2 arguments, 1 given
ArgumentCountError: SoapFault::__construct() expects at least 2 arguments, 1 given
ArgumentCountError: SoapFault::__construct() expects at most 7 arguments, 8 given
ArgumentCountError: Random\Randomizer::__construct() expects at most 1 argument, 2 given
string(1) "m"
