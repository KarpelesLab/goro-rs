--TEST--
Comparisons of an object (cast to 1.0) with NAN are unordered (ZEND_THREEWAY_COMPARE)
--FILE--
<?php
$o = new stdClass; $n = NAN;
var_dump($o <= $n, $o >= $n, $o < $n, $o > $n, $o == $n, $o != $n, $o <=> $n, $n <=> $o, $n <= $o, $n < $o);
var_dump($o == 1, $o == 1.0, $o <=> 2, 2 <=> $o, $o < INF, $o > -INF);
var_dump([$o] <= [$n], max($o, $n), min($n, $o));
--EXPECTF--

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to float in %s on line 3
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
int(1)
int(1)
bool(false)
bool(false)

Notice: Object of class stdClass could not be converted to int in %s on line 4

Notice: Object of class stdClass could not be converted to float in %s on line 4

Notice: Object of class stdClass could not be converted to int in %s on line 4

Notice: Object of class stdClass could not be converted to int in %s on line 4

Notice: Object of class stdClass could not be converted to float in %s on line 4

Notice: Object of class stdClass could not be converted to float in %s on line 4
bool(true)
bool(true)
int(-1)
int(1)
bool(true)
bool(true)

Notice: Object of class stdClass could not be converted to float in %s on line 5

Notice: Object of class stdClass could not be converted to float in %s on line 5

Notice: Object of class stdClass could not be converted to float in %s on line 5
bool(false)
object(stdClass)#1 (0) {
}
object(stdClass)#1 (0) {
}
