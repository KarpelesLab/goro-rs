--TEST--
filemtime()/fileatime() of a time before the epoch are negative
--FILE--
<?php
touch("/tmp/goro_touch_t", -1); var_dump(filemtime("/tmp/goro_touch_t"), fileatime("/tmp/goro_touch_t")); unlink("/tmp/goro_touch_t");
--EXPECT--
int(-1)
int(-1)
