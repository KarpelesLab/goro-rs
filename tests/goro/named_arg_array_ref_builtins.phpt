--TEST--
Array-modifying builtins (sort, array_shift, end, ...) given their array as a named argument modify it
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$a = [3, 1, 2];
t(function () use ($a) { sort(array: $a, flags: SORT_STRING); return $a; });
t(function () use ($a) { rsort(flags: 0, array: $a); return $a; });
t(function () use ($a) { usort($a, callback: fn($x, $y) => $x <=> $y); return $a; });
t(function () use ($a) { uasort(callback: fn($x, $y) => $x <=> $y, array: $a); return $a; });
t(function () use ($a) { array_push($a, ...[4, 5]); return $a; });
t(function () use ($a) { array_unshift($a, ...['x' => 4]); return $a; });
t(function () use ($a) { return array_splice(array: $a, offset: 1, replacement: ['q']); });
t(function () use ($a) { array_splice($a, 1, replacement: ['q']); return $a; });
t(function () use ($a) { return [array_pop(array: $a), $a]; });
t(function () use ($a) { return [array_shift(array: $a), $a]; });
t(function () use ($a) { return [end(array: $a), key(array: $a), reset(array: $a), next(array: $a), prev(array: $a), current(array: $a)]; });
t(function () use ($a) { return [shuffle(array: $a), count($a)]; });
t(function () use ($a) { array_walk(array: $a, callback: function (&$v, $k, $p) { $v .= $p; }, arg: '!'); return $a; });
t(function () use ($a) { return [settype(var: $a, type: 'string'), $a]; });
t(function () { $s = "12 apples"; $n = sscanf($s, "%d %s", $num, $what); return [$n, $num, $what]; });
t(function () { preg_match(pattern: '/(a)/', subject: 'xa', matches: $m); return $m; });
t(function () { preg_match('/(a)/', 'xa', flags: PREG_OFFSET_CAPTURE, matches: $m); return $m; });
 $a=[3,1,2]; sort(array: $a); echo implode($a),"\n"; $a=[3,1,2]; sort($a, flags: 0); echo implode($a),"\n"; $a=[3,1,2]; array_shift(array: $a); echo implode($a),"\n"; $a=[3,1,2]; echo end(array: $a), "\n"; function f() { $a=[3,1,2]; sort(array: $a); echo implode($a),"\n"; } f(); $g = function() { $a=[3,1,2]; array_pop(array: $a); echo implode($a),"\n"; }; $g();
--EXPECTF--
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(2)
  [2]=>
  int(1)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [1]=>
  int(1)
  [2]=>
  int(2)
  [0]=>
  int(3)
}
array(5) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  [3]=>
  int(4)
  [4]=>
  int(5)
}
ArgumentCountError: array_unshift() does not accept unknown named parameters
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  string(1) "q"
}
array(2) {
  [0]=>
  int(2)
  [1]=>
  array(2) {
    [0]=>
    int(3)
    [1]=>
    int(1)
  }
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
}
array(6) {
  [0]=>
  int(2)
  [1]=>
  int(2)
  [2]=>
  int(3)
  [3]=>
  int(1)
  [4]=>
  int(3)
  [5]=>
  int(3)
}
array(2) {
  [0]=>
  bool(true)
  [1]=>
  int(3)
}
array(3) {
  [0]=>
  string(2) "3!"
  [1]=>
  string(2) "1!"
  [2]=>
  string(2) "2!"
}

Warning: Array to string conversion in %s on line 17
array(2) {
  [0]=>
  bool(true)
  [1]=>
  string(5) "Array"
}
array(3) {
  [0]=>
  int(2)
  [1]=>
  int(12)
  [2]=>
  string(6) "apples"
}
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "a"
}
array(2) {
  [0]=>
  array(2) {
    [0]=>
    string(1) "a"
    [1]=>
    int(1)
  }
  [1]=>
  array(2) {
    [0]=>
    string(1) "a"
    [1]=>
    int(1)
  }
}
123
123
12
2
123
31
