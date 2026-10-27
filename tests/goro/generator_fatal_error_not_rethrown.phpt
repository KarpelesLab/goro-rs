--TEST--
A fatal error inside a generator iterated by foreach is not turned into an Error exception
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
function f() {
    $l = new D('local');
    $g = (function() { $gl = new D('gen-local'); yield 1; trigger_error("in generator", E_USER_ERROR); })();
    foreach ($g as $v) { echo "got $v\n"; }
}
f();
--EXPECTF--
got 1

Deprecated: Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead in %s on line 5

Fatal error: in generator in %s on line 5
