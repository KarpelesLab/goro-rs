--TEST--
An interface may extend Throwable alongside other interfaces (symfony/dependency-injection ExceptionInterface)
--FILE--
<?php
// symfony/dependency-injection's ExceptionInterface
interface ContainerExceptionInterface extends Throwable {}
interface ExceptionInterface extends ContainerExceptionInterface, \Throwable {}
interface Plain extends \Throwable {}
interface Two extends Countable, \Throwable {}
class E extends Exception implements ExceptionInterface, Two { function count(): int { return 0; } }
$e = new E('m');
var_dump($e instanceof Throwable, $e instanceof ExceptionInterface, $e instanceof Two);
var_dump(interface_exists('Plain'), class_implements('E'));

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
array(6) {
  ["Stringable"]=>
  string(10) "Stringable"
  ["Throwable"]=>
  string(9) "Throwable"
  ["ExceptionInterface"]=>
  string(18) "ExceptionInterface"
  ["Two"]=>
  string(3) "Two"
  ["ContainerExceptionInterface"]=>
  string(27) "ContainerExceptionInterface"
  ["Countable"]=>
  string(9) "Countable"
}
