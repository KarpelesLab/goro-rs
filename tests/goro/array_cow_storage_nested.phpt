--TEST--
Copy-on-write array storage: nested writes, references into copies, destructors and cycle collection
--FILE--
<?php
function show($label, ...$v) { echo $label, ': ', json_encode($v), "\n"; }
class D { public $n; public $self; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }

// nested writes on either side of a copy
$a = [[1, 2], ['k' => [3]]];
$b = $a;
$a[0][] = 'a0';
$b[1]['k'][] = 'b1';
show('n1', $a, $b);
$c = $b;
sort($c[0]);
rsort($b[0]);
show('n2', $b, $c);
$d = $c;
foreach ($d[1]['k'] as &$v) { $v = "d$v"; }
unset($v);
show('n3', $c, $d);
$e = $d;
array_push($e[0], 'pushed');
show('n4', $d, $e);
$f = $e;
array_walk_recursive($f, function (&$x) { if (is_int($x)) $x *= 10; });
show('n5', $e, $f);
$g = $f;
[$p, [$q]] = [$g[0], $g[1]['k']];
$p[] = 'p';
show('n6', $g, $p, $q);
$h = $g;
[, &$ref] = $h[0];
$ref = 'REF';
show('n7', $g[0], $h[0]);
unset($ref);

// references into copies
$a = [1, 2, 3];
$b = $a;
$r = &$b[0];
$r = 'r';
show('r1', $a, $b);
$c = $b;
$r = 'r2';
show('r2', $b, $c);
unset($r);
$x = ['n' => [1]];
$y = $x;
$rr = &$y['n'];
$rr[] = 2;
show('r3', $x, $y);
unset($rr);

// self insertion
$a = ['x' => [1]];
$a['x'][] = $a;
show('s1', $a);
$c = [1];
$c[] = $c;
$c[] = $c;
show('s3', $c);

// extract with references on a copy
$src = ['ea' => 1, 'eb' => [2]];
$cp = $src;
extract($cp, EXTR_REFS);
$ea = 'changed';
$eb[] = 3;
show('e1', $src, $cp);

// static properties
class S { public static $arr = [1, [2]]; }
$x = S::$arr;
S::$arr[] = 'static';
S::$arr[1][] = 'nested';
$x[] = 'local';
show('sp', S::$arr, $x);

// destructor timing with objects held by shared storage
$a = [new D(1), new D(2)];
$b = $a;
echo "unset a\n";
unset($a);
echo "unset b\n";
unset($b);
echo "after\n";
$c = [new D(3)];
$d = $c;
$c[] = 'x';
echo "replace d\n";
$d = null;
echo "unset c\n";
unset($c);
echo "done\n";

// cycles through shared storage
gc_collect_cycles();
$o = new D(4);
$o->self = [$o];
$copy = $o->self;
unset($o);
echo "collected: ", gc_collect_cycles(), "\n";
unset($copy);
echo "collected: ", gc_collect_cycles(), "\n";
$p = new D(5);
$arr = [$p, [$p]];
$p->self = $arr;
$q = $arr;
unset($p, $arr);
echo "collected: ", gc_collect_cycles(), "\n";
unset($q);
echo "collected: ", gc_collect_cycles(), "\n";
$z = ['a' => 1];
$z['me'] = &$z;
$w = $z;
unset($z);
echo "collected: ", gc_collect_cycles(), "\n";
unset($w);
echo "collected: ", gc_collect_cycles(), "\n";

// functions modifying their array parameter
function modify(array $arr) { $arr['added'] = true; $arr['n'][] = 1; return $arr; }
$base = ['n' => [0]];
$m1 = modify($base);
$m2 = modify($m1);
show('f1', $base, $m1, $m2);
function keep(array $arr) { static $kept = []; $kept[] = $arr; return $kept; }
$k = [1];
keep($k);
$k[] = 2;
show('f2', keep($k));

// var_export / serialize / json of copies
$a = ['x' => [1, 2], 'y' => 'z'];
$b = $a;
$b['x'][0] = 'B';
var_export($a); echo "\n";
echo serialize($b), "\n";
echo count($a, COUNT_RECURSIVE), count($b, COUNT_RECURSIVE), "\n";
// array functions returning modified copies
$a = [3, 1, 2];
$b = $a;
show('af', array_reverse($b), array_slice($b, 1), array_merge($a, $b), $a + [5, 6, 7, 8], array_map(fn($x) => $x * 2, $b), array_filter($b, fn($x) => $x > 1));
$st = ['k' => 'v'];
$cp = $st;
$cp2 = array_change_key_case($cp, CASE_UPPER);
unset($cp['k']);
show('af2', $st, $cp, $cp2);
// internal pointer functions on copies
$a = ['a' => 1, 'b' => 2, 'c' => 3];
$b = $a;
end($b);
show('ip', key($a), key($b), current($a), current($b));
$c = $b;
reset($c);
show('ip2', key($b), key($c));
// array as object property, copied out and in
$o = new stdClass;
$o->list = [1, 2];
$l = $o->list;
$o->list[] = 3;
$l[] = 'l';
$o2 = clone $o;
$o2->list[] = 'o2';
show('op', $o->list, $l, $o2->list);
?>
--EXPECT--
n1: [[[1,2,"a0"],{"k":[3]}],[[1,2],{"k":[3,"b1"]}]]
n2: [[[2,1],{"k":[3,"b1"]}],[[1,2],{"k":[3,"b1"]}]]
n3: [[[1,2],{"k":[3,"b1"]}],[[1,2],{"k":["d3","db1"]}]]
n4: [[[1,2],{"k":["d3","db1"]}],[[1,2,"pushed"],{"k":["d3","db1"]}]]
n5: [[[1,2,"pushed"],{"k":["d3","db1"]}],[[10,20,"pushed"],{"k":["d3","db1"]}]]
n6: [[[10,20,"pushed"],{"k":["d3","db1"]}],[10,20,"pushed","p"],"d3"]
n7: [[10,20,"pushed"],[10,"REF","pushed"]]
r1: [[1,2,3],["r",2,3]]
r2: [["r2",2,3],["r2",2,3]]
r3: [{"n":[1]},{"n":[1,2]}]
s1: [{"x":[1,{"x":[1]}]}]
s3: [[1,[1],[1,[1]]]]
e1: [{"ea":1,"eb":[2]},{"ea":"changed","eb":[2,3]}]
sp: [[1,[2,"nested"],"static"],[1,[2],"local"]]
unset a
unset b
dtor 1
dtor 2
after
replace d
unset c
dtor 3
done
collected: 0
collected: dtor 4
2
collected: 0
collected: dtor 5
3
collected: 0
collected: 1
f1: [{"n":[0]},{"n":[0,1],"added":true},{"n":[0,1,1],"added":true}]
f2: [[[1],[1,2]]]
array (
  'x' => 
  array (
    0 => 1,
    1 => 2,
  ),
  'y' => 'z',
)
a:2:{s:1:"x";a:2:{i:0;s:1:"B";i:1;i:2;}s:1:"y";s:1:"z";}
44
af: [[2,1,3],[1,2],[3,1,2,3,1,2],[3,1,2,8],[6,2,4],{"0":3,"2":2}]
af2: [{"k":"v"},[],{"K":"v"}]
ip: ["a","c",1,3]
ip2: ["c","a"]
op: [[1,2,3],[1,2,"l"],[1,2,3,"o2"]]
