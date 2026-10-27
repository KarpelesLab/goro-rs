--TEST--
WeakReference after handle reuse; foreach does not keep WeakMap / Iterator keys alive
--FILE--
<?php
// A WeakReference is not reused for a new object that got a dead one's handle
$o = new stdClass;
$w = WeakReference::create($o);
unset($o);
$p = new stdClass;
var_dump(spl_object_id($p), WeakReference::create($p)->get() === $p, $w->get());
// ... and one nothing holds any more is freed
$q = new stdClass;
WeakReference::create($q);
$w2 = WeakReference::create($q);
var_dump($w2, WeakReference::create($q) === $w2);
unset($w2);
var_dump(WeakReference::create($q));

// Loops over a WeakMap do not keep its keys alive once left
foreach (['val', 'ref', 'valbreak', 'refbreak', 'refnokey'] as $mode) {
    $o = new stdClass;
    $w = WeakReference::create($o);
    $wm = new WeakMap;
    $wm[$o] = 1;
    switch ($mode) {
        case 'val': foreach ($wm as $k => $v) {} break;
        case 'ref': foreach ($wm as $k => &$v) {} break;
        case 'valbreak': foreach ($wm as $k => $v) { break; } break;
        case 'refbreak': foreach ($wm as $k => &$v) { break; } break;
        case 'refnokey': foreach ($wm as &$v) {} break;
    }
    unset($k, $v, $o);
    var_dump($mode, $w->get(), count($wm));
}

// nor does a loop over an Iterator keep its last key
class It implements Iterator {
    public $i = 0;
    function __construct(public $k) {}
    function current(): mixed { return 1; }
    function key(): mixed { return $this->k; }
    function next(): void { $this->i++; }
    function rewind(): void { $this->i = 0; }
    function valid(): bool { return $this->i < 1; }
}
$o = new stdClass;
$w = WeakReference::create($o);
$it = new It($o);
foreach ($it as $k => $v) {}
unset($k, $o);
$it->k = null;
var_dump($w->get());
--EXPECT--
int(1)
bool(true)
NULL
object(WeakReference)#4 (1) {
  ["object"]=>
  object(stdClass)#3 (0) {
  }
}
bool(true)
object(WeakReference)#4 (1) {
  ["object"]=>
  object(stdClass)#3 (0) {
  }
}
string(3) "val"
NULL
int(0)
string(3) "ref"
NULL
int(0)
string(8) "valbreak"
NULL
int(0)
string(8) "refbreak"
NULL
int(0)
string(8) "refnokey"
NULL
int(0)
NULL
