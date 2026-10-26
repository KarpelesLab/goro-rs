--TEST--
Destructors of user date subclasses' objects made internally
--FILE--
<?php
class I extends DateTimeImmutable { function __destruct() { echo "dtor ", $this->format('Y-m-d'), "\n"; } }
class M extends DateTime { function __destruct() { echo "dtorM ", $this->format('Y-m-d'), "\n"; } }
class Z extends DateTimeZone { function __destruct() { echo "dtorZ\n"; } }
class V extends DateInterval { function __destruct() { echo "dtorV\n"; } }
$i = new I('2024-01-01');
$j = $i->modify('+1 day');
unset($j);
echo "a\n";
$k = I::createFromFormat('Y-m-d', '2024-05-05');
$k = null;
echo "b\n";
$m = M::createFromImmutable($i); unset($m);
echo "c\n";
$x = I::createFromMutable(new M('2020-02-02'));
unset($x);
echo "d\n";
$c = clone $i; unset($c);
echo "e\n";
$u = unserialize(serialize($i)); unset($u);
echo "f\n";
$s = I::__set_state(['date' => '2023-03-03 00:00:00.000000', 'timezone_type' => 3, 'timezone' => 'UTC']); unset($s);
echo "g\n";
$z = new Z('UTC'); $zz = clone $z; unset($zz);
echo "h\n";
$v = V::createFromDateString('1 day'); unset($v);
echo "i\n";
$t = I::createFromTimestamp(0); unset($t);
echo "j\n";
$i2 = $i->setTimezone(new DateTimeZone('UTC'))->add(new DateInterval('P1D'));
echo "k\n";
unset($i2);
echo "end\n";
--EXPECT--
dtor 2024-01-02
a
dtor 2024-05-05
b
dtorM 2024-01-01
c
dtorM 2020-02-02
dtor 2020-02-02
d
dtor 2024-01-01
e
dtor 2024-01-01
f
g
dtorZ
h
i
dtor 1970-01-01
j
dtor 2024-01-01
k
dtor 2024-01-02
end
dtorZ
dtor 2024-01-01
