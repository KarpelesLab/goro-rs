--TEST--
Objects made by NEW and overwritten are released at once: weak references, WeakMap keys, buffered roots, destructors, handle reuse
--FILE--
<?php
class P { public $x = 1; public $y = 'a'; }
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }

// Objects made by NEW, overwritten in a loop: their handles are reused
$ids = [];
for ($i = 0; $i < 5; $i++) {
    $o = new P;
    $ids[] = spl_object_id($o);
}
echo implode(',', $ids), "\n";

// A WeakReference to an overwritten object sees it gone
$o = new P;
$w = WeakReference::create($o);
$o = new P;
var_dump($w->get());

// A WeakMap key overwritten: its entry goes
$m = new WeakMap;
$k = new P;
$m[$k] = 'v';
for ($i = 0; $i < 3; $i++) {
    $k = new P;
}
var_dump(count($m));

// An object that was a possible cycle root, then overwritten
$a = new P;
$b = $a;
unset($b);           // $a is buffered as a root
$a = new P;          // the buffered one is freed
var_dump(gc_collect_cycles());
var_dump(gc_status()['roots'] <= 1);

// Objects with a destructor in the mix (the general path)
$d = new D(1);
for ($i = 0; $i < 3; $i++) {
    $o = new P;
    if ($i == 1) { $d = new D(2); }
}
echo "after loop\n";

// foreach over an object, overwritten inside the loop
$o = new P;
foreach ($o as $k2 => $v2) {
    echo "$k2=$v2\n";
    $o = new P;
}

// memory is not retained by overwritten objects
$before = memory_get_usage();
for ($i = 0; $i < 10000; $i++) {
    $o = new P;
}
var_dump(memory_get_usage() - $before < 4096);
echo "done\n";
?>
--EXPECT--
1,2,1,2,1
NULL
int(0)
int(0)
bool(true)
dtor 1
after loop
x=1
y=a
bool(true)
done
dtor 2
