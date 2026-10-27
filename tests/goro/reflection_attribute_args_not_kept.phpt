--TEST--
ReflectionAttribute does not keep the arguments newInstance() evaluated (handles recycled like PHP)
--FILE--
<?php
#[Attribute(Attribute::TARGET_CLASS | Attribute::IS_REPEATABLE)]
class Attr { public function __construct(public $value) {} }
#[Attr(strlen(...))]
#[Attr(strrev(...))]
class D {}
$r = new ReflectionClass(D::class);
$as = $r->getAttributes();
$x = $as[0]->newInstance(); var_dump(spl_object_id($x), spl_object_id($x->value)); unset($x);
var_dump(spl_object_id(new stdClass));
$x = $as[1]->newInstance(); var_dump(spl_object_id($x), spl_object_id($x->value)); unset($x);
var_dump(spl_object_id(new stdClass));

$as = $r->getAttributes();
var_dump(spl_object_id($as[0]->newInstance()));
var_dump(spl_object_id($as[1]->newInstance()));
echo "--\n";
foreach ($as as $a) { var_dump(spl_object_id($a->newInstance())); }
echo "--\n";
foreach ($as as $a) { $o = $a->newInstance(); var_dump(spl_object_id($o), spl_object_id($o->value)); }
--EXPECT--
int(5)
int(4)
int(5)
int(4)
int(5)
int(4)
int(2)
int(3)
--
int(2)
int(3)
--
int(2)
int(3)
int(7)
int(6)
