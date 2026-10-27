--TEST--
max_execution_time in a for loop is reported at the loop body (php-src's back-edge target)
--FILE--
<?php

set_time_limit(1);

$y = 0;
for ($i = 0; $i < INF; $i++) {
    $y++;
}

?>
never reached here
--EXPECTF--

Fatal error: Maximum execution time of 1 second exceeded in %s on line 7
