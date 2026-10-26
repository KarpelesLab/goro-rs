--TEST--
Comparison side effects of nested elements (notices, date compare handlers)
--FILE--
<?php
// zend_compare() of the elements: notices / compare handlers' warnings and
// exceptions, up to the first unequal pair (none when the counts differ)
set_error_handler(function ($n, $s) { echo "E: $s\n"; return true; });
var_dump([new stdClass] == [1]);
var_dump([1, new stdClass] == [2, 1]);
var_dump(['a' => new stdClass] == ['b' => 1]);
var_dump([new stdClass, 1] == [1]);
$o = new stdClass; $o->x = new stdClass; $p = new stdClass; $p->x = 1;
var_dump($o == $p);
var_dump([[new stdClass]] == [[1.5]]);
var_dump([new DateInterval('P1D'), new DateInterval('P1D')] == [new DateInterval('P1D'), new DateInterval('P1D')]);
var_dump([1, new DateInterval('P1D')] == [2, new DateInterval('P1D')]);
var_dump([new DateTime('2020-01-01'), new DateInterval('P1D')] == [new DateTime('2020-01-01'), new DateInterval('P1D')]);
var_dump([new DateTime('2020-01-01'), new DateInterval('P1D')] == [new DateTime('2020-01-02'), new DateInterval('P1D')]);
var_dump([[new DateInterval('P1D')], new DateInterval('P1D')] < [[new DateInterval('P1D')], new DateInterval('P1D')]);
try {
    var_dump([new DateTimeZone('UTC'), new DateInterval('P1D')] == [new DateTimeZone('+00:00'), new DateInterval('P1D')]);
} catch (Exception $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
var_dump([new DateTimeZone('UTC')] == [new DateTimeZone('UTC')]);
--EXPECT--
E: Object of class stdClass could not be converted to int
bool(true)
bool(false)
bool(false)
bool(false)
E: Object of class stdClass could not be converted to int
bool(true)
E: Object of class stdClass could not be converted to float
bool(false)
E: Cannot compare DateInterval objects
bool(false)
bool(false)
E: Cannot compare DateInterval objects
bool(false)
bool(false)
E: Cannot compare DateInterval objects
bool(false)
DateException: Cannot compare two different kinds of DateTimeZone objects
bool(true)
