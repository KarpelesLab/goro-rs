--TEST--
Loops left by break or return release their iterators; stored temporaries
--FILE--
<?php
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "~D{$this->n}\n"; } }
class Node { public $children = []; function add($c) { $this->children[] = $c; } function merge($o) { $this->children = array_merge($this->children, $o->children); } }
function find($list, $x) {
    foreach ($list as $k => $v) {
        foreach ($v as $w) {
            if ($w === $x) { return [$k, $w]; }
        }
    }
    return null;
}
function firstBig($list) {
    foreach ($list as $v) {
        try {
            if ($v > 10) { return $v; }
        } finally {
            echo "f$v ";
        }
    }
    return -1;
}
function breaks($list) {
    $n = 0;
    foreach ($list as $a) {
        foreach ($a as $b) {
            if ($b > 2) { break 2; }
            $n += $b;
        }
    }
    return $n;
}
function objs() {
    foreach ([new D(1), new D(2), new D(3)] as $d) {
        if ($d->n == 2) { return $d->n; }
    }
    return 0;
}
function tree() {
    $root = new Node;
    for ($i = 0; $i < 3; $i++) {
        $n = new Node;
        $n->add($i);
        $n->add([$i, $i * 2]);
        $root->merge($n);
        $root->add($n);
    }
    return count($root->children);
}
$l = [[1, 2], [3, 4], [5]];
for ($i = 0; $i < 3; $i++) {
    echo json_encode(find($l, 4)), ' ', json_encode(find($l, 9)), ' ', firstBig([1, 20, 30]), ' ', breaks($l), "\n";
}
echo objs(), "\n";
echo "after\n";
echo tree(), "\n";
?>
--EXPECT--
[1,4] null f1 f20 20 3
[1,4] null f1 f20 20 3
[1,4] null f1 f20 20 3
~D1
~D3
~D2
2
after
9
