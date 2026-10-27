--TEST--
Dynamic properties named like DateTime/DateTimeZone view properties are the script's own
--FILE--
<?php
$p = new DateTime("2024-01-01 10:00:00", new DateTimeZone("Europe/Paris"));
var_dump(isset($p->timezone));
$p->timezone = "abc";
var_dump($p->timezone, isset($p->timezone));
$p->modify("+1 day");
var_dump($p->timezone, $p->format("c"));
$p->date = "zz";
var_dump($p->date, $p->format("c"));
$c = clone $p; var_dump($c->date);
unset($p->timezone); var_dump(isset($p->timezone));
$z = new DateTimeZone("UTC"); $z->timezone = "q"; var_dump($z->timezone, $z->getName());
class D extends DateTime {
    public function __get($n) { return "magic:$n"; }
    public function __isset($n) { echo "isset($n)\n"; return true; }
    public function __set($n, $v) { echo "set($n, $v)\n"; }
    public function __unset($n) { echo "unset($n)\n"; }
}
$d = new D('2024-01-01');
var_dump(isset($d->timezone), isset($d->date), empty($d->timezone_type));
$d->timezone = 'Europe/Paris';
$d->date = 'x';
unset($d->timezone);
var_dump($d->timezone ?? 'none');
var_dump($d);
$p = new DateTime('2024-01-01');
var_dump(isset($p->timezone));
$p->timezone = 'abc';
var_dump($p->timezone);

?>
--EXPECT--
bool(false)
string(3) "abc"
bool(true)
string(3) "abc"
string(25) "2024-01-02T10:00:00+01:00"
string(2) "zz"
string(25) "2024-01-02T10:00:00+01:00"
string(2) "zz"
bool(false)
string(1) "q"
string(3) "UTC"
isset(timezone)
isset(date)
isset(timezone_type)
bool(true)
bool(true)
bool(false)
set(timezone, Europe/Paris)
set(date, x)
unset(timezone)
isset(timezone)
string(14) "magic:timezone"
object(D)#4 (3) {
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
bool(false)
string(3) "abc"
