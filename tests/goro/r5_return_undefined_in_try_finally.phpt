--TEST--
return of an undefined variable inside try/finally warns (and an exception goes through finally)
--FILE--
<?php
function f0() { try { return $undef; } finally { echo "finally0\n"; } }
var_dump(f0());
set_error_handler(function($n, $s) { throw new Exception("h: $s"); });
function f2() { try { return $undef; } finally { echo "finally2\n"; } }
try { f2(); } catch (Exception $e) { echo "c2 ", $e->getMessage(), "\n"; }
function f3() { try { try { return $undef; } finally { echo "inner finally\n"; } } catch (Exception $e) { echo "outer catch ", $e->getMessage(), "\n"; return 3; } }
var_dump(f3());
echo "end\n";
--EXPECTF--

Warning: Undefined variable $undef in %s on line 2
finally0
NULL
finally2
c2 h: Undefined variable $undef
inner finally
outer catch h: Undefined variable $undef
int(3)
end
