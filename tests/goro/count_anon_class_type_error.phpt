--TEST--
count()/sizeof() of a non-countable anonymous class object names it class@anonymous
--FILE--
<?php
namespace Foo;
$o = new class {};
try { var_dump(sizeof($o)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(count($o)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = 'count';
try { var_dump($f($o)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\count($o)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(sizeof(new \stdClass)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$o2 = new class extends \ArrayIterator {};
var_dump(sizeof($o2));
$o3 = new class implements \IteratorAggregate { function getIterator(): \Iterator { return new \ArrayIterator([]); } };
try { var_dump(sizeof($o3)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$o4 = new class extends \Exception {};
try { var_dump(count($o4)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
TypeError: sizeof(): Argument #1 ($value) must be of type Countable|array, class@anonymous given
TypeError: count(): Argument #1 ($value) must be of type Countable|array, class@anonymous given
TypeError: count(): Argument #1 ($value) must be of type Countable|array, class@anonymous given
TypeError: count(): Argument #1 ($value) must be of type Countable|array, class@anonymous given
TypeError: sizeof(): Argument #1 ($value) must be of type Countable|array, stdClass given
int(0)
TypeError: sizeof(): Argument #1 ($value) must be of type Countable|array, IteratorAggregate@anonymous given
TypeError: count(): Argument #1 ($value) must be of type Countable|array, Exception@anonymous given
