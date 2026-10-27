--TEST--
Directory has no constructor: method_exists() is false, new throws
--FILE--
<?php
var_dump(method_exists('Directory', '__construct'), method_exists('Directory', 'read'));
try { new Directory(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$r = new ReflectionClass('Directory');
var_dump($r->getConstructor(), $r->hasMethod('__construct'), $r->isInstantiable());
try { $r->newInstance(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->newInstanceArgs([]); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$d = dir('.'); var_dump(get_class($d), is_callable([$d, '__construct']), method_exists($d, '__construct'));

?>
--EXPECT--
bool(false)
bool(true)
Error: Cannot directly construct Directory, use dir() instead
NULL
bool(false)
bool(true)
Error: Cannot directly construct Directory, use dir() instead
Error: Cannot directly construct Directory, use dir() instead
string(9) "Directory"
bool(false)
bool(false)
