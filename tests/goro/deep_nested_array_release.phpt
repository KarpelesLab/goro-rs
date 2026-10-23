--TEST--
Releasing a very deeply nested array
--FILE--
<?php
// Releasing a very deeply nested array does not overflow the native stack.
$a = [];
$p = &$a;
for ($i = 0; $i < 100000; $i++) { $p[0] = []; $p = &$p[0]; }
unset($p);
echo count($a), "\n";
$b = $a;
$b[0][0] = 1;
unset($b);
unset($a);
echo "freed\n";
function mk() { $r = []; $p = &$r; for ($i = 0; $i < 50000; $i++) { $p['k'] = [$i]; $p = &$p['k']; } return $r; }
$x = mk();
$x = null;
echo "done\n";
--EXPECT--
1
freed
done
