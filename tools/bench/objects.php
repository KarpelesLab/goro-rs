<?php
// Objects: instantiation, property access, method calls, static calls.
class Point
{
    public function __construct(public float $x, public float $y) {}
    public function add(Point $o): Point { return new Point($this->x + $o->x, $this->y + $o->y); }
    public function len2(): float { return $this->x * $this->x + $this->y * $this->y; }
    public static function origin(): Point { return new Point(0.0, 0.0); }
}
$t = microtime(true);
$p = Point::origin();
$d = new Point(0.5, 0.25);
$acc = 0.0;
for ($i = 0; $i < 2000000; $i++) {
    $p = $p->add($d);
    $acc += $p->len2() * 1e-12;
    $p->x -= 0.25;
}
printf("%.6f %.2f in %.3fs\n", $acc, $p->y, microtime(true) - $t);
