--TEST--
Arrays: nested arrays are shared by copies until written
--FILE--
<?php
// Copies of arrays share their nested arrays until either is written.
$b = ['x' => ['y' => 1, 'z' => [1]]];
$a = $b; $a['x']['y'] = 2; $a['x']['z'][] = 2;
echo json_encode($b), json_encode($a), "\n";
$a = $b; foreach ($a['x'] as &$v) { $v = 9; } unset($v);
echo json_encode($b), json_encode($a), "\n";
$a = $b; foreach ($a as &$v) { $v['y'] = 7; } unset($v);
echo json_encode($b), json_encode($a), "\n";
$a = $b; $r = &$a['x']['z']; $r[] = 5; unset($r);
echo json_encode($b), json_encode($a), "\n";
$a = $b; array_walk_recursive($a, function (&$v) { $v = 0; });
echo json_encode($b), json_encode($a), "\n";
$a = $b; sort($a['x']['z']); array_push($a['x']['z'], 3);
echo json_encode($b), json_encode($a), "\n";
function g(array $p) { $p['x']['y'] = 'G'; return $p; }
echo json_encode($b), json_encode(g($b)), "\n";
// properties
class H { public $a = []; public $b = [30, 10, 20]; }
$h = new H; $h->a = $h->b; sort($h->a); echo implode(',', $h->b), ' ', implode(',', $h->a), "\n";
$h = new H; $h->a = $h->b; end($h->a); var_dump(current($h->b));
$h = new H; $h->a = $h->b; foreach ($h->a as &$v) { $v = 0; } unset($v); echo implode(',', $h->b), "\n";
$h = new H; $h->a[0] = $h->b; next($h->a[0]); var_dump(current($h->b));
// storing a copy into an element of itself
$c = [[]];
$c[0][0] = $c;
var_dump($c);
// many copies of a big nested array stay cheap
$big = [];
for ($i = 0; $i < 200; $i++) { $big["k$i"] = ['n' => [$i, [$i]]]; }
$m = memory_get_usage();
$copies = [];
for ($i = 0; $i < 100; $i++) { $copies[] = ['big' => $big]; }
var_dump(memory_get_usage() - $m < 4000000);
?>
--EXPECT--
{"x":{"y":1,"z":[1]}}{"x":{"y":2,"z":[1,2]}}
{"x":{"y":1,"z":[1]}}{"x":{"y":9,"z":9}}
{"x":{"y":1,"z":[1]}}{"x":{"y":7,"z":[1]}}
{"x":{"y":1,"z":[1]}}{"x":{"y":1,"z":[1,5]}}
{"x":{"y":1,"z":[1]}}{"x":{"y":0,"z":[0]}}
{"x":{"y":1,"z":[1]}}{"x":{"y":1,"z":[1,3]}}
{"x":{"y":1,"z":[1]}}{"x":{"y":"G","z":[1]}}
30,10,20 10,20,30
int(30)
30,10,20
int(30)
array(1) {
  [0]=>
  array(1) {
    [0]=>
    array(1) {
      [0]=>
      array(0) {
      }
    }
  }
}
bool(true)
