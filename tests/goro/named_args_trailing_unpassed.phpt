--TEST--
Named arguments: parameters after the last passed one are not passed (func_get_args())
--FILE--
<?php
// Named arguments: parameters after the last one passed are not passed
function defaults($a, $b = 'B', $c = 'C', ...$rest) { return [func_num_args(), func_get_args()]; }
echo json_encode(defaults(...['a' => 1])), "\n";
echo json_encode(defaults(a: 1)), "\n";
echo json_encode(defaults(a: 1, c: 3)), "\n";
echo json_encode(defaults(c: 3, a: 1)), "\n";
echo json_encode(defaults(1, c: 3)), "\n";
echo json_encode(defaults(1, ...['b' => 2])), "\n";
echo json_encode(defaults(1, 2, 3, 4, ...['x' => 5])), "\n";
function req($a, $b) { return func_get_args(); }
try { req(a: 1); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
try { req(b: 1); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
class M { function m($x, $y = 'y') { return func_get_args(); } static function s($x = 1, $y = 2) { return func_num_args(); } }
echo json_encode((new M)->m(x: 'X')), M::s(x: 5), M::s(y: 5), "\n";
$f = fn($p, $q = 'q') => func_get_args();
echo json_encode($f(p: 1)), "\n";
--EXPECTF--
[1,[1]]
[1,[1]]
[3,[1,"B",3]]
[3,[1,"B",3]]
[3,[1,"B",3]]
[2,[1,2]]
[4,[1,2,3,4]]
Too few arguments to function req(), 1 passed in %s on line 12 and exactly 2 expected
req(): Argument #1 ($a) not passed
["X"]12
[1]
