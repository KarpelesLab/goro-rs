--TEST--
Doc comments: /**/ is none; the latest before a declaration wins and survives unrelated statements
--FILE--
<?php
#[Attribute] class Foo {}
/**/
function a() {}
class Q {
    public /** d1 */ $q;
    public /** d2 */ function n() {}
    #[Foo] /** d3 */ const C = 1;
    #[Foo] /** d4 */ public $p;
    #[Foo] /** d5 */ public function m() {}
    /** x1 */ public /** x2 */ static $s;
}
/** a1 */ abstract /** a2 */ class N {}
/** d */ $x = 1;
function e() {}
/** dc */ $y = 2;
class Z {}
/** dcl */ $y2 = 3;
$cl = function() {};
$o = new /** dn */ class {};
/** do */ $o2 = new class {};
var_dump((new ReflectionFunction('a'))->getDocComment());
var_dump((new ReflectionProperty('Q', 'q'))->getDocComment());
var_dump((new ReflectionMethod('Q', 'n'))->getDocComment());
var_dump((new ReflectionClassConstant('Q', 'C'))->getDocComment());
var_dump((new ReflectionProperty('Q', 'p'))->getDocComment());
var_dump((new ReflectionMethod('Q', 'm'))->getDocComment());
var_dump((new ReflectionProperty('Q', 's'))->getDocComment());
var_dump((new ReflectionClass('N'))->getDocComment());
var_dump((new ReflectionFunction('e'))->getDocComment());
var_dump((new ReflectionClass('Z'))->getDocComment());
var_dump((new ReflectionFunction($cl))->getDocComment());
var_dump((new ReflectionObject($o))->getDocComment());
var_dump((new ReflectionObject($o2))->getDocComment());
--EXPECT--
bool(false)
string(9) "/** d1 */"
string(9) "/** d2 */"
string(9) "/** d3 */"
string(9) "/** d4 */"
string(9) "/** d5 */"
string(9) "/** x2 */"
string(9) "/** a2 */"
string(8) "/** d */"
string(9) "/** dc */"
string(10) "/** dcl */"
string(9) "/** dn */"
string(9) "/** do */"
