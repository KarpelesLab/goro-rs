--TEST--
$GLOBALS[] is a compile error in any fetch context
--FILE--
<?php
echo "not reached\n";
function f(&$a) {}
f($GLOBALS[]);
?>
--EXPECTF--
Fatal error: Cannot append to $GLOBALS in %s on line %d
