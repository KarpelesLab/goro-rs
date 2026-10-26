--TEST--
DatePeriod properties: fresh objects per property table, iterator handles
--FILE--
<?php
// DatePeriod's property tables get fresh objects each time
// (date_period_get_properties_for), none are kept in the object.
$p = new DatePeriod(new DateTime('2024-01-01'), new DateInterval('P1W'), 2);
var_dump($p);
$x = new stdClass;
var_dump(spl_object_id($x));
$a = (array)$p;
$b = get_object_vars($p);
var_dump($a['start'] === $b['start'], $a['start'] == $b['start']);
var_dump($p->start === $p->start, spl_object_id($p->interval));
$c = clone $p;
var_dump(count((array)$c), $c->recurrences);
var_dump(new DatePeriod(new DateTime('2024-01-01'), new DateInterval('P1D'), 1) == new DatePeriod(new DateTime('2025-01-01'), new DateInterval('P2D'), 3));
// foreach: one handle for the iterator; getIterator(): two
foreach ($p as $v) echo spl_object_id($v), ' ', spl_object_id(new stdClass), "\n";
$it = $p->getIterator();
echo spl_object_id($it), ' ', spl_object_id(new stdClass), "\n";
$c1 = $it->current();
$c2 = $it->current();
var_dump($c1 === $c2, spl_object_id($c1), spl_object_id($c2));
--EXPECT--
object(DatePeriod)#1 (7) {
  ["start"]=>
  object(DateTime)#3 (3) {
    ["date"]=>
    string(26) "2024-01-01 00:00:00.000000"
    ["timezone_type"]=>
    int(3)
    ["timezone"]=>
    string(3) "UTC"
  }
  ["current"]=>
  NULL
  ["end"]=>
  NULL
  ["interval"]=>
  object(DateInterval)#2 (10) {
    ["y"]=>
    int(0)
    ["m"]=>
    int(0)
    ["d"]=>
    int(7)
    ["h"]=>
    int(0)
    ["i"]=>
    int(0)
    ["s"]=>
    int(0)
    ["f"]=>
    float(0)
    ["invert"]=>
    int(0)
    ["days"]=>
    bool(false)
    ["from_string"]=>
    bool(false)
  }
  ["recurrences"]=>
  int(3)
  ["include_start_date"]=>
  bool(true)
  ["include_end_date"]=>
  bool(false)
}
int(2)
bool(false)
bool(true)
bool(false)
int(8)
int(7)
int(3)
bool(true)
9 11
11 9
9 11
11 7
bool(false)
int(7)
int(12)
