--TEST--
Releasing a long chain of objects at once does not overflow the native stack
--INI--
memory_limit=1G
--FILE--
<?php
class N { public $next; public $v; }
$head = null;
for ($i = 0; $i < 300000; $i++) { $o = new N; $o->v = $i; $o->next = $head; $head = $o; }
$o = null;
$head = null;
echo "released\n";
// a short chain still frees its handles innermost first
$a = new N; $a->next = new N; $a->next->next = new N;
$ids = [spl_object_id($a), spl_object_id($a->next), spl_object_id($a->next->next)];
$a = null;
$x = new N; $y = new N; $z = new N;
var_dump([spl_object_id($x), spl_object_id($y), spl_object_id($z)] === [$ids[0], $ids[1], $ids[2]]);
?>
--EXPECT--
released
bool(true)
