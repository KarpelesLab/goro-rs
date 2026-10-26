--TEST--
A callback reaching __call() keeps its named arguments (with their names) in $arguments
--FILE--
<?php
class M {
    public function __call($m, $a) { echo $m, ' ', json_encode($a), "\n"; return $this; }
    public static function __callStatic($m, $a) { echo "static $m ", json_encode($a), "\n"; }
    public function real($years = 1, $x = 0) { echo "real $years $x\n"; }
}
$m = new M;
$p = ['years' => 5];
[$m, 'years'](...$p);
[$m, 'years'](1, ...['x' => 2]);
[$m, 'real'](...$p);
['M', 'stat'](...$p);
'M::stat'(...$p);
call_user_func_array([$m, 'years'], $p);
call_user_func([$m, 'years'], years: 6);
$c = [$m, 'years'];
$c(years: 7);
$c(...$p);
$m->years(...$p);
$f = $m->years(...);
$f(years: 8);

?>
--EXPECT--
years {"years":5}
years {"0":1,"x":2}
real 5 0
static stat {"years":5}
static stat {"years":5}
years {"years":5}
years {"years":6}
years {"years":7}
years {"years":5}
years {"years":5}
years {"years":8}
