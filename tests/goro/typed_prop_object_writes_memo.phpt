--TEST--
Writes of objects to class-typed properties follow the class table (late classes, aliases, unset slots, readonly)
--FILE--
<?php
interface Shape {}
class Circle implements Shape {}
class Square implements Shape {}
class Holder {
    public Shape $s;
    public ?Circle $c = null;
    public object $o;
    public Shape|int $u = 0;
    private Shape $p;
    function setP($v) { $this->p = $v; return get_class($this->p); }
}
class Magic {
    public Shape $s;
    function __set($n, $v) { echo "__set($n)\n"; }
}

function tryset($h, $prop, $v) {
    try {
        $h->$prop = $v;
        return get_class($h->$prop);
    } catch (TypeError $e) {
        return $e->getMessage();
    }
}

$h = new Holder;
$objs = [new Circle, new Square, new stdClass, new Circle];
for ($round = 0; $round < 3; $round++) {
    foreach ($objs as $o) {
        foreach (['s', 'c', 'o', 'u'] as $p) {
            echo $round, ' ', $p, ' ', get_class($o), ': ', tryset($h, $p, $o), "\n";
        }
        echo '  p: ';
        try { echo $h->setP($o), "\n"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    }
    if ($round == 0) {
        // a class declared after the first writes
        eval('class Triangle implements Shape {} class Oval extends Circle {}');
        $objs[] = new Triangle;
        $objs[] = new Oval;
    }
    if ($round == 1) {
        class_alias('Square', 'Box');
        $objs[] = new Box;
    }
}

// an unset typed property: __set runs
$m = new Magic;
unset($m->s);
$m->s = new Circle;
var_dump(isset($m->s));
$m2 = new Magic;
$m2->s = new Square;
var_dump(get_class($m2->s));

// in a loop (the same write site)
$h2 = new Holder;
$all = [new Circle, new Square, new stdClass, new Oval, new Triangle, new Box];
foreach ([1, 2] as $_) {
    foreach ($all as $o) {
        try {
            $h2->s = $o;
            echo get_class($h2->s), ' ';
        } catch (TypeError $e) {
            echo 'TypeError ';
        }
    }
    echo "\n";
}

// readonly
class RO { public function __construct(public readonly Shape $s) {} }
$r = new RO(new Circle);
try { $r->s = new Square; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
var_dump(get_class($r->s));
?>
--EXPECT--
0 s Circle: Circle
0 c Circle: Circle
0 o Circle: Circle
0 u Circle: Circle
  p: Circle
0 s Square: Square
0 c Square: Cannot assign Square to property Holder::$c of type ?Circle
0 o Square: Square
0 u Square: Square
  p: Square
0 s stdClass: Cannot assign stdClass to property Holder::$s of type Shape
0 c stdClass: Cannot assign stdClass to property Holder::$c of type ?Circle
0 o stdClass: stdClass
0 u stdClass: Cannot assign stdClass to property Holder::$u of type Shape|int
  p: Cannot assign stdClass to property Holder::$p of type Shape
0 s Circle: Circle
0 c Circle: Circle
0 o Circle: Circle
0 u Circle: Circle
  p: Circle
1 s Circle: Circle
1 c Circle: Circle
1 o Circle: Circle
1 u Circle: Circle
  p: Circle
1 s Square: Square
1 c Square: Cannot assign Square to property Holder::$c of type ?Circle
1 o Square: Square
1 u Square: Square
  p: Square
1 s stdClass: Cannot assign stdClass to property Holder::$s of type Shape
1 c stdClass: Cannot assign stdClass to property Holder::$c of type ?Circle
1 o stdClass: stdClass
1 u stdClass: Cannot assign stdClass to property Holder::$u of type Shape|int
  p: Cannot assign stdClass to property Holder::$p of type Shape
1 s Circle: Circle
1 c Circle: Circle
1 o Circle: Circle
1 u Circle: Circle
  p: Circle
1 s Triangle: Triangle
1 c Triangle: Cannot assign Triangle to property Holder::$c of type ?Circle
1 o Triangle: Triangle
1 u Triangle: Triangle
  p: Triangle
1 s Oval: Oval
1 c Oval: Oval
1 o Oval: Oval
1 u Oval: Oval
  p: Oval
2 s Circle: Circle
2 c Circle: Circle
2 o Circle: Circle
2 u Circle: Circle
  p: Circle
2 s Square: Square
2 c Square: Cannot assign Square to property Holder::$c of type ?Circle
2 o Square: Square
2 u Square: Square
  p: Square
2 s stdClass: Cannot assign stdClass to property Holder::$s of type Shape
2 c stdClass: Cannot assign stdClass to property Holder::$c of type ?Circle
2 o stdClass: stdClass
2 u stdClass: Cannot assign stdClass to property Holder::$u of type Shape|int
  p: Cannot assign stdClass to property Holder::$p of type Shape
2 s Circle: Circle
2 c Circle: Circle
2 o Circle: Circle
2 u Circle: Circle
  p: Circle
2 s Triangle: Triangle
2 c Triangle: Cannot assign Triangle to property Holder::$c of type ?Circle
2 o Triangle: Triangle
2 u Triangle: Triangle
  p: Triangle
2 s Oval: Oval
2 c Oval: Oval
2 o Oval: Oval
2 u Oval: Oval
  p: Oval
2 s Square: Square
2 c Square: Cannot assign Square to property Holder::$c of type ?Circle
2 o Square: Square
2 u Square: Square
  p: Square
__set(s)
bool(false)
string(6) "Square"
Circle Square TypeError Oval Triangle Square 
Circle Square TypeError Oval Triangle Square 
Error: Cannot modify readonly property RO::$s
string(6) "Circle"
