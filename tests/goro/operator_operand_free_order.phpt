--TEST--
Operands of binary operators are released in Zend's order ($a > $b is $b < $a)
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "d{$this->n} "; } }
function mk($n) { return [new D($n)]; }
function o($n) { return new D($n); }
echo "add: "; $x = mk(1) + mk(2); echo "| "; unset($x); echo "\n";
echo "eq: "; $x = mk(1) == mk(2); echo "\n";
echo "ident: "; $x = mk(1) === mk(2); echo "\n";
echo "neq: "; $x = mk(1) != mk(2); echo "\n";
echo "lt: "; $x = mk(1) < mk(2); echo "\n";
echo "spaceship: "; $x = mk(1) <=> mk(2); echo "\n";
echo "objeq: "; $x = o(1) == o(2); echo "\n";
echo "objid: "; $x = o(1) === o(2); echo "\n";
echo "concat: "; $x = [o(1)][0]->n . [o(2)][0]->n; echo "\n";
echo "and: "; $x = o(1) && o(2); echo "\n";
echo "or: "; $x = o(1) xor o(2); echo "\n";
echo "inst: "; $x = o(1) instanceof D; echo "\n";
echo "mul: "; $x = count(mk(1)) * count(mk(2)); echo "\n";
echo "bitor: "; $x = mk(1) == [] | mk(2) == []; echo "\n";
echo "coalesce: "; $x = mk(1) ?? mk(2); echo "| "; unset($x); echo "\n";
echo "ternary: "; $x = mk(1) ?: mk(2); echo "| "; unset($x); echo "\n";
echo "gt: "; $x = mk(1) > mk(2); echo "\n";
echo "ge: "; $x = mk(1) >= mk(2); echo "\n";
echo "le: "; $x = mk(1) <= mk(2); echo "\n";
echo "nident: "; $x = mk(1) !== mk(2); echo "\n";
echo "addobj: "; try { $x = o(1) + o(2); } catch (Throwable $e) { echo get_class($e), " "; } echo "\n";
--EXPECT--
add: d2 | d1 
eq: d1 d2 
ident: d1 d2 
neq: d1 d2 
lt: d1 d2 
spaceship: d1 d2 
objeq: d1 d2 
objid: d1 d2 
concat: d1 d2 
and: d1 d2 
or: d2 
inst: d1 d1 
mul: d1 d2 
bitor: d1 d2 
coalesce: | d1 
ternary: | d1 
gt: d2 d1 
ge: d2 d1 
le: d1 d2 
nident: d1 d2 
addobj: d1 d2 TypeError 
