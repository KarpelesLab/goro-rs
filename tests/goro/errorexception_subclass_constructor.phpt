--TEST--
A subclass of ErrorException runs ErrorException's constructor
--FILE--
<?php
class E3 extends ErrorException {}
$e = new E3('x', 0, 4);
var_dump($e->getSeverity(), $e->getPrevious());
class E2 extends ErrorException { function __construct() { parent::__construct('m', 1, E_NOTICE, 'ff', 9); } }
$e = new E2; echo $e->getSeverity(), $e->getFile(), $e->getLine(), "\n";
$e = new E3('y', 2, E_WARNING, 'fn', 5, new Exception('p'));
echo $e->getSeverity(), $e->getFile(), $e->getLine(), get_class($e->getPrevious()), "\n";
class E4 extends E3 {}
echo (new E4('z', 0, 16))->getSeverity(), "\n";
--EXPECT--
int(4)
NULL
8ff9
2fn5Exception
16
