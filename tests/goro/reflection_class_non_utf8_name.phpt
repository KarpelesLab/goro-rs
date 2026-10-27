--TEST--
ReflectionClass of a class whose name is not UTF-8
--FILE--
<?php
eval("class \xA9 { public \$v = 1; function m() {} }");
$r = new ReflectionClass("\xA9");
var_dump(bin2hex($r->getName()), bin2hex($r->name), $r->hasMethod("m"), count($r->getProperties()));
$o = $r->newInstance(); var_dump(bin2hex(get_class($o)), $r->isInstance($o));

?>
--EXPECT--
string(2) "a9"
string(2) "a9"
bool(true)
int(1)
string(2) "a9"
bool(true)
