--TEST--
error_log() type 4 goes to the SAPI logger after what was printed so far
--FILE--
<?php
echo "a\n";
error_log("x", 4);
echo "b\n";
--EXPECT--
a
x
b
