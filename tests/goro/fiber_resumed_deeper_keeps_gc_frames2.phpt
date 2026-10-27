--TEST--
Fibers and generators resumed from another depth: frames intact
--FILE--
<?php
// A fiber started at one depth and resumed (to its end) from a deeper one:
// the frames of its resumer stay intact (return values, GC).
$f = new Fiber(function () { Fiber::suspend(); return 'fiber-done'; });
$f->start();
class Holder { public $o; }
function deep(int $n, Fiber $f): ?object {
    if ($n > 0) {
        $r = deep($n - 1, $f);
        return $r;
    }
    $f->resume();
    $h = new Holder;
    $h->o = new stdClass;
    return $h;
}
$run = function () use ($f) { return deep(4, $f); };
$x = $run();
var_dump(get_class($x), $f->getReturn());
gc_collect_cycles();
// a generator driven inside a fiber resumed from elsewhere
function gen() { $v = yield 1; Fiber::suspend('in-gen'); yield $v; }
$g = gen();
$f2 = new Fiber(function () use ($g) { $g->current(); $g->send('sent'); return $g->current(); });
var_dump($f2->start());
function deep2(int $n, Fiber $f) { if ($n > 0) { $r = deep2($n - 1, $f); return [$r]; } $f->resume(); return 'leaf'; }
var_dump(deep2(3, $f2), $f2->getReturn());
echo "done\n";

?>
--EXPECTF--
string(6) "Holder"
string(10) "fiber-done"
string(6) "in-gen"
array(1) {
  [0]=>
  array(1) {
    [0]=>
    array(1) {
      [0]=>
      string(4) "leaf"
    }
  }
}
string(4) "sent"
done
