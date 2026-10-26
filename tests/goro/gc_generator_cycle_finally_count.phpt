--TEST--
gc_collect_cycles() counts a garbage generator its close freed
--FILE--
<?php
function cyc() { $self = yield; try { yield 1; } finally { echo "cyc finally\n"; } }
$c = cyc(); $c->send($c); unset($c);
var_dump(gc_collect_cycles());
function cyc2() { $self = yield; yield 1; }
$c = cyc2(); $c->send($c); unset($c);
var_dump(gc_collect_cycles());
class Holder { public $g; }
function cyc3($h) { try { yield 1; } finally { echo "cyc3 finally\n"; } }
$h = new Holder; $h->g = cyc3($h); $h->g->current(); unset($h);
var_dump(gc_collect_cycles());
--EXPECT--
cyc finally
int(1)
int(1)
cyc3 finally
int(1)
