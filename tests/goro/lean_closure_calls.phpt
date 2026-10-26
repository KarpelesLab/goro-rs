--TEST--
Closures called directly, run without a frame of their own while they may
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
// Closures called directly, their frames run without one of their own
// while they may.
$k = 3;
$add = function ($x) use ($k) { return $x + $k; };
$inc = function () use (&$k) { return ++$k; };
$bt = function ($a, $b = 2) { $t = debug_backtrace(); return count($t) . $t[0]['function'] . json_encode($t[0]['args']) . json_encode(func_get_args()); };
$st = function () { static $n = 0; return ++$n; };
$arrow = fn($x) => $x * $k;
$t = 0;
for ($i = 0; $i < 5; $i++) {
    $t += $add($i) + $inc() + $st() + $arrow($i);
}
var_dump($t, $k);
var_dump($bt(1), $bt(1, 5, 6));
$st2 = clone $st;
var_dump($st(), $st2(), $st());
class Box {
    private $v = 10;
    public function getter() { return function ($d) { return $this->v + $d; }; }
    public static function make() { return static function ($d) { return static::class . $d; }; }
    public function tracer() { return function () { return (new Exception)->getTraceAsString(); }; }
}
class SubBox extends Box {}
$g = (new Box)->getter();
$m = SubBox::make();
for ($i = 0; $i < 3; $i++) var_dump($g($i), $m($i));
echo (new Box)->tracer()(), "\n";
$fact = function ($n) use (&$fact) { return $n <= 1 ? 1 : $n * $fact($n - 1); };
var_dump($fact(10));
$thrower = function ($x) { if ($x > 2) throw new DomainException("x=$x"); return $x; };
try { for ($i = 0; $i < 5; $i++) echo $thrower($i), "\n"; } catch (DomainException $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$cur = function ($x) { return Closure::getCurrent() === $GLOBALS['cur2'] ? "same$x" : "other$x"; };
$cur2 = $cur;
var_dump($cur(1));
$gen = function ($n) { for ($i = 0; $i < $n; $i++) yield $i; };
foreach ($gen(3) as $v) echo $v;
echo "\n";
$typed = function (int $x): int { return $x * 2; };
var_dump($typed(4));
try { $typed("nope"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$arr = [1, 2, 3];
$modify = function ($a) { $a[] = 4; return count($a); };
var_dump($modify($arr), count($arr));
$byref = function (&$a) { $a[] = 9; };
$byref($arr);
var_dump(count($arr));
$dtor = function () { $o = new class { function __destruct() { echo "dtor\n"; } }; echo "body\n"; return 1; };
var_dump($dtor());
$deep = function ($n) use (&$deep) { return $n ? $deep($n - 1) : debug_backtrace(); };
var_dump(count($deep(3)));
$warn = function ($x) { return $x + $undef; };
var_dump($warn(1));
$badret = function ($x): array { return $x; };
try { $badret(1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_map($badret, [2]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$voidret = function (): void { return; };
var_dump($voidret(), array_map($voidret, [1]));
$intret = fn($x): int => $x;
var_dump($intret(3), array_map($intret, ["4"]));
--EXPECTF--
int(100)
int(8)
string(%d) "1{closure:%s:7}[1][1]"
string(%d) "1{closure:%s:7}[1,5,6][1,5,6]"
int(6)
int(6)
int(7)
int(10)
string(7) "SubBox0"
int(11)
string(7) "SubBox1"
int(12)
string(7) "SubBox2"
#0 %s(28): Box->{closure:Box::tracer():22}()
#1 {main}
int(3628800)
0
1
2
x=3
#0 %s(32): {closure:%s:31}(3)
#1 {main}
string(5) "same1"
012
int(8)
{closure:%s:39}(): Argument #1 ($x) must be of type int, string given, called in %s on line 41
int(4)
int(3)
int(4)
body
dtor
int(1)
int(4)

Warning: Undefined variable $undef in %s on line 52
int(1)
{closure:%s:54}(): Return value must be of type array, int returned
{closure:%s:54}(): Return value must be of type array, int returned
NULL
array(1) {
  [0]=>
  NULL
}
int(3)
array(1) {
  [0]=>
  int(4)
}
