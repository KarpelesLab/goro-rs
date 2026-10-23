--TEST--
empty() of an inaccessible property: __isset() decides, __get() alone is not called
--FILE--
<?php
class C { protected $z = 0; private $w = 1; public function __get($n) { echo "get $n\n"; return "x"; } }
$c = new C; var_dump(empty($c->z), empty($c->w), empty($c->nope), isset($c->z), $c->z ?? "dflt");
class D { protected $z = 0; }
$d = new D; var_dump(empty($d->z), isset($d->z));
--EXPECT--
get z
bool(true)
bool(true)
bool(true)
bool(false)
string(1) "x"
bool(true)
bool(false)
