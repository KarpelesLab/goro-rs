--TEST--
DateInterval from_string/date_string are properties once the property table was built
--FILE--
<?php
$i = DateInterval::createFromDateString('1 day');
var_dump(isset($i->from_string), isset($i->days), isset($i->d));
echo $i->from_string ?? 'unset', "\n";
print_r($i);
var_dump(isset($i->from_string));
echo $i->date_string ?? 'unset', "\n";
$j = new DateInterval('P1D');
echo $j->from_string ?? 'unset', "\n";
$x = (array)$j;
echo $j->from_string ?? 'unset', "\n";
var_dump($j->days);
$k = new DateInterval('P2D');
var_dump(get_object_vars($k)['from_string']);
var_dump($k->from_string);
$a = new DateTime('2024-01-01'); $b = new DateTime('2024-01-02');
$df = $a->diff($b);
var_dump(isset($df->from_string));
--EXPECT--
bool(false)
bool(true)
bool(true)
unset
DateInterval Object
(
    [from_string] => 1
    [date_string] => 1 day
)
bool(true)
1 day
unset

bool(false)
bool(false)
bool(false)
bool(false)
