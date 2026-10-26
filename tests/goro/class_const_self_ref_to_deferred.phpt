--TEST--
A class constant expression using self::X whose value is evaluated later (it uses another class's constants)
--FILE--
<?php
enum En { case X; }
final class R {
    public const A = '[a-z]';
    public const B = '(?:=' . self::A . ')';
}
final class H {
    private const S = '\s*(' . R::A . R::B . ')\s*';
    private const L = '/{(' . self::S . ')+}/';
    const ALIAS = self::S;
    const ARR = [R::A, 'k' => R::B];
    const ARR2 = self::ARR;
    const ARR3 = [...self::ARR, 'z'];
    const DYN = self::{'S'} . '!';
    const E = self::ENUMS[0];
    const ENUMS = [En::X];
    public static function get() { return [self::L, self::S, self::ALIAS, self::ARR2, self::ARR3, self::DYN, self::E]; }
}
var_dump(H::get());
var_dump((new ReflectionClassConstant('H', 'L'))->getValue());

?>
--EXPECT--
array(7) {
  [0]=>
  string(30) "/{(\s*([a-z](?:=[a-z]))\s*)+}/"
  [1]=>
  string(23) "\s*([a-z](?:=[a-z]))\s*"
  [2]=>
  string(23) "\s*([a-z](?:=[a-z]))\s*"
  [3]=>
  array(2) {
    [0]=>
    string(5) "[a-z]"
    ["k"]=>
    string(10) "(?:=[a-z])"
  }
  [4]=>
  array(3) {
    [0]=>
    string(5) "[a-z]"
    ["k"]=>
    string(10) "(?:=[a-z])"
    [1]=>
    string(1) "z"
  }
  [5]=>
  string(24) "\s*([a-z](?:=[a-z]))\s*!"
  [6]=>
  enum(En::X)
}
string(30) "/{(\s*([a-z](?:=[a-z]))\s*)+}/"
