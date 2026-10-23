--TEST--
Arrow functions capture variables used in any sub-expression
--FILE--
<?php
class K {
    public static $sp = 'SP';
    public $p = 'P';
    const C = 'CC';
    public $v;
    function __construct($v = null) { $this->v = $v; }
    function m($x) { return "m$x"; }
    static function sm() { return 'SM'; }
}
$b = 5; $o = new K; $n = 'p'; $cls = 'K'; $meth = 'sm'; $arr = [1, 2];
var_dump((fn() => "v=$b {$o->p} {$n}")());
var_dump((fn() => isset($b, $o->p))(), (fn() => empty($b))(), (fn() => @$b)());
var_dump((fn() => new K($b))()->v, (fn() => new $cls)() instanceof K, (fn() => new class($b) { function __construct(public $q) {} })()->q);
var_dump((fn() => $o->$n)(), (fn() => $cls::$sp)(), (fn() => $cls::C)(), (fn() => $cls::sm())(), (fn() => K::$meth())());
var_dump((fn() => $o instanceof $cls)(), (fn() => $o->m(...)($b))(), (fn() => $b |> strval(...))());
var_dump((fn() => <<<EOT
h $b
EOT)());
var_dump((fn() => [...$arr, $b])(), (fn() => $o?->p)(), (fn() => print(''))());
--EXPECT--
string(7) "v=5 P p"
bool(true)
bool(false)
int(5)
int(5)
bool(true)
int(5)
string(1) "P"
string(2) "SP"
string(2) "CC"
string(2) "SM"
string(2) "SM"
bool(true)
string(2) "m5"
string(1) "5"
string(3) "h 5"
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(5)
}
string(1) "P"
int(1)
