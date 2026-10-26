--TEST--
$GLOBALS++ is a compile error
--FILE--
<?php
echo "not reached\n";
$GLOBALS++;
?>
--EXPECTF--
Fatal error: $GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax in %s on line %d
