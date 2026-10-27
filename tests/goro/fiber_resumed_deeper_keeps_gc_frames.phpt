--TEST--
A fiber started at one depth and resumed to its end from a deeper one leaves its resumer's frames (returned values)
--FILE--
<?php
// A fiber started at one depth and resumed (to its end) from a deeper one:
// the frames of its resumer stay intact (an object a call returns there).
$f = new Fiber(function () { Fiber::suspend(); return 'fiber-done'; });
function starter(Fiber $f) { $f->start(); }
starter($f);
function y(Fiber $f): object { $f->resume(); $o = new stdClass; $o->v = 'kept'; return $o; }
function x(Fiber $f) { $r = y($f); var_dump($r); }
x($f);
var_dump($f->getReturn());
$g = new Fiber(function () { Fiber::suspend(); });
function starter2(Fiber $g) { $g->start(); }
starter2($g);
function y2(Fiber $g): array { $g->resume(); return ['a' => 1]; }
function x2(Fiber $g) { $r = y2($g); var_dump($r); }
x2($g);
echo "done\n";

?>
--EXPECTF--
object(stdClass)#2 (1) {
  ["v"]=>
  string(4) "kept"
}
string(10) "fiber-done"
array(1) {
  ["a"]=>
  int(1)
}
done
