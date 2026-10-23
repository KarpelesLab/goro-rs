--TEST--
CachingIterator TOSTRING_USE_INNER over a Generator throws and keeps the fetched element
--FILE--
<?php
function g() { yield 1; yield 2; }
$c = new CachingIterator(g(), CachingIterator::TOSTRING_USE_INNER);
try { $c->rewind(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($c->valid(), $c->current(), $c->key());
$c = new CachingIterator(g(), CachingIterator::TOSTRING_USE_INNER);
try { iterator_to_array($c); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($c->valid(), $c->current(), $c->key());
$c = new CachingIterator(new ArrayIterator([1, 2]), CachingIterator::TOSTRING_USE_INNER);
try { var_dump(iterator_to_array($c)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$c = new CachingIterator(g(), CachingIterator::TOSTRING_USE_INNER);
try { foreach ($c as $v) echo $v; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($c->valid(), $c->current(), $c->key());
$c = new CachingIterator(new ArrayIterator([[1], 2]), CachingIterator::CALL_TOSTRING);
try { foreach ($c as $v) echo "x"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($c->valid(), $c->current(), $c->key());
--EXPECTF--
Error: Object of class Generator could not be converted to string
bool(true)
int(1)
int(0)
Error: Object of class Generator could not be converted to string
bool(true)
int(1)
int(0)
Error: Object of class ArrayIterator could not be converted to string
Error: Object of class Generator could not be converted to string
bool(true)
int(1)
int(0)

Warning: Array to string conversion in %s on line 15
xxbool(false)
NULL
NULL
