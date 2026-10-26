--TEST--
The max_execution_time fatal error is reported at the running line
--FILE--
<?php
set_time_limit(1);
function spin($n) { while (true) {} }
spin(3);
--EXPECTF--

Fatal error: Maximum execution time of 1 second exceeded in %s on line 3
