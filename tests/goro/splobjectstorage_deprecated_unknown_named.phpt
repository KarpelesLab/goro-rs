--TEST--
SplObjectStorage::attach()/detach(): unknown named argument error before the deprecation
--FILE--
<?php
// an unknown named argument fails at the send, before the deprecation
try { (new SplObjectStorage)->attach(foo: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new SplObjectStorage)->attach(new stdClass, inf: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new SplObjectStorage)->detach(foo: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new SplObjectStorage)->attach(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$s = new SplObjectStorage;
$s->attach(object: new stdClass, info: 2);
var_dump(count($s));
?>
--EXPECTF--
Error: Unknown named parameter $foo
Error: Unknown named parameter $inf
Error: Unknown named parameter $foo

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 6
ArgumentCountError: SplObjectStorage::attach() expects at least 1 argument, 0 given

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 8
int(1)
