--TEST--
$o->{NAME} / $o->{__CLASS__}() use the constant's value as the member name
--FILE--
<?php
const M = "zz";
class A {
    const K = 'kk';
    public $kk = 'prop kk';
    function __call($m, $a) { return "call $m"; }
    static function __callStatic($m, $a) { return "cs $m"; }
    function t() {
        var_dump($this->{__CLASS__}(), $this->{__METHOD__}(), $this->{PHP_EOL}());
        var_dump(static::{__CLASS__}(), A::{M}(), $this->{M}(), $this->{\M}(), $this->{self::K}(), $this->{self::K});
        var_dump($this->{__CLASS__}, $this->{M} ?? 'unset');
    }
}
(new A)->t();
var_dump((new A)->{M}(), A::{M}(), (new A)->{A::K});

?>
--EXPECTF--
string(6) "call A"
string(9) "call A::t"
string(6) "call 
"
string(6) "call A"
string(7) "call zz"
string(7) "call zz"
string(7) "call zz"
string(7) "call kk"
string(7) "prop kk"

Warning: Undefined property: A::$A in %s on line %d
NULL
string(5) "unset"
string(7) "call zz"
string(5) "cs zz"
string(7) "prop kk"
