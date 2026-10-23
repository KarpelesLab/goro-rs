--TEST--
Array copies share their storage until written (copy-on-write): both sides, references, arguments, by-ref builtins
--FILE--
<?php
// Copy-on-write semantics of array copies.
function show($label, ...$v) { echo $label, ': ', json_encode($v), "\n"; }

// 1. plain copies, both sides written
$a = [1, 2, 3];
$b = $a;
$b[] = 4;
show('1a', $a, $b);
$a[0] = 'x';
show('1b', $a, $b);
$c = $a; $d = $c; unset($c[1]); $d['k'] = 'v';
show('1c', $a, $c, $d);

// 2. references inside arrays
$a = [1, 2, 3];
$r = &$a[1];
$b = $a;
$r = 'R';
show('2a', $a, $b);
$b[1] = 'B';
show('2b', $a, $b, $r);
unset($r);
$c = $a;
$c[1] = 'C';
show('2c', $a, $c);
$x = [1, 2];
$y = &$x[0];
unset($y);
$z = $x;
$z[0] = 9;
show('2d', $x, $z);
$p = [1, 2]; $q = $p; $s = &$p[0]; $s = 'S';
show('2e', $p, $q);

// 3. function arguments
function addone($arr) { $arr[] = 1; return $arr; }
function sortit(array &$arr) { sort($arr); }
$a = [3, 1, 2];
$b = addone($a);
show('3a', $a, $b);
$c = $a;
sortit($a);
show('3b', $a, $c);
$d = $a;
array_push($d, 9);
array_pop($a);
show('3c', $a, $d);
$e = [5, 4];
$f = $e;
$g = array_splice($e, 0, 1);
show('3d', $e, $f, $g);
$h = [1, 2, 3];
$i = $h;
next($h); next($h);
show('3e', current($h), current($i), key($h), key($i));
end($i);
show('3f', current($h), current($i));

// 4. nested arrays
$a = ['x' => [1, 2], 'y' => ['z' => [3]]];
$b = $a;
$b['x'][] = 3;
$b['y']['z'][0] = 'Z';
show('4a', $a, $b);
$c = $a['y'];
$c['z'][] = 4;
show('4b', $a, $c);
$a['y']['n'] = 1;
show('4c', $a, $b, $c);

// 5. foreach
$a = [1, 2, 3];
foreach ($a as $k => $v) {
    if ($k == 0) { $a[] = 4; $a[1] = 'two'; }
    echo $v, ' ';
}
echo "\n";
show('5a', $a);
$a = [1, 2, 3];
foreach ($a as $k => &$v) {
    if ($k == 0) { $a[] = 4; }
    $v = $v * 10;
}
unset($v);
show('5b', $a);
$a = [1, 2, 3];
$b = $a;
foreach ($b as &$v) { $v++; }
unset($v);
show('5c', $a, $b);
$a = [[1], [2]];
foreach ($a as $k => $sub) { $sub[] = 'x'; $a[$k][] = 'y'; }
show('5d', $a);

// 6. static and global
function st() { static $s = [1]; $t = $s; $s[] = count($s) + 1; return [$s, $t]; }
st();
show('6a', st());
$g = [1, 2];
function gl() { global $g; $h = $g; $g[] = 3; return $h; }
$hh = gl();
show('6b', $g, $hh);
$gg = $GLOBALS['g'];
$GLOBALS['g'][] = 4;
show('6c', $g, $gg);

// 7. objects holding arrays
class Box { public $items = [1]; public function get() { return $this->items; } public function add($x) { $this->items[] = $x; } }
$o = new Box;
$copy = $o->get();
$o->add(2);
$copy[] = 'c';
show('7a', $o->items, $copy);
$o2 = clone $o;
$o2->add(3);
show('7b', $o->items, $o2->items);
$arr = $o->items;
$o->items[] = 'p';
show('7c', $arr, $o->items);
$o->items = $arr;
$arr[] = 'q';
show('7d', $arr, $o->items);

// 8. closures
$a = [1];
$f = function () use ($a) { $a[] = 2; return $a; };
$a[] = 'outer';
show('8a', $f(), $a, $f());
$g = function () use (&$a) { $a[] = 'ref'; };
$b = $a;
$g();
show('8b', $a, $b);

// 9. list/destructuring and swap
$a = [[1, 2], [3, 4]];
[$x, $y] = $a;
$x[] = 9;
show('9a', $a, $x, $y);
$p = [1]; $q = [2];
[$p, $q] = [$q, $p];
$p[] = 'p';
show('9b', $p, $q);

// 10. array functions taking by reference
$a = ['b' => 2, 'a' => 1];
$b = $a;
ksort($a);
show('10a', $a, $b);
$c = $b;
array_walk($c, function (&$v) { $v *= 100; });
show('10b', $b, $c);
$d = [3, 1, 2];
$e = $d;
usort($d, fn($x, $y) => $x <=> $y);
shuffle($e); sort($e);
show('10c', $d, $e);
$f = [1, 2];
$g = $f;
array_unshift($f, 0);
array_shift($g);
show('10d', $f, $g);
$s = ['x' => 1];
$t = $s;
extract($t);
$u = compact('s', 't');
$u['s'][] = 1;
show('10e', $s, $t, $u);

// 11. self-assignment, appending an array to itself
$a = [1];
$a[] = $a;
$a[0] = 'z';
show('11a', $a);
$b = [1];
$b['self'] = &$b;
$c = $b;
$c[] = 2;
echo count($b), ' ', count($c), "\n";

// 12. copy in a loop, unset copy, write original
$a = range(1, 5);
for ($i = 0; $i < 3; $i++) { $t = $a; $a[] = $i; unset($t); }
show('12a', $a);
$keep = [];
for ($i = 0; $i < 3; $i++) { $keep[] = $a; $a[] = "k$i"; }
show('12b', $keep, $a);

// 13. internal pointer copied with the array
$a = [1, 2, 3];
next($a);
$b = $a;
show('13a', current($b));
next($b);
show('13b', current($a), current($b));
$c = $a;
$c[] = 4;
show('13c', current($c));
?>
--EXPECT--
1a: [[1,2,3],[1,2,3,4]]
1b: [["x",2,3],[1,2,3,4]]
1c: [["x",2,3],{"0":"x","2":3},{"0":"x","1":2,"2":3,"k":"v"}]
2a: [[1,"R",3],[1,"R",3]]
2b: [[1,"B",3],[1,"B",3],"B"]
2c: [[1,"C",3],[1,"C",3]]
2d: [[1,2],[9,2]]
2e: [["S",2],[1,2]]
3a: [[3,1,2],[3,1,2,1]]
3b: [[1,2,3],[3,1,2]]
3c: [[1,2],[1,2,3,9]]
3d: [[4],[5,4],[5]]
3e: [3,1,2,0]
3f: [3,3]
4a: [{"x":[1,2],"y":{"z":[3]}},{"x":[1,2,3],"y":{"z":["Z"]}}]
4b: [{"x":[1,2],"y":{"z":[3]}},{"z":[3,4]}]
4c: [{"x":[1,2],"y":{"z":[3],"n":1}},{"x":[1,2,3],"y":{"z":["Z"]}},{"z":[3,4]}]
1 2 3 
5a: [[1,"two",3,4]]
5b: [[10,20,30,40]]
5c: [[1,2,3],[2,3,4]]
5d: [[[1,"y"],[2,"y"]]]
6a: [[[1,2,3],[1,2]]]
6b: [[1,2,3],[1,2]]
6c: [[1,2,3,4],[1,2,3]]
7a: [[1,2],[1,"c"]]
7b: [[1,2],[1,2,3]]
7c: [[1,2],[1,2,"p"]]
7d: [[1,2,"q"],[1,2]]
8a: [[1,2],[1,"outer"],[1,2]]
8b: [[1,"outer","ref"],[1,"outer"]]
9a: [[[1,2],[3,4]],[1,2,9],[3,4]]
9b: [[2,"p"],[1]]
10a: [{"a":1,"b":2},{"b":2,"a":1}]
10b: [{"b":2,"a":1},{"b":200,"a":100}]
10c: [[1,2,3],[1,2,3]]
10d: [[0,1,2],[2]]
10e: [{"x":1},{"x":1},{"s":{"x":1,"0":1},"t":{"x":1}}]
11a: [["z",[1]]]
2 3
12a: [[1,2,3,4,5,0,1,2]]
12b: [[[1,2,3,4,5,0,1,2],[1,2,3,4,5,0,1,2,"k0"],[1,2,3,4,5,0,1,2,"k0","k1"]],[1,2,3,4,5,0,1,2,"k0","k1","k2"]]
13a: [2]
13b: [2,3]
13c: [2]
