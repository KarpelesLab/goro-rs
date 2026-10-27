--TEST--
writes of typed properties from a site run many times (exact types and coercions)
--FILE--
<?php
interface I {}
class Base {}
class Sub extends Base implements I {}
class T {
    public int $i;
    public float $f;
    public ?string $s = null;
    public bool $b = false;
    public ?Base $o = null;
    public I $iface;
    public int|string $u = 0;
    public array $a = [];
    public mixed $m;
    protected int $prot = 0;
    public readonly int $ro;
    public function set($v) { $this->i = $v; }
    public function setF($v) { $this->f = $v; }
    public function setS($v) { $this->s = $v; }
    public function setO($v) { $this->o = $v; }
    public function setI($v) { $this->iface = $v; }
    public function setProt($v) { $this->prot = $v; return $this->prot; }
    public function setRo($v) { $this->ro = $v; }
}
class M {
    public int $x;
    public function __set($n, $v) { echo "__set($n)\n"; }
}
$vals = [1, 2.0, 2.5, "3", "x", null, true, [1], new Sub, new Base];
for ($r = 0; $r < 2; $r++) {
    foreach ($vals as $v) {
        $t = new T;
        foreach (['set', 'setF', 'setS', 'setO', 'setI', 'setProt'] as $m) {
            try {
                $res = $t->$m($v);
                echo $m, ': ok ';
            } catch (TypeError $e) {
                echo $m, ': ', $e->getMessage(), ' ';
            }
        }
        try { $t->u = $v; echo 'u ok '; } catch (TypeError $e) { echo 'u: ', $e->getMessage(), ' '; }
        try { $t->a = $v; echo 'a ok '; } catch (TypeError $e) { echo 'a: ', $e->getMessage(), ' '; }
        $t->m = $v;
        try { $t->b = $v; echo 'b ok'; } catch (TypeError $e) { echo 'b: ', $e->getMessage(); }
        echo "\n";
        var_dump($t->i ?? 'unset', $t->f ?? 'unset', $t->s, $t->u, $t->b);
    }
    $t = new T;
    $t->setRo(1);
    try { $t->setRo(2); } catch (Error $e) { echo $e->getMessage(), "\n"; }
    try { $t->prot = 5; } catch (Error $e) { echo $e->getMessage(), "\n"; }
    $m = new M;
    $m->x = 1;
    unset($m->x);
    $m->x = 2;
    var_dump(isset($m->x));
    $m2 = new M;
    $m2->x = 3;
    var_dump($m2->x);
}
?>
--EXPECTF--
set: ok setF: ok setS: ok setO: Cannot assign int to property T::$o of type ?Base setI: Cannot assign int to property T::$iface of type I setProt: ok u ok a: Cannot assign int to property T::$a of type array b ok
int(1)
float(1)
string(1) "1"
int(1)
bool(true)
set: ok setF: ok setS: ok setO: Cannot assign float to property T::$o of type ?Base setI: Cannot assign float to property T::$iface of type I setProt: ok u ok a: Cannot assign float to property T::$a of type array b ok
int(2)
float(2)
string(1) "2"
int(2)
bool(true)

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 17
set: ok setF: ok setS: ok setO: Cannot assign float to property T::$o of type ?Base setI: Cannot assign float to property T::$iface of type I 
Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 22
setProt: ok 
Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 41
u ok a: Cannot assign float to property T::$a of type array b ok
int(2)
float(2.5)
string(3) "2.5"
int(2)
bool(true)
set: ok setF: ok setS: ok setO: Cannot assign string to property T::$o of type ?Base setI: Cannot assign string to property T::$iface of type I setProt: ok u ok a: Cannot assign string to property T::$a of type array b ok
int(3)
float(3)
string(1) "3"
string(1) "3"
bool(true)
set: Cannot assign string to property T::$i of type int setF: Cannot assign string to property T::$f of type float setS: ok setO: Cannot assign string to property T::$o of type ?Base setI: Cannot assign string to property T::$iface of type I setProt: Cannot assign string to property T::$prot of type int u ok a: Cannot assign string to property T::$a of type array b ok
string(5) "unset"
string(5) "unset"
string(1) "x"
string(1) "x"
bool(true)
set: Cannot assign null to property T::$i of type int setF: Cannot assign null to property T::$f of type float setS: ok setO: ok setI: Cannot assign null to property T::$iface of type I setProt: Cannot assign null to property T::$prot of type int u: Cannot assign null to property T::$u of type string|int a: Cannot assign null to property T::$a of type array b: Cannot assign null to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: ok setF: ok setS: ok setO: Cannot assign true to property T::$o of type ?Base setI: Cannot assign true to property T::$iface of type I setProt: ok u ok a: Cannot assign true to property T::$a of type array b ok
int(1)
float(1)
string(1) "1"
int(1)
bool(true)
set: Cannot assign array to property T::$i of type int setF: Cannot assign array to property T::$f of type float setS: Cannot assign array to property T::$s of type ?string setO: Cannot assign array to property T::$o of type ?Base setI: Cannot assign array to property T::$iface of type I setProt: Cannot assign array to property T::$prot of type int u: Cannot assign array to property T::$u of type string|int a ok b: Cannot assign array to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: Cannot assign Sub to property T::$i of type int setF: Cannot assign Sub to property T::$f of type float setS: Cannot assign Sub to property T::$s of type ?string setO: ok setI: ok setProt: Cannot assign Sub to property T::$prot of type int u: Cannot assign Sub to property T::$u of type string|int a: Cannot assign Sub to property T::$a of type array b: Cannot assign Sub to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: Cannot assign Base to property T::$i of type int setF: Cannot assign Base to property T::$f of type float setS: Cannot assign Base to property T::$s of type ?string setO: ok setI: Cannot assign Base to property T::$iface of type I setProt: Cannot assign Base to property T::$prot of type int u: Cannot assign Base to property T::$u of type string|int a: Cannot assign Base to property T::$a of type array b: Cannot assign Base to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
Cannot modify readonly property T::$ro
Cannot access protected property T::$prot
__set(x)
bool(false)
int(3)
set: ok setF: ok setS: ok setO: Cannot assign int to property T::$o of type ?Base setI: Cannot assign int to property T::$iface of type I setProt: ok u ok a: Cannot assign int to property T::$a of type array b ok
int(1)
float(1)
string(1) "1"
int(1)
bool(true)
set: ok setF: ok setS: ok setO: Cannot assign float to property T::$o of type ?Base setI: Cannot assign float to property T::$iface of type I setProt: ok u ok a: Cannot assign float to property T::$a of type array b ok
int(2)
float(2)
string(1) "2"
int(2)
bool(true)

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 17
set: ok setF: ok setS: ok setO: Cannot assign float to property T::$o of type ?Base setI: Cannot assign float to property T::$iface of type I 
Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 22
setProt: ok 
Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 41
u ok a: Cannot assign float to property T::$a of type array b ok
int(2)
float(2.5)
string(3) "2.5"
int(2)
bool(true)
set: ok setF: ok setS: ok setO: Cannot assign string to property T::$o of type ?Base setI: Cannot assign string to property T::$iface of type I setProt: ok u ok a: Cannot assign string to property T::$a of type array b ok
int(3)
float(3)
string(1) "3"
string(1) "3"
bool(true)
set: Cannot assign string to property T::$i of type int setF: Cannot assign string to property T::$f of type float setS: ok setO: Cannot assign string to property T::$o of type ?Base setI: Cannot assign string to property T::$iface of type I setProt: Cannot assign string to property T::$prot of type int u ok a: Cannot assign string to property T::$a of type array b ok
string(5) "unset"
string(5) "unset"
string(1) "x"
string(1) "x"
bool(true)
set: Cannot assign null to property T::$i of type int setF: Cannot assign null to property T::$f of type float setS: ok setO: ok setI: Cannot assign null to property T::$iface of type I setProt: Cannot assign null to property T::$prot of type int u: Cannot assign null to property T::$u of type string|int a: Cannot assign null to property T::$a of type array b: Cannot assign null to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: ok setF: ok setS: ok setO: Cannot assign true to property T::$o of type ?Base setI: Cannot assign true to property T::$iface of type I setProt: ok u ok a: Cannot assign true to property T::$a of type array b ok
int(1)
float(1)
string(1) "1"
int(1)
bool(true)
set: Cannot assign array to property T::$i of type int setF: Cannot assign array to property T::$f of type float setS: Cannot assign array to property T::$s of type ?string setO: Cannot assign array to property T::$o of type ?Base setI: Cannot assign array to property T::$iface of type I setProt: Cannot assign array to property T::$prot of type int u: Cannot assign array to property T::$u of type string|int a ok b: Cannot assign array to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: Cannot assign Sub to property T::$i of type int setF: Cannot assign Sub to property T::$f of type float setS: Cannot assign Sub to property T::$s of type ?string setO: ok setI: ok setProt: Cannot assign Sub to property T::$prot of type int u: Cannot assign Sub to property T::$u of type string|int a: Cannot assign Sub to property T::$a of type array b: Cannot assign Sub to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
set: Cannot assign Base to property T::$i of type int setF: Cannot assign Base to property T::$f of type float setS: Cannot assign Base to property T::$s of type ?string setO: ok setI: Cannot assign Base to property T::$iface of type I setProt: Cannot assign Base to property T::$prot of type int u: Cannot assign Base to property T::$u of type string|int a: Cannot assign Base to property T::$a of type array b: Cannot assign Base to property T::$b of type bool
string(5) "unset"
string(5) "unset"
NULL
int(0)
bool(false)
Cannot modify readonly property T::$ro
Cannot access protected property T::$prot
__set(x)
bool(false)
int(3)
