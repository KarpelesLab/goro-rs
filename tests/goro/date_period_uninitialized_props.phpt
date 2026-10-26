--TEST--
DatePeriod: fields of an uninitialized period
--FILE--
<?php
$p = (new ReflectionClass('DatePeriod'))->newInstanceWithoutConstructor();
var_dump($p->start, $p->current, $p->end, $p->interval, $p->recurrences, $p->include_start_date, $p->include_end_date);
var_dump(isset($p->start), isset($p->recurrences));
var_dump($p);
try { foreach ($p as $x) {} } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { $p->getStartDate(); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
var_dump($p->getRecurrences());
--EXPECT--
NULL
NULL
NULL
NULL
int(0)
bool(false)
bool(false)
bool(false)
bool(false)
object(DatePeriod)#2 (0) {
}
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor
NULL
