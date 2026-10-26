--TEST--
\false / \true / \null in property, static property and parameter defaults; constant() / defined() of true/false/null (phpstan TurboExtensionEnabler)
--FILE--
<?php
namespace N;
final class T {
    private static bool $e = \false;
    private static $n = \null;
    public $p = \true;
    const C = \false;
    private static array $a = [\true, \PHP_INT_MAX, \E_ALL];
    public static function g() { return [self::$e, self::$n, self::C, self::$a]; }
    public function h($x = \false, int $y = \PHP_INT_SIZE) { return [$x, $y]; }
}
var_dump(T::g(), (new T)->p, (new T)->h());
function k($z = \true) { static $s = \false; return [$z, $s]; }
var_dump(k());

var_dump(constant("false"), constant("\\true"), constant("NULL"), defined("false"), defined("\\TRUE"), defined("nul"));

?>
--EXPECT--
array(4) {
  [0]=>
  bool(false)
  [1]=>
  NULL
  [2]=>
  bool(false)
  [3]=>
  array(3) {
    [0]=>
    bool(true)
    [1]=>
    int(9223372036854775807)
    [2]=>
    int(30719)
  }
}
bool(true)
array(2) {
  [0]=>
  bool(false)
  [1]=>
  int(8)
}
array(2) {
  [0]=>
  bool(true)
  [1]=>
  bool(false)
}
bool(false)
bool(true)
NULL
bool(true)
bool(true)
bool(false)
