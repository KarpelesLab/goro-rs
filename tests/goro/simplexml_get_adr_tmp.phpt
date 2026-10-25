--TEST--
Elements created by a SimpleXML write fetch live in sxe->tmp
--FILE--
<?php $s = simplexml_load_string("<r><i>d1</i></r>"); $s->nope->i[0] = 1.5; echo spl_object_id(new stdClass), "\n"; $s->a = 1; echo spl_object_id(new stdClass), "\n"; $s->b->c = 1; echo spl_object_id(new stdClass), "\n"; $s->i[0] = 2; echo spl_object_id(new stdClass), "\n";  $s->d->e->f = 1; echo spl_object_id(new stdClass), "\n";
--EXPECT--
4
4
2
2
3
