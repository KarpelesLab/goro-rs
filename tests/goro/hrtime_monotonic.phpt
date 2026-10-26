--TEST--
hrtime() reads the monotonic clock (not the Unix time)
--FILE--
<?php
[$s, $ns] = hrtime();
var_dump($s < time() - 1000000, $ns >= 0 && $ns < 1000000000);
$a = hrtime(true);
$b = hrtime(true);
var_dump(is_int($a), $b >= $a, $a < (time() - 1000000) * 1000000000);
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
