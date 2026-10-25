--TEST--
SimpleXML writes that create elements: offset warnings still assign, empty values
--FILE--
<?php foreach (["v", 1.5, 12, true, null, ""] as $v) { $s = simplexml_load_string("<r><j/></r>"); $s->j->n[1] = $v; $s->j->m[0] = $v; $s->j->o = $v; $s->a->b->c = $v; $s->j->q[] = $v; echo $s->asXML(); }
 $s = simplexml_load_string("<r><j/></r>"); $s->j->n[1] = "v"; echo $s->asXML(); $s->j->n[0] = "w"; echo $s->asXML(); $s->j->q[0] = "z"; echo $s->asXML(); $x = $s->j->zz; $x[1] = "y"; echo $s->asXML();
--EXPECTF--

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n>v</n><m>v</m><o>v</o><q>v</q></j><a><b><c>v</c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n>1.5</n><m>1.5</m><o>1.5</o><q>1.5</q></j><a><b><c>1.5</c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n>12</n><m>12</m><o>12</o><q>12</q></j><a><b><c>12</c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n>1</n><m>1</m><o>1</o><q>1</q></j><a><b><c>1</c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n/><m/><o></o><q/></j><a><b><c></c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 1
<?xml version="1.0"?>
<r><j><n/><m/><o></o><q/></j><a><b><c></c></b></a></r>

Warning: main(): Cannot add element n number 1 when only 0 such elements exist in %s on line 2
<?xml version="1.0"?>
<r><j><n>v</n></j></r>
<?xml version="1.0"?>
<r><j><n>w</n></j></r>
<?xml version="1.0"?>
<r><j><n>w</n><q>z</q></j></r>
<?xml version="1.0"?>
<r><j><n>w</n><q>z</q></j></r>
