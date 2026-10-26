--TEST--
DateTimeImmutable::add()/sub()/setTimezone() check the argument count before the types
--FILE--
<?php
$d = new DateTimeImmutable('2020-01-01');
foreach (['add', 'sub', 'setTimezone'] as $m) {
    try { $d->$m(1, 2); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { $d->$m(1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--

Warning: The return value of method DateTimeImmutable::add() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::add() does not modify the object itself in %s on line 4
ArgumentCountError: DateTimeImmutable::add() expects exactly 1 argument, 2 given

Warning: The return value of method DateTimeImmutable::add() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::add() does not modify the object itself in %s on line 5
TypeError: DateTimeImmutable::add(): Argument #1 ($interval) must be of type DateInterval, int given

Warning: The return value of method DateTimeImmutable::sub() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::sub() does not modify the object itself in %s on line 4
ArgumentCountError: DateTimeImmutable::sub() expects exactly 1 argument, 2 given

Warning: The return value of method DateTimeImmutable::sub() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::sub() does not modify the object itself in %s on line 5
TypeError: DateTimeImmutable::sub(): Argument #1 ($interval) must be of type DateInterval, int given

Warning: The return value of method DateTimeImmutable::setTimezone() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::setTimezone() does not modify the object itself in %s on line 4
ArgumentCountError: DateTimeImmutable::setTimezone() expects exactly 1 argument, 2 given

Warning: The return value of method DateTimeImmutable::setTimezone() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::setTimezone() does not modify the object itself in %s on line 5
TypeError: DateTimeImmutable::setTimezone(): Argument #1 ($timezone) must be of type DateTimeZone, int given
