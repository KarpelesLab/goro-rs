--TEST--
phpversion($ext) / ReflectionExtension::getVersion() give each loaded module's version (dom's DOM_API_VERSION), false for an unknown one
--EXTENSIONS--
dom
xml
--FILE--
<?php
var_dump(phpversion("dom"), phpversion("DOM"), (new ReflectionExtension("dom"))->getVersion());
var_dump((new ReflectionClass("DOMNodeList"))->getExtension()->getVersion());
var_dump(phpversion("xml") === PHP_VERSION, phpversion("Reflection") === PHP_VERSION, phpversion("standard") === PHP_VERSION);
var_dump(phpversion("no_such_extension"));
?>
--EXPECT--
string(8) "20031129"
string(8) "20031129"
string(8) "20031129"
string(8) "20031129"
bool(true)
bool(true)
bool(true)
bool(false)
