--TEST--
SPL / DatePeriod iterators release their handles in php-src's order
--FILE--
<?php
// Handles freed by SPL iterators come back in php-src's release order
// (spl_dual_it_free_storage(): current element, inner iterator handle,
// inner object, cache, callback; date_period_it_dtor(); the fixed array
// iterator's array before its handle).
function probe($l) { $p = []; for ($i = 0; $i < 10; $i++) { $o = new stdClass; $p[] = $o; } echo $l, ": ", implode(",", array_map('spl_object_id', $p)), "\n"; }
$f = new CallbackFilterIterator(new ArrayIterator([1, 2]), fn($x) => true); foreach ($f as $v) {} unset($f, $v); probe("cbf");
$f = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([[1], 2]), fn($x) => true); foreach (new RecursiveIteratorIterator($f) as $v) {} unset($f); probe("rcbf");
$f = new CachingIterator(new ArrayIterator([new stdClass, new stdClass]), CachingIterator::FULL_CACHE); foreach ($f as $v); unset($f, $v); probe("cache");
$f = new AppendIterator; $f->append(new ArrayIterator([new stdClass])); $f->append(new ArrayIterator([new stdClass])); foreach ($f as $v) break; unset($f, $v); probe("append");
$f = new LimitIterator(new ArrayIterator([1, 2, 3]), 1, 1); foreach ($f as $v); unset($f); probe("limit");
$x = new SplFixedArray(1); $x[0] = new stdClass; $i = $x->getIterator(); unset($x); probe("fixed"); unset($i); probe("fixed it");
$p = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2); $i = $p->getIterator(); foreach ($i as $v) {} unset($p, $v); probe("period"); unset($i); probe("period it");
$x = new RecursiveIteratorIterator(new RecursiveArrayIterator([[1, [2]], 3])); $x->rewind(); while ($x->valid() && $x->getDepth() < 2) $x->next(); unset($x); probe("rii levels");
$x = new RecursiveTreeIterator(new RecursiveArrayIterator([[1, [2]], 3])); foreach ($x as $v) { if ($x->getDepth() == 2) break; } unset($x, $v); probe("tree levels");
?>
--EXPECT--
cbf: 1,3,2,4,5,6,7,8,9,10
rcbf: 10,8,9,7,4,6,5,11,3,2
cache: 5,2,11,3,6,4,7,9,8,10
append: 4,10,8,3,11,7,9,6,2,5
limit: 5,2,6,9,7,11,3,8,10,4
fixed: 11,7,9,6,2,5,1,12,13,14
fixed it: 3,8,4,10,14,13,12,1,5,2
period: 13,12,14,10,4,8,3,6,9,7
period it: 5,1,2,7,9,6,3,8,4,10
rii levels: 10,4,8,3,6,9,7,2,1,5
tree levels: 5,2,1,7,8,3,4,12,14,13
