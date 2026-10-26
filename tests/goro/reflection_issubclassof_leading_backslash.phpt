--TEST--
ReflectionClass::isSubclassOf()/implementsInterface() with a leading backslash (hoa/event in phpstan.phar)
--FILE--
<?php
class A {} interface I {} class B extends A implements I {}
$r = new ReflectionClass("B");
var_dump($r->isSubclassOf("\\A"), $r->implementsInterface("\\I"), $r->isSubclassOf("\\I"));
foreach (["isSubclassOf", "implementsInterface"] as $m) foreach (["\\Nope", "\\a", "\\Countable", "\\stdclass"] as $n) { try { var_dump($r->$m($n)); } catch (Exception $e) { echo $e->getMessage(), "\n"; } }

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
Class "\Nope" does not exist
bool(true)
bool(false)
bool(false)
Interface "\Nope" does not exist
A is not an interface
bool(false)
stdClass is not an interface
