--TEST--
Closures created in static methods called back by internal functions get their arguments
--FILE--
<?php
class T {
    static function map($v) {
        return array_map(function ($m) use ($v) { return $m . $v; }, ['a', 'b']);
    }
    static function cb($v) {
        return preg_replace_callback('/b/', function ($m) use ($v) { return $m[0] . $v; }, 'abc');
    }
    static function ucf() {
        $f = function ($m, $n = 'default') { return "$m $n"; };
        return call_user_func($f, 'x');
    }
    function inst($v) {
        return array_map(function ($m) use ($v) { return $m . $v . get_class($this); }, ['c']);
    }
}
var_dump(T::map('!'), T::cb('?'), T::ucf(), (new T)->inst('.'));
?>
--EXPECT--
array(2) {
  [0]=>
  string(2) "a!"
  [1]=>
  string(2) "b!"
}
string(4) "ab?c"
string(9) "x default"
array(1) {
  [0]=>
  string(3) "c.T"
}
