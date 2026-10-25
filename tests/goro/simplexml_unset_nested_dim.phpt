--TEST--
unset() through a missing SimpleXML offset creates nothing and notices
--FILE--
<?php $s = simplexml_load_string("<r><i>1</i><j><k/></j></r>"); echo "a\n"; unset($s->i[1]->new[0]); echo "b\n"; unset($s->i[0]->new[0]); echo "c\n"; unset($s->zz[0]->q); echo "d\n"; unset($s->j[0]->k); echo $s->asXML(); echo "e\n"; $s->i[3]->x = 1; echo "f\n"; $s->nope[2]["a"] = 1; echo $s->asXML();
--EXPECTF--
a

Notice: Indirect modification of overloaded element of SimpleXMLElement has no effect in %s on line 1
b
c
d
<?xml version="1.0"?>
<r><i>1</i><j/><zz/></r>
e

Warning: main(): Cannot add element i number 3 when only 1 such elements exist in %s on line 1
f

Warning: main(): Cannot add element nope number 2 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><i>1</i><j/><zz/><i><x>1</x></i><nope a="1"/></r>
