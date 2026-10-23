--TEST--
The date classes' __wakeup() is deprecated (8.5) and reads the real properties only
--FILE--
<?php
$objs = [
    new DateTime('2020-01-01'),
    new DateTimeImmutable('2020-01-01'),
    new DateTimeZone('UTC'),
    new DateInterval('P1D'),
    new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2),
];
foreach ($objs as $o) {
    try { $o->__wakeup(); echo get_class($o), " ok\n"; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
$r = new ReflectionMethod('DateTime', '__wakeup');
var_dump($r->isDeprecated());
class MyDT extends DateTime { function __wakeup(): void { echo "mine\n"; } }
$m = new MyDT('2020-01-01'); $m->__wakeup();
$d = unserialize(serialize(new DateTime('2020-01-01')));
var_dump($d->format('Y'));
$d2 = unserialize('O:8:"DateTime":3:{s:4:"date";s:26:"2020-01-01 00:00:00.000000";s:13:"timezone_type";i:3;s:8:"timezone";s:3:"UTC";}');
var_dump($d2->format('Y'));
$dt = new DateTime('2020-01-01'); $dt->foo = 1;
--EXPECTF--

Deprecated: Method DateTime::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 10
Error: Invalid serialization data for DateTime object

Deprecated: Method DateTimeImmutable::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 10
Error: Invalid serialization data for DateTimeImmutable object

Deprecated: Method DateTimeZone::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 10
Error: Invalid serialization data for DateTimeZone object

Deprecated: Method DateInterval::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 10
DateInterval ok

Deprecated: Method DatePeriod::__wakeup() is deprecated since 8.5, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 10
Error: Invalid serialization data for DatePeriod object
bool(true)
mine
string(4) "2020"
string(4) "2020"

Deprecated: Creation of dynamic property DateTime::$foo is deprecated in %s on line 20
