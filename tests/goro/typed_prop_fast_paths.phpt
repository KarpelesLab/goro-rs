--TEST--
Typed properties written in loops, typed references alive while loops run, protected properties from related scopes, PhpToken reads
--FILE--
<?php
class P {}
class Q extends P {}
final class T {
    private int $i = 0;
    private float $f = 0.0;
    private ?string $s = null;
    private array $a = [];
    private bool $b = false;
    private ?P $p = null;
    private int|string $u = 0;
    public int $pub = 0;
    function run(int $n) {
        for ($k = 0; $k < $n; $k++) {
            $this->i += 2;
            $this->i++;
            $this->f = $k;          // int to float: coerced
            $this->f += 0.5;
            $this->s = "s$k";
            $this->a[] = $k;
            $this->a['k'] = $k;
            $this->b = !$this->b;
            $this->p = $k % 2 ? new Q : new P;
            $this->u = $k % 2 ? "x" : $k;
        }
        return [$this->i, $this->f, $this->s, count($this->a), $this->b, get_class($this->p), $this->u];
    }
    function bad($what) {
        try {
            switch ($what) {
                case 0: $this->i = "nope"; break;
                case 1: $this->i += 1.5; break;
                case 2: $this->p = new stdClass; break;
                case 3: $this->a = null; break;
                case 4: $this->s = []; break;
                case 5: $this->i = PHP_INT_MAX; $this->i++; break;
            }
        } catch (TypeError|ArithmeticError $e) {
            echo get_class($e), ": ", $e->getMessage(), "\n";
        }
        var_dump($this->i);
    }
}
$t = new T;
var_dump($t->run(1000));
for ($w = 0; $w < 6; $w++) $t->bad($w);

// typed references alive while plain loops run
$o = new T;
$r = &$o->pub;
$sum = 0;
for ($k = 0; $k < 1000; $k++) { $sum += $k; $r++; }
var_dump($sum, $o->pub);
try { $r = "abc"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
function bump(&$x) { for ($k = 0; $k < 3; $k++) { $x++; } $x = "str"; }
try { bump($o->pub); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($o->pub);
$r = 5.0;
var_dump($o->pub);

// protected properties read from the scopes of related classes
class A { protected $p = 'A::p'; protected int $q = 1; }
class B extends A { function get(A $o) { return [$o->p, $o->q]; } }
class C extends A {}
class D { function get(A $o) { return $o->p; } }
$b = new B;
for ($k = 0; $k < 3; $k++) { $res = [$b->get(new C), $b->get(new B), $b->get(new A)]; }
var_dump($res);
try { (new D)->get(new C); } catch (Error $e) { echo $e->getMessage(), "\n"; }

// PhpToken properties
class MyToken extends PhpToken { public $extra = 'e'; }
$ids = 0; $len = 0;
foreach (MyToken::tokenize('<?php echo 1 + 2;') as $tok) { $ids += $tok->id; $len += strlen($tok->text) + $tok->line + $tok->pos; $tok->extra .= 'x'; }
var_dump($ids > 0, $len, $tok->extra);
try { $tok->id = "x"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--
array(7) {
  [0]=>
  int(3000)
  [1]=>
  float(999.5)
  [2]=>
  string(4) "s999"
  [3]=>
  int(1001)
  [4]=>
  bool(false)
  [5]=>
  string(1) "Q"
  [6]=>
  string(1) "x"
}
TypeError: Cannot assign string to property T::$i of type int
int(3000)

Deprecated: Implicit conversion from float 3001.5 to int loses precision in %s on line 32
int(3001)
TypeError: Cannot assign stdClass to property T::$p of type ?P
int(3001)
TypeError: Cannot assign null to property T::$a of type array
int(3001)
TypeError: Cannot assign array to property T::$s of type ?string
int(3001)
TypeError: Cannot increment property T::$i of type int past its maximal value
int(9223372036854775807)
int(499500)
int(1000)
Cannot assign string to reference held by property T::$pub of type int
Cannot assign string to reference held by property T::$pub of type int
int(1003)
int(5)
array(3) {
  [0]=>
  array(2) {
    [0]=>
    string(4) "A::p"
    [1]=>
    int(1)
  }
  [1]=>
  array(2) {
    [0]=>
    string(4) "A::p"
    [1]=>
    int(1)
  }
  [2]=>
  array(2) {
    [0]=>
    string(4) "A::p"
    [1]=>
    int(1)
  }
}
Cannot access protected property C::$p
bool(true)
int(123)
string(2) "ex"
Cannot assign string to property PhpToken::$id of type int
