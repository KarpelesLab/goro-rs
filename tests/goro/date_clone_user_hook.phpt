--TEST--
clone of date objects: __clone() runs on the uninitialized clone
--FILE--
<?php
// clone: a user __clone() runs before the date state is copied
#[AllowDynamicProperties]
class M extends DateTime {
    public $n = 0;
    function __clone() {
        var_dump($this);
        try { $this->format('Y'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
        $this->n++;
        $this->added = true;
    }
}
$m = new M('2020-01-01 UTC');
$c = clone $m;
var_dump($c, $c->format('Y-m-d'));
class Z extends DateTimeZone { function __clone() { var_dump($this); try { $this->getName(); } catch (Error $e) { echo $e->getMessage(), "\n"; } } }
var_dump(clone new Z('Europe/Paris'));
class I extends DateInterval { function __clone() { var_dump($this); } }
$i = clone new I('P1D');
var_dump($i->d);
class P extends DatePeriod { function __clone() { var_dump($this->recurrences, $this->start); } }
$p = clone new P(new DateTime('2020-01-01'), new DateInterval('P1D'), 2);
var_dump($p->recurrences);
--EXPECT--
object(M)#2 (1) {
  ["n"]=>
  int(0)
}
DateObjectError: Object of type M (inheriting DateTime) has not been correctly initialized by calling parent::__construct() in its constructor
object(M)#2 (5) {
  ["n"]=>
  int(1)
  ["added"]=>
  bool(true)
  ["date"]=>
  string(26) "2020-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
string(10) "2020-01-01"
object(Z)#4 (0) {
}
Object of type Z (inheriting DateTimeZone) has not been correctly initialized by calling parent::__construct() in its constructor
object(Z)#4 (2) {
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(12) "Europe/Paris"
}
object(I)#3 (0) {
}
int(1)
int(0)
NULL
int(3)
