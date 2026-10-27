--TEST--
max_execution_time in a while loop is reported at the loop body
--FILE--
<?php
set_time_limit(1);
$y = 0;
while (true)
{
    $y++;
    $y--;
}
--EXPECTF--

Fatal error: Maximum execution time of 1 second exceeded in %s on line 6
