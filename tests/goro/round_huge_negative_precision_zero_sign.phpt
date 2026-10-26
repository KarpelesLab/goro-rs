--TEST--
round(): a huge negative precision gives an unsigned zero
--FILE--
<?php
// round() with a huge negative precision takes the snprintf()/strtod() path,
// where php's snprintf() drops the sign of -0.0
var_dump(round(-1, PHP_INT_MIN), round(-2.5, PHP_INT_MIN), round(-3.5, -30), round(2.5, PHP_INT_MIN));
--EXPECT--
float(0)
float(0)
float(0)
float(0)
