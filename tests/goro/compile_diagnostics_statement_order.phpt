--TEST--
Compile diagnostics of one line come in statement order (hoisted declarations included)
--FILE--
<?php
declare(foo=1); function f($a=1,$b){}
function g($a=1,$b){} declare(bar=1);
class C { function m($x=1, $y) {} } declare(baz=1);
declare(qux=1); class D { function m($x=1, $y) {} }
echo "ok\n";
--EXPECTF--

Warning: Unsupported declare 'foo' in %s on line 2

Deprecated: f(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 2

Deprecated: g(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter in %s on line 3

Warning: Unsupported declare 'bar' in %s on line 3

Deprecated: C::m(): Optional parameter $x declared before required parameter $y is implicitly treated as a required parameter in %s on line 4

Warning: Unsupported declare 'baz' in %s on line 4

Warning: Unsupported declare 'qux' in %s on line 5

Deprecated: D::m(): Optional parameter $x declared before required parameter $y is implicitly treated as a required parameter in %s on line 5
ok
