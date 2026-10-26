--TEST--
Trait and class properties with identical array defaults are compatible
--FILE--
<?php
trait U { public $p = [1, 'a' => [2]]; public static $s = ['x']; } class B { use U; public $p = [1, 'a' => [2]]; public static $s = ['x']; } echo "ok\n";
trait V { const C = [1]; } class D { use V; const C = [1]; } echo "const ok\n";
trait W { public $p = [1]; } class E { use W; public $p = [1.0]; }
?>
--EXPECTF--
ok
const ok

Fatal error: E and W define the same property ($p) in the composition of E. However, the definition differs and is considered incompatible. Class was composed in %s on line 4
