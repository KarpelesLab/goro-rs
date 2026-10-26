--TEST--
An exception leaving a frame does not close a generator that is referenced elsewhere
--FILE--
<?php
function gen() { try { yield 1; yield 2; } finally { echo "gen finally\n"; } }
function f($g) { throw new Exception("x"); }
$g = gen(); $g->current();
try { f($g); } catch (Exception $e) { echo "caught\n"; }
var_dump($g->valid());
$g->next();
var_dump($g->current());
echo "end\n";
--EXPECT--
caught
bool(true)
int(2)
end
gen finally
