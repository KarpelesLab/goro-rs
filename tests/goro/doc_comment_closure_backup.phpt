--TEST--
A closure's doc comment is the latest one before its parameter list (backup_doc_comment)
--FILE--
<?php
/***/ function a() {}
/** */ function b() {}
/**x*/ function c() {}
/**
*/ function d() {}
/** doc for e */
$x = 1;
function e() {}
/** doc for f */
echo "";
function f() {}
/** before class */
$y = 2;
class K {}
class L {
  /** const doc */
  public function __construct() {}
  /** dangling */
}
/** for closure */
$z = 3;
$cl = function() {};
var_dump((new ReflectionFunction('a'))->getDocComment(), (new ReflectionFunction('b'))->getDocComment(), (new ReflectionFunction('c'))->getDocComment(), (new ReflectionFunction('d'))->getDocComment(), (new ReflectionFunction('e'))->getDocComment(), (new ReflectionFunction('f'))->getDocComment(), (new ReflectionClass('K'))->getDocComment(), (new ReflectionFunction($cl))->getDocComment());
class M {
  /** p1 */
  public
  /** p2 */
  static
  /** p3 */
  int $p = 1;
  /** m1 */ final /** m2 */ public function m() {}
  /** c1 */ final /** c2 */ const C = 1;
}
var_dump((new ReflectionProperty('M', 'p'))->getDocComment(), (new ReflectionMethod('M', 'm'))->getDocComment(), (new ReflectionClassConstant('M', 'C'))->getDocComment());
/** a1 */ abstract /** a2 */ class N {}
var_dump((new ReflectionClass('N'))->getDocComment());
/** fn1 */ $af = /** fn2 */ static /** fn3 */ fn() => 1;
var_dump((new ReflectionFunction($af))->getDocComment());
function params(/** pd */ int $a, #[X] /** pd2 */ $b) {}
var_dump((new ReflectionFunction('params'))->getDocComment());
/** fn1 */ $af2 = /** fn2 */ static /** fn3 */ fn() => 1;
var_dump((new ReflectionFunction($af2))->getDocComment());
function params2() {}
var_dump((new ReflectionFunction("params2"))->getDocComment());
?>
--EXPECT--
bool(false)
string(6) "/** */"
bool(false)
string(6) "/**
*/"
string(16) "/** doc for e */"
string(16) "/** doc for f */"
string(19) "/** before class */"
string(18) "/** for closure */"
string(9) "/** p3 */"
string(9) "/** m2 */"
string(9) "/** c2 */"
string(9) "/** a2 */"
string(10) "/** fn3 */"
bool(false)
string(10) "/** fn3 */"
bool(false)
