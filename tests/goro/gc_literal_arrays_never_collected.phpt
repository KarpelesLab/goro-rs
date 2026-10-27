--TEST--
The cycle collector never frees nor counts literal arrays
--FILE--
<?php
// Literal arrays are held by the literal table: the cycle collector never
// frees (nor counts) them.
function f() {
  $o = new stdClass; $o->a = [1, 2]; $o->self = $o;
}
f();
var_dump(gc_collect_cycles());
function g() {
  $o = new stdClass; $o->a = [1, [3]]; $o->b = ['x' => [$o]]; $o->self = $o;
}
g();
var_dump(gc_collect_cycles());
$x = [1, 2];
$o = new stdClass; $o->a = $x; $o->self = $o; unset($o);
var_dump(gc_collect_cycles());
$o = new stdClass; $o->a = [1, 2]; $o->a[] = 3; $o->self = $o; unset($o);
var_dump(gc_collect_cycles());
const C = [1, 2];
$o = new stdClass; $o->a = C; $o->self = $o; unset($o);
var_dump(gc_collect_cycles());
$o = new stdClass; $o->a = range(1, 2); $o->self = $o; unset($o);
var_dump(gc_collect_cycles());
$o = new stdClass; $o->list = [[1], [2, [3]]]; $o->list[] = $o; unset($o);
var_dump(gc_collect_cycles());
for ($i = 0; $i < 3; $i++) { $o = new stdClass; $o->a = ['k' => [$i]]; $o->b = ['k' => [1]]; $o->s = $o; }
unset($o);
var_dump(gc_collect_cycles());
--EXPECT--
int(1)
int(3)
int(1)
int(2)
int(1)
int(2)
int(2)
int(9)
