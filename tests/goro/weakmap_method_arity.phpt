--TEST--
WeakMap methods check their argument count
--FILE--
<?php
$w = new WeakMap; $k = new stdClass; $w[$k] = 1;
foreach ($w as $kk => $v) var_dump($v);
var_dump(count($w), $w->count());
try { $w->count(1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $w->getIterator(1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $w->offsetGet(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$it = $w->getIterator(); var_dump(get_class($it), $it->current());
--EXPECT--
int(1)
int(1)
int(1)
WeakMap::count() expects exactly 0 arguments, 1 given
WeakMap::getIterator() expects exactly 0 arguments, 1 given
WeakMap::offsetGet() expects exactly 1 argument, 0 given
string(16) "InternalIterator"
int(1)
