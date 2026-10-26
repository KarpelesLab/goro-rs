--TEST--
A generator released by a throwing frame: an exception from its finally has the one in flight as previous
--FILE--
<?php
function gen() { try { yield 1; } finally { throw new Exception("gen finally throws"); } }
function f() { $g = gen(); $g->current(); throw new Exception("orig"); }
try { f(); } catch (Exception $e) { echo $e->getMessage(), " / prev: ", $e->getPrevious()?->getMessage(), "\n"; }
function f2() { $g = gen(); $g->current(); return 1; }
try { f2(); } catch (Exception $e) { echo "f2: ", $e->getMessage(), "\n"; }
function f3() { $g = gen(); $g->current(); unset($g); echo "after unset\n"; }
try { f3(); } catch (Exception $e) { echo "f3: ", $e->getMessage(), "\n"; }
class D { function __destruct() { throw new Exception("D dtor"); } }
function f4() { $a = new D; $b = new D; throw new Exception("f4"); }
try { f4(); } catch (Exception $e) { $c = $e; $n = 0; while ($c) { echo "chain $n: ", $c->getMessage(), "\n"; $c = $c->getPrevious(); $n++; } }
function f5() { try { $a = new D; } finally { echo "f5 finally\n"; } }
try { f5(); } catch (Exception $e) { echo "f5: ", $e->getMessage(), "\n"; }
function f6() { $x = new D; $x = null; echo "unreached?\n"; }
try { f6(); } catch (Exception $e) { echo "f6: ", $e->getMessage(), "\n"; }
--EXPECT--
gen finally throws / prev: orig
f2: gen finally throws
f3: gen finally throws
chain 0: D dtor
chain 1: D dtor
chain 2: f4
f5 finally
f5: D dtor
f6: D dtor
