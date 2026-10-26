--TEST--
Calling __call() explicitly passes its arguments as given (not packed as for a magic dispatch)
--FILE--
<?php
class P {
    public function __call($m, $a) { echo 'P ', $m, ' ', json_encode($a), "\n"; return 1; }
    public static function __callStatic($m, $a) { echo 'PS ', $m, ' ', json_encode($a), "\n"; return 2; }
}
class C extends P {
    public function __call($m, $a) { echo 'C ', $m, ' ', json_encode($a), "\n"; return parent::__call($m, $a); }
    public static function __callStatic($m, $a) { echo 'CS ', $m, ' ', json_encode($a), "\n"; return parent::__callStatic($m, $a); }
    public function explicit() { return $this->__call('ex', ['v']); }
    public static function sexplicit() { return static::__callStatic('sx', ['w']); }
}
$c = new C;
$c->foo(1, 2);
C::bar(3);
$c->explicit();
C::sexplicit();
$c->__call('direct', [4]);
C::__callStatic('sdirect', [5]);
call_user_func([$c, '__call'], 'cuf', [6]);
call_user_func_array([$c, '__call'], ['cufa', [7]]);
call_user_func('C::__callStatic', 'cufs', [8]);
$m = '__call';
$c->$m('dyn', [9]);
$f = $c->__call(...);
$f('fcc', [10]);
(new ReflectionMethod('C', '__call'))->invoke($c, 'refl', [11]);
$c->__call(...['spread', [12]]);
$c->__call(m: 'named', a: [13]);

?>
--EXPECT--
C foo [1,2]
P foo [1,2]
CS bar [3]
PS bar [3]
C ex ["v"]
P ex ["v"]
CS sx ["w"]
PS sx ["w"]
C direct [4]
P direct [4]
CS sdirect [5]
PS sdirect [5]
C cuf [6]
P cuf [6]
C cufa [7]
P cufa [7]
CS cufs [8]
PS cufs [8]
C dyn [9]
P dyn [9]
C fcc [10]
P fcc [10]
C refl [11]
P refl [11]
C spread [12]
P spread [12]
C named [13]
P named [13]
