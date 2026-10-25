--TEST--
empty() on a native object property asks has_property(NOT_EMPTY)
--FILE--
<?php $s = simplexml_load_string("<r><e/><e></e><e>  </e><n>12</n><z>0</z><y></y><x><a/></x><w> </w></r>"); var_dump(empty($s->e), empty($s->e[1]), empty($s->e[2]), empty($s->n), empty($s->z), empty($s->y), empty($s->x), empty($s->w), empty($s->nope), empty($s["a"]), isset($s->e)); $s = simplexml_load_string("<r a=\"0\" b=\"\" c=\"x\"/>"); var_dump(empty($s["a"]), empty($s["b"]), empty($s["c"]), empty($s["d"]), isset($s["b"]));
$d = new DOMDocument; $d->loadXML("<r><a/>0</r>"); var_dump(empty($d->documentElement->firstChild->textContent), empty($d->documentElement->lastChild->data), empty($d->documentElement->firstChild), empty($d->nope));
--EXPECT--
bool(true)
bool(true)
bool(false)
bool(false)
bool(true)
bool(true)
bool(false)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
