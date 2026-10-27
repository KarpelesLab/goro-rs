--TEST--
Releasing a long chain of objects (or of objects and arrays) at once does not overflow the native stack
--INI--
memory_limit=1G
--FILE--
<?php
// (php itself overflows its stack on chains this long: the output was
// checked against it with shorter ones)
class N { public $next; public $v; }
$head = null;
for ($i = 0; $i < 300000; $i++) { $o = new N; $o->v = $i; $o->next = $head; $head = $o; }
$o = null;
$head = null;
echo "objects released\n";
// object -> array -> object -> ...
$head = null;
for ($i = 0; $i < 100000; $i++) { $o = new N; $o->next = [$head]; $head = $o; }
$o = null;
$head = null;
echo "objects and arrays released\n";
// object -> reference -> object -> ...
$head = null;
for ($i = 0; $i < 100000; $i++) { $o = new N; $r = $head; $o->next = &$r; unset($r); $head = $o; }
$o = null;
$head = null;
echo "objects and references released\n";
// in a fiber (its own, smaller stack)
$f = new Fiber(function () {
    $head = null;
    for ($i = 0; $i < 100000; $i++) { $o = new N; $o->next = $head; $head = $o; }
    $o = null;
    $head = null;
    echo "released in a fiber\n";
});
$f->start();
// a short chain still frees its handles innermost first
$a = new N; $a->next = new N; $a->next->next = new N;
$ids = [spl_object_id($a), spl_object_id($a->next), spl_object_id($a->next->next)];
$a = null;
$x = new N; $y = new N; $z = new N;
var_dump([spl_object_id($x), spl_object_id($y), spl_object_id($z)] === [$ids[0], $ids[1], $ids[2]]);
?>
--EXPECT--
objects released
objects and arrays released
objects and references released
released in a fiber
bool(true)
