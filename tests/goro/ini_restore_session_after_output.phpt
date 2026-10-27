--TEST--
ini_restore() of a session entry after output: the warning names ini_restore()
--FILE--
<?php
ini_set("session.name", "zz");
echo "x\n";
ini_restore("session.name");
var_dump(ini_get("session.name"));
--EXPECTF--
x

Warning: ini_restore(): Session ini settings cannot be changed after headers have already been sent (sent from %s on line 3) in %s on line 4
string(2) "zz"
