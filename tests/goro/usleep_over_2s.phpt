--TEST--
usleep() sleeps the full duration over 2 seconds
--FILE--
<?php
$t = microtime(true); usleep(2500000); $d = microtime(true) - $t; var_dump($d >= 2.4 && $d < 4);

?>
--EXPECTF--
bool(true)
