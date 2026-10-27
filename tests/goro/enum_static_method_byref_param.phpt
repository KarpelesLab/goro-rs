--TEST--
self::m($v) / static::m() / E::m() of an enum's static method with a by-reference parameter passes the variable by reference
--FILE--
<?php
enum E {
    case A;
    public static function t(array $v) { self::h($v); return $v; }
    public static function t2(array $v) { static::h($v); return $v; }
    public static function t3(array $v) { E::h($v); return $v; }
    public function t4(array $v) { $this->hi($v); return $v; }
    private static function h(array &$a) { $a[] = 9; }
    private function hi(array &$a) { $a[] = 8; }
    public static function t5(int $n) { self::hn($n); return $n; }
    private static function hn(int &$n) { $n++; }
}
var_dump(E::t([1]), E::t2([1]), E::t3([1]), E::A->t4([1]), E::t5(1));

?>
--EXPECTF--
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(9)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(9)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(9)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(8)
}
int(2)
