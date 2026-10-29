<?php
// Objects: instantiation, method calls, property access, static calls.
interface Shape { public function area(): float; }
class Point {
    public function __construct(public float $x = 0.0, public float $y = 0.0) {}
    public function add(Point $o): Point { return new Point($this->x + $o->x, $this->y + $o->y); }
    public function len2(): float { return $this->x * $this->x + $this->y * $this->y; }
}
abstract class Base implements Shape {
    protected int $id;
    private static int $next = 0;
    public function __construct() { $this->id = self::$next++; }
    public function getId(): int { return $this->id; }
    abstract public function area(): float;
}
class Rect extends Base {
    public function __construct(private float $w, private float $h) { parent::__construct(); }
    public function area(): float { return $this->w * $this->h; }
}
class Circle extends Base {
    public function __construct(private float $r) { parent::__construct(); }
    public function area(): float { return 3.14159 * $this->r * $this->r; }
}
class Counter {
    private $n = 0;
    public function inc() { $this->n++; return $this; }
    public function get() { return $this->n; }
    public static function make() { return new static(); }
}
$t = hrtime(true);
$acc = new Point();
$d = new Point(0.5, 0.25);
for ($i = 0; $i < 1200000; $i++) { $acc = $acc->add($d); }
$sum = $acc->len2();
$shapes = [];
for ($i = 0; $i < 100000; $i++) { $shapes[] = $i % 2 ? new Rect($i % 10, 2) : new Circle($i % 7); }
$area = 0.0; $ids = 0;
for ($k = 0; $k < 20; $k++) {
    foreach ($shapes as $s) { $area += $s->area(); $ids += $s->getId(); }
}
$c = Counter::make();
for ($i = 0; $i < 2000000; $i++) { $c->inc(); }
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%.3f\n", $ms, $sum + $area + $ids + $c->get());
