--TEST--
clone() of a Generator: an object, uncloneable
--FILE--
<?php
$g = (function () { yield 1; })();
try { clone($g, []); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { clone($g); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { clone $g; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { clone($g, 5); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { array_map(clone(...), [$g]); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { call_user_func('clone', $g); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
Error: Trying to clone an uncloneable object of class Generator
Error: Trying to clone an uncloneable object of class Generator
Error: Trying to clone an uncloneable object of class Generator
TypeError: clone(): Argument #2 ($withProperties) must be of type array, int given
Error: Trying to clone an uncloneable object of class Generator
Error: Trying to clone an uncloneable object of class Generator
