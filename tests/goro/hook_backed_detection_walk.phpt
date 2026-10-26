--TEST--
Backed hooked property detection finds $this->prop anywhere in a hook body
--FILE--
<?php
class A {
    public $a = 1 { get { return "{$this->a}"; } }
    public $b = 2 { get { return "$this->b"; } }
    public $c = 3 { get { return <<<EOT
    v=$this->c
    EOT; } }
    public $d = 4 { get { return $this->d |> strval(...); } }
    public $e = 5 { get { return empty($this->e); } }
    public $f = 6 { get { [$v] = [$this->f]; return $v; } }
    public $g = 'stdClass' { get { return new ($this->g); } }
    public $h = 8 { get { return $this?->h; } }
    public $i { get { return fn() => $this->i; } }
    public $j { get { return function () { return $this->j; }; } }
}
$a = new A;
foreach (['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j'] as $p) {
    echo $p, ': ';
    var_dump((new ReflectionProperty('A', $p))->isVirtual());
}
var_dump($a->a, $a->b, $a->c, $a->d, $a->e, $a->f, $a->g, $a->h);
--EXPECT--
a: bool(false)
b: bool(false)
c: bool(false)
d: bool(false)
e: bool(false)
f: bool(false)
g: bool(false)
h: bool(false)
i: bool(true)
j: bool(true)
string(1) "1"
string(1) "2"
string(3) "v=3"
string(1) "4"
bool(false)
int(6)
object(stdClass)#2 (0) {
}
int(8)
