--TEST--
DateTime::diff() days overflow the C int like cvttsd2si
--FILE--
<?php
$i=(new DateTime('2024-01-01'))->diff(new DateTime('@'.PHP_INT_MAX)); var_dump($i->days, $i->format('%a'));
$i=(new DateTime('@'.PHP_INT_MIN))->diff(new DateTime('@'.PHP_INT_MAX)); var_dump($i->days, $i->format('%a'));
$i=(new DateTime('2024-01-01 UTC'))->diff(new DateTime('3024-01-01 Europe/Paris')); var_dump($i->days);
--EXPECT--
int(-2147483648)
string(11) "-2147483648"
int(1158050441)
string(10) "1158050441"
int(365241)
