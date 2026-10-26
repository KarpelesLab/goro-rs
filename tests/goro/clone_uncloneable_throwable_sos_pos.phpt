--TEST--
Exceptions and WeakReference are uncloneable; a cloned SplObjectStorage is rewound
--FILE--
<?php
class MyEx extends RuntimeException {}
foreach ([new Exception('x'), new TypeError('t'), new MyEx('m'), WeakReference::create(new stdClass)] as $o) {
    try { clone $o; echo "cloned\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
$s = new SplObjectStorage;
$s[new stdClass] = 1; $s[new stdClass] = 2;
$s->rewind(); $s->next();
$c = clone $s;
var_dump($c->key(), $c->getInfo(), $s->key(), $s->getInfo());
?>
--EXPECT--
Trying to clone an uncloneable object of class Exception
Trying to clone an uncloneable object of class TypeError
Trying to clone an uncloneable object of class MyEx
Trying to clone an uncloneable object of class WeakReference
int(0)
int(1)
int(1)
int(2)
