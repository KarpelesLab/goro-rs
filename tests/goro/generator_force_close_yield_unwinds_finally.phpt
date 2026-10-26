--TEST--
A yield in finally of a force-closed generator throws through the enclosing try blocks
--FILE--
<?php
function gen2() { try { yield 1; } finally { echo "f1\n"; try { yield 2; } catch (Error $e) { echo "caught: ", $e->getMessage(), "\n"; } finally { echo "f2\n"; } echo "after\n"; } }
$g = gen2(); $g->current(); unset($g); echo "done\n";
function gen3() { try { try { yield 1; } finally { echo "a\n"; yield 3; } } finally { echo "b\n"; } }
$g = gen3(); $g->current();
try { unset($g); } catch (Error $e) { echo "outer caught: ", $e->getMessage(), "\n"; }
echo "end\n";
--EXPECT--
f1
caught: Cannot yield from finally in a force-closed generator
f2
after
done
a
b
outer caught: Cannot yield from finally in a force-closed generator
end
