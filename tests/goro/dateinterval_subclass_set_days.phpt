--TEST--
A DateInterval subclass's __set() is called for days (only y, m, d, h, i, s, f, invert write the interval)
--FILE--
<?php
class I extends DateInterval {
    public function __set($n, $v) { echo "set($n, ", json_encode($v), ")\n"; }
}
$i = new I('P1D');
$i->days = -6;
$i->d = 3;
$i->from_string = true;
var_dump($i->days, $i->d);
$p = new DateInterval('P2D');
$p->d = 5;
var_dump($p->d);

?>
--EXPECT--
set(days, -6)
set(from_string, true)
bool(false)
int(3)
int(5)
