--TEST--
strftime() %s is mktime() of the broken-down time
--FILE--
<?php
// %s is the C library's mktime() of the local fields, in the TZ environment's zone
$old = getenv('TZ');
putenv('TZ=UTC');
date_default_timezone_set('Europe/Paris');
var_dump(@strftime('%s', 1704112496), @gmstrftime('%s', 1704112496));
putenv('TZ=Asia/Tokyo');
var_dump(@strftime('%s %10s', 1704112496));
putenv($old === false ? 'TZ' : "TZ=$old");
--EXPECT--
string(10) "1704116096"
string(10) "1704112496"
string(21) "1704083696 1704083696"
