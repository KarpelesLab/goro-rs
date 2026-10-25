--TEST--
foreach loops over arrays ending in fast runs: the iterator's release, empty arrays, destructors
--FILE--
<?php
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "~D{$this->n}\n"; } }
function mk() { return [new D(1), new D(2)]; }
function lists() { return [[1, 2], [3], []]; }
function run() {
    $t = 0;
    foreach (lists() as $l) {
        foreach ($l as $v) { $t += $v; }
        foreach ($l as $k => $v) { $t += $k; }
    }
    echo $t, "\n";
    $e = [];
    foreach ($e as $v) { echo "never\n"; }
    foreach ([] as $k => $v) { echo "never\n"; }
    $shared = ['a' => [1, 2], 'b' => [3, 4]];
    $copy = $shared;
    foreach ($shared as $k => $inner) {
        foreach ($inner as $x) { $copy[$k][] = $x * 10; }
    }
    echo json_encode($copy), ' ', json_encode($shared), "\n";
    foreach (mk() as $d) { echo "in {$d->n}\n"; }
    echo "after mk\n";
    unset($d);
    echo "unset\n";
    $big = range(1, 600);
    $sum = 0;
    foreach ($big as $v) { $sum += $v; }
    foreach (array_chunk($big, 7) as $chunk) { foreach ($chunk as $v) { $sum -= $v; } }
    echo $sum, "\n";
    $r = [1, 2, 3];
    foreach ($r as &$ref) { $ref *= 2; }
    unset($ref);
    foreach ($r as $v) { echo $v; }
    echo "\n";
}
run();
echo "done\n";
?>
--EXPECT--
7
{"a":[1,2,10,20],"b":[3,4,30,40]} {"a":[1,2],"b":[3,4]}
in 1
in 2
~D1
after mk
~D2
unset
0
246
done
