--TEST--
createFromTimestamp() instantiates before the range check
--FILE--
<?php
// The object is instantiated before the range check fails.
try { DateTime::createFromTimestamp(NAN); } catch (Throwable $e) { echo get_class($e), "\n"; }
var_dump(spl_object_id($e));
$a = new stdClass;
var_dump(spl_object_id($a));
try { DateTimeImmutable::createFromTimestamp("x"); } catch (Throwable $e) { echo get_class($e), "\n"; }
var_dump(spl_object_id($e));
--EXPECT--
DateRangeError
int(2)
int(1)
TypeError
int(3)
