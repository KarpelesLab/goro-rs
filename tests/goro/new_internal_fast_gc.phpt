--TEST--
objects of internal classes made from a NEW site template are collected in cycles
--FILE--
<?php
function cyc() {
    $a = new ArrayObject();
    $a[0] = $a;
    $f = new SplFixedArray(1);
    $f[0] = $f;
    $e = new Exception("x");
    $o = new stdClass;
    $o->e = $e;
    $e->o = $o;
}
for ($i = 0; $i < 3; $i++) {
    cyc();
    var_dump(gc_collect_cycles());
}
echo "ok\n";
?>
--EXPECTF--

Deprecated: Creation of dynamic property Exception::$o is deprecated in %s on line 10
int(7)

Deprecated: Creation of dynamic property Exception::$o is deprecated in %s on line 10
int(7)

Deprecated: Creation of dynamic property Exception::$o is deprecated in %s on line 10
int(7)
ok
