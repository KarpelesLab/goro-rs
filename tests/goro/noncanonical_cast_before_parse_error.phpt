--TEST--
Non-canonical cast deprecations of the code read before a parse error are shown
--FILE--
<?php
echo "x\n";
var_dump((boolean)1, (real)1);
--EXPECTF--

Deprecated: Non-canonical cast (boolean) is deprecated, use the (bool) cast instead in %s on line 3

Parse error: The (real) cast has been removed, use (float) instead in %s on line 3
