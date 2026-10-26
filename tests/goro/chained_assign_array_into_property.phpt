--TEST--
$o->p = $x = [] gives the property its own copy of the array (nette/di Autowiring in phpstan)
--FILE--
<?php
// nette/di Autowiring::rebuild(): $this->lowPriority = $this->highPriority = $preferred = [];
// then writes to $preferred must not show in the properties
class P {
    public $a = []; public $b = [];
    function f1() { $this->b = $x = []; $x[] = 1; return [$this->b, $x]; }
    function f2() { $this->b = $x = [5]; $x[] = 1; return [$this->b, $x]; }
    function f3() { $x = []; $this->b = $x; $x[] = 1; return [$this->b, $x]; }
    function f4() { $this->a = $this->b = []; $this->b[] = 2; return [$this->a, $this->b]; }
    function f5() { $this->b = $x = []; $this->b[] = 1; return [$this->b, $x]; }
    function f6() { $y = $x = []; $x[] = 1; return [$y, $x]; }
    function f7() { $this->a = $this->b = $p = []; $p['t'] = true; $this->b['h'] = 1; return [$this->a, $this->b, $p]; }
}
$p = new P;
foreach (['f1', 'f2', 'f3', 'f4', 'f5', 'f6', 'f7'] as $f) echo $f, ' ', json_encode($p->$f()), "\n";
$o = new stdClass;
$o->b = $x = []; $x[] = 1; echo json_encode([$o->b, $x]), "\n";
$arr = [];
$arr['k'] = $z = []; $z[] = 1; echo json_encode([$arr, $z]), "\n";
class S { static $s = []; static function f() { self::$s = $x = [1]; $x[] = 2; return [self::$s, $x]; } }
echo json_encode(S::f()), "\n";
function gen() { $o = new stdClass; $o->p = $x = ['a']; $x[] = 'b'; yield [$o->p, $x]; $arr = []; $arr[0] = $y = []; $y[] = 1; yield [$arr, $y]; }
foreach (gen() as $v) echo json_encode($v), "\n";
$r = [];
$ref = &$r;
$o2 = new stdClass; $o2->q = $r = [7]; $r[] = 8; echo json_encode([$o2->q, $r, $ref]), "\n";
$k = (function () { $a = $b = [1]; $b[] = 2; return [$a, $b]; })(); echo json_encode($k), "\n";

?>
--EXPECT--
f1 [[],[1]]
f2 [[5],[5,1]]
f3 [[],[1]]
f4 [[],[2]]
f5 [[1],[]]
f6 [[],[1]]
f7 [[],{"h":1},{"t":true}]
[[],[1]]
[{"k":[]},[1]]
[[1],[1,2]]
[["a"],["a","b"]]
[[[]],[1]]
[[7],[7,8],[7,8]]
[[1],[1,2]]
