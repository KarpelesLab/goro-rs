--TEST--
iterator_to_array() of an ArrayIterator keeps the storage's references (get_current_data)
--FILE--
<?php
$x = 1; $a = [&$x, 2];
$fns = ['array_slice' => fn($a) => array_slice($a, 0), 'array_merge' => fn($a) => array_merge($a, []), 'array_values' => fn($a) => array_values($a),
  'array_reverse' => fn($a) => array_reverse($a), 'array_filter' => fn($a) => array_filter($a), 'array_map' => fn($a) => array_map(null, $a),
  'array_pad' => fn($a) => array_pad($a, 3, 0), 'array_replace' => fn($a) => array_replace($a, []), 'array_unique' => fn($a) => array_unique($a),
  'array_combine' => fn($a) => array_combine([0, 1], $a), 'array_chunk' => fn($a) => array_chunk($a, 1)[0], 'iterator_to_array' => fn($a) => iterator_to_array(new ArrayIterator($a)),
  'spread' => fn($a) => [...$a], 'plus' => fn($a) => $a + [], 'copy' => fn($a) => $a, 'array_splice' => function ($a) { array_splice($a, 1, 0, [9]); return $a; },
  'sort' => function ($a) { sort($a); return $a; }, 'compact' => function ($a) { $q = $a; return compact('q')['q']; }, 'array_column' => fn($a) => array_column([$a], 0),
  'array_diff' => fn($a) => array_diff($a, []), 'array_intersect' => fn($a) => array_intersect($a, $a), 'array_fill_keys' => fn($a) => array_fill_keys([0], $a[0]),
  'unserialize' => fn($a) => unserialize(serialize($a)), 'var_export' => fn($a) => eval('return ' . var_export($a, true) . ';'), 'json' => fn($a) => json_decode(json_encode($a), true),
  'array_walk' => function ($a) { array_walk($a, function (&$v) { $v = $v; }); return $a; }, 'foreach_ref' => function ($a) { foreach ($a as &$v) {} unset($v); return $a; },
  'array_shift' => function ($a) { array_shift($a); return $a; }, 'array_unshift' => function ($a) { array_unshift($a, 0); return $a; }, 'array_push' => function ($a) { array_push($a, 0); return $a; },
  'array_pop' => function ($a) { array_pop($a); return $a; }, 'end' => function ($a) { end($a); return $a; },
];
foreach ($fns as $n => $f) {
  $r = $f($a);
  echo $n, ": ";
  ob_start(); var_dump($r); echo str_replace("\n", " ", ob_get_clean());
  $r[0] = 100; echo " x=", $x, "\n"; $x = 1;
}
$x = 1; $a = [&$x, 2];
$r = iterator_to_array(new ArrayObject($a)); $r[0] = 5; var_dump($x); $x = 1;
$r = iterator_to_array(new ArrayIterator($a), false); $r[0] = 6; var_dump($x); $x = 1;
$r = iterator_to_array(new RecursiveArrayIterator($a)); $r[0] = 7; var_dump($x); $x = 1;
class AI extends ArrayIterator { function current(): mixed { return parent::current(); } }
$r = iterator_to_array(new AI($a)); $r[0] = 8; var_dump($x); $x = 1;
$r = iterator_to_array(new IteratorIterator(new ArrayIterator($a))); $r[0] = 9; var_dump($x); $x = 1;
$r = iterator_to_array(new LimitIterator(new ArrayIterator($a), 0, 2)); $r[0] = 10; var_dump($x); $x = 1;
function g() { $y = 1; $a = [&$y]; yield from new ArrayIterator($a); }
var_dump(iterator_to_array(g()));
$r = [...new ArrayIterator($a)]; $r[0] = 11; var_dump($x); $x = 1;
foreach (new ArrayIterator($a) as $k => $v) { $v = 12; } var_dump($x);
$r = iterator_to_array((function () use ($a) { yield from $a; })()); $r[0] = 13; var_dump($x); $x = 1;
var_dump(new ArrayIterator($a));
--EXPECT--
array_slice: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_merge: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_values: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_reverse: array(2) {   [0]=>   int(2)   [1]=>   &int(1) }  x=1
array_filter: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_map: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_pad: array(3) {   [0]=>   &int(1)   [1]=>   int(2)   [2]=>   int(0) }  x=100
array_replace: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_unique: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_combine: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_chunk: array(1) {   [0]=>   &int(1) }  x=100
iterator_to_array: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
spread: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
plus: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
copy: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_splice: array(3) {   [0]=>   &int(1)   [1]=>   int(9)   [2]=>   int(2) }  x=100
sort: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
compact: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_column: array(1) {   [0]=>   int(1) }  x=1
array_diff: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_intersect: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_fill_keys: array(1) {   [0]=>   int(1) }  x=1
unserialize: array(2) {   [0]=>   int(1)   [1]=>   int(2) }  x=1
var_export: array(2) {   [0]=>   int(1)   [1]=>   int(2) }  x=1
json: array(2) {   [0]=>   int(1)   [1]=>   int(2) }  x=1
array_walk: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
foreach_ref: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
array_shift: array(1) {   [0]=>   int(2) }  x=1
array_unshift: array(3) {   [0]=>   int(0)   [1]=>   &int(1)   [2]=>   int(2) }  x=1
array_push: array(3) {   [0]=>   &int(1)   [1]=>   int(2)   [2]=>   int(0) }  x=100
array_pop: array(1) {   [0]=>   &int(1) }  x=100
end: array(2) {   [0]=>   &int(1)   [1]=>   int(2) }  x=100
int(5)
int(6)
int(7)
int(1)
int(1)
int(1)
array(1) {
  [0]=>
  int(1)
}
int(1)
int(1)
int(13)
object(ArrayIterator)#33 (1) {
  ["storage":"ArrayIterator":private]=>
  array(2) {
    [0]=>
    &int(1)
    [1]=>
    int(2)
  }
}
