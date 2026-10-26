--TEST--
array_walk() on date objects: no view fields
--FILE--
<?php
date_default_timezone_set('UTC');
$cb = function ($v, $k) { echo "walk $k\n"; };
$x = new DateTime("2024-01-01"); array_walk($x, $cb);
$d = new DateTime("2024-01-01"); @$d->foo = 1; array_walk($d, $cb);
$z = new DateTimeZone('UTC'); array_walk($z, $cb);
$i = new DateInterval('P1D'); array_walk($i, $cb);
print_r($i); array_walk($i, $cb);
$p = new DatePeriod(new DateTime('2024-01-01'), new DateInterval('P1D'), 1); array_walk($p, $cb);
var_dump(property_exists($p, 'start'), property_exists($p, 'nope'), isset($p->start), isset($p->current));
$r = new ReflectionObject($p); foreach ($r->getProperties() as $pp) echo $pp->getName(), ' '; echo "\n";
$rp = new ReflectionProperty($p, 'recurrences'); var_dump($rp->getValue($p), $rp->isInitialized($p));
foreach ($p as $k => $v) echo $k, "\n";
$q = new class(new DateTime('2024-01-01'), new DateInterval('P1D'), 1) extends DatePeriod { public $own = 5; };
array_walk($q, $cb);
var_dump(get_object_vars($q));
--EXPECT--
walk foo
walk y
walk m
walk d
walk h
walk i
walk s
walk f
walk invert
walk days
walk from_string
DateInterval Object
(
    [y] => 0
    [m] => 0
    [d] => 1
    [h] => 0
    [i] => 0
    [s] => 0
    [f] => 0
    [invert] => 0
    [days] => 
    [from_string] => 
)
walk y
walk m
walk d
walk h
walk i
walk s
walk f
walk invert
walk days
walk from_string
bool(true)
bool(false)
bool(true)
bool(false)
start current end interval recurrences include_start_date include_end_date 
int(2)
bool(true)
0
1
walk own
array(8) {
  ["own"]=>
  int(5)
  ["start"]=>
  object(DateTime)#9 (3) {
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
  object(DateInterval)#11 (10) {
    ["y"]=>
    int(0)
    ["m"]=>
    int(0)
    ["d"]=>
    int(1)
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
  int(2)
  ["include_start_date"]=>
  bool(true)
  ["include_end_date"]=>
  bool(false)
}
