--TEST--
autoloaders are called for class names starting with a digit (zend_is_valid_class_name)
--FILE--
<?php
spl_autoload_register(function ($c) { echo "autoload($c)\n"; });
var_dump(class_exists('1bad'));
var_dump(class_exists('\\9Lead'));
var_dump(class_exists('a-b'));
var_dump(class_exists('a.b'));
try { new ('2x'); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
autoload(1bad)
bool(false)
autoload(9Lead)
bool(false)
bool(false)
bool(false)
autoload(2x)
Class "2x" not found
