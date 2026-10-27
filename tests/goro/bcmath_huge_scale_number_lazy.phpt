--TEST--
BcMath\Number with a huge scale: the value is only formatted when read (memory_limit hit there), multiplications and remainders do not allocate it
--EXTENSIONS--
bcmath
--INI--
memory_limit=128M
--FILE--
<?php
$n = new BcMath\Number('1');
$r = $n->mul('2', 2147483647);
var_dump($r->scale);
$r = $n->mod('3', 2147483647);
var_dump($r->scale);
[$q, $m] = $n->divmod('3', 2147483647);
var_dump($q->scale, $m->scale);
$r = $n->add('2', 1000000);
var_dump(strlen($r->value), strlen((string) $r));
$r = $n->add('2', 2147483647);
echo "created\n";
echo strlen($r->value), "\n";
?>
--EXPECTF--
int(2147483647)
int(2147483647)
int(0)
int(2147483647)
int(1000002)
int(1000002)
created

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483680 bytes) in %s on line %d
