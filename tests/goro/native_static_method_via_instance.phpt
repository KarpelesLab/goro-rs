--TEST--
A static method of an internal class called through an instance gets no $this argument
--FILE--
<?php
$d = new DateTime('2020-01-01');
var_dump($d->getLastErrors());
var_dump($d->createFromFormat('Y-m-d', '2021-02-03')->format('Y'));
$tz = new DateTimeZone('UTC');
var_dump(count($tz->listIdentifiers()) > 100);
$i = new DateTimeImmutable('2020-01-01');
var_dump($i->getLastErrors());
var_dump(get_class($i->createFromMutable($d)));
$iv = new DateInterval('P1D');
var_dump($iv->createFromDateString('1 day')->d);

?>
--EXPECT--
bool(false)
string(4) "2021"
bool(true)
bool(false)
string(17) "DateTimeImmutable"
int(1)
