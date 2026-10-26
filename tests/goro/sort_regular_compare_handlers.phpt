--TEST--
sort(): compare handlers' warnings/exceptions of date objects
--FILE--
<?php
set_error_handler(function ($n, $s) { echo "E: $s\n"; return true; });
$i = [new DateInterval('P2D'), new DateInterval('P1D')];
sort($i);
echo count($i), "\n";
$a = [[new DateInterval('P2D')], [new DateInterval('P1D')]];
sort($a);
$z = [new DateTimeZone('UTC'), new DateTimeZone('+01:00')];
try { sort($z); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$d = [new DateTime('2024-03-01'), new DateTime('2023-01-01')];
sort($d);
echo $d[0]->format('Y'), "\n";
--EXPECT--
E: Cannot compare DateInterval objects
2
E: Cannot compare DateInterval objects
DateException: Cannot compare two different kinds of DateTimeZone objects
2023
