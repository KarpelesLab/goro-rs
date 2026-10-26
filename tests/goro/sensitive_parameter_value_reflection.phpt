--TEST--
ReflectionProperty::isInitialized() of SensitiveParameterValue::$value
--FILE--
<?php
$o = new SensitiveParameterValue('secret');
$r = new ReflectionClass($o::class);
var_dump($r->hasProperty('value'));
$p = $r->getProperty('value');
var_dump($p->isInitialized($o), $p->getValue($o), $p->isPrivate(), $p->isReadOnly());
var_dump($r->getProperties());
var_dump(get_object_vars($o));
var_dump((array) $o);

?>
--EXPECT--
bool(true)
bool(true)
string(6) "secret"
bool(true)
bool(true)
array(1) {
  [0]=>
  object(ReflectionProperty)#4 (2) {
    ["name"]=>
    string(5) "value"
    ["class"]=>
    string(23) "SensitiveParameterValue"
  }
}
array(0) {
}
array(0) {
}
