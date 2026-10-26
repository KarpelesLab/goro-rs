--TEST--
#[\Deprecated] with an unknown named or extra argument throws when the declaration is used
--FILE--
<?php
#[\Deprecated(message: "a", since: "b", extra: 1)] function fx() {}
try { fx(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
#[\Deprecated("a", "b", "c")] function fy() {}
try { fy(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class K { #[\Deprecated(foo: 1)] const C = 1; #[\Deprecated(bar: 1)] function m() {} }
try { var_dump(K::C); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new K)->m(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
#[\Deprecated(zz: 1)] const GC = 1;
try { var_dump(GC); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo "done\n";
--EXPECT--
Error: Unknown named parameter $extra
ArgumentCountError: Deprecated::__construct() expects at most 2 arguments, 3 given
Error: Unknown named parameter $foo
Error: Unknown named parameter $bar
Error: Unknown named parameter $zz
done
