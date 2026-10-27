--TEST--
zend_update_class_constants(): constants, then instance defaults, then static defaults, each evaluated once
--FILE--
<?php
$f = strrev(...); var_dump(spl_object_id($f));
class C { public static $d = strrev(...); const K = strlen(...); public $p = strtoupper(...); }
var_dump(spl_object_id(C::$d));
$o = new stdClass; var_dump(spl_object_id($o));
var_dump(spl_object_id(C::K));
$c = new C; var_dump(spl_object_id($c), spl_object_id($c->p));
class C2 { const K = strlen(...); }
var_dump(spl_object_id(C2::K));
var_dump(spl_object_id(new stdClass));
class D2 { public static $d = strrev(...); }
var_dump(spl_object_id(D2::$d));
var_dump(spl_object_id(new stdClass));
class E2 { public $p = strtoupper(...); }
$e = new E2; var_dump(spl_object_id($e), spl_object_id($e->p));
var_dump(spl_object_id(new stdClass));
--EXPECT--
int(1)
int(4)
int(5)
int(2)
int(6)
int(3)
int(7)
int(8)
int(8)
int(9)
int(10)
int(9)
int(11)
