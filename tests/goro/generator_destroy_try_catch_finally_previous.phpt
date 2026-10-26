--TEST--
Generator destruction runs only finally blocks (not a try's end); an exception from a finally keeps the pending one as previous
--FILE--
<?php
function g() { while (true) { try { yield; } catch (Exception $e) { echo "caught ", $e->getMessage(), "\n"; } } }
$gen = g(); $gen->current(); $gen->throw(new Exception("b")); unset($gen); echo "ok\n";
function g2() { try { yield 1; } catch (Exception $e) { echo "c\n"; } finally { echo "fin\n"; } }
$gen = g2(); $gen->current(); unset($gen); echo "ok2\n";
function g3() { try { try { yield 1; } finally { echo "inner fin\n"; } } catch (Exception $e) { echo "c\n"; } }
$gen = g3(); $gen->current(); $gen = null; echo "ok3\n";
function g11() { try { yield 1; } catch (Exception $e) { echo "no\n"; } echo "after (wrong)\n"; }
$gen = g11(); $gen->current(); unset($gen); echo "ok4\n";
function gb() { try { yield 1; } finally { throw new Exception("fin"); } }
$g = gb(); $g->current();
try { $g->throw(new Exception("orig")); } catch (Exception $e) { var_dump($e->getMessage(), $e->getPrevious()?->getMessage()); }
function g2b() { try { yield 1; } finally { echo "in finally\n"; } }
$g = g2b(); $g->current();
try { $g->throw(new Exception("orig2")); } catch (Exception $e) { var_dump($e->getMessage(), $e->getPrevious()?->getMessage()); }
--EXPECTF--
caught b
ok
fin
ok2
inner fin
ok3
ok4
string(3) "fin"
string(4) "orig"
in finally
string(5) "orig2"
NULL
