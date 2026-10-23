--TEST--
setTimezone() with an uninitialised DateTimeZone keeps the zone
--FILE--
<?php
class Z extends DateTimeZone { function __construct() {} }
var_dump((new DateTime('2024-01-01'))->setTimezone(new Z)->format('c e'));
var_dump((new DateTimeImmutable('2024-01-01 Europe/Paris'))->setTimezone(new Z)->format('c e'));
var_dump(date_timezone_set(new DateTime('2024-01-01'), new Z)->format('c e'));
--EXPECT--
string(29) "2024-01-01T00:00:00+00:00 UTC"
string(38) "2024-01-01T00:00:00+01:00 Europe/Paris"
string(29) "2024-01-01T00:00:00+00:00 UTC"
