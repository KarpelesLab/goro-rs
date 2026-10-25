--TEST--
XPath number literals and number() accept an exponent; array callback results warn
--FILE--
<?php
$d = new DOMDocument; $d->loadXML('<r/>');
$x = new DOMXPath($d);
foreach (['number("1e3")', 'number("1E-2")', 'number("1e")', 'number(" 2e+2 ")', 'number("e3")', 'number(".5e1")', '1e3', '.5e1', '1E+2', '2e-3', 'number("1e400")', 'number("-1e2")', 'string(1e21)', 'string(1e-7)', 'string(123.456e5)', '5e-1 * 2', 'number("1.e2")', 'number(".")', 'number("+1")'] as $q) {
    echo $q, " => "; var_dump($x->evaluate($q));
}
$x->registerNamespace('php', 'http://php.net/xpath');
$x->registerPhpFunctions(['h' => fn() => [1, 2]]);
var_dump($x->evaluate('php:function("h")'));
--EXPECTF--
number("1e3") => float(1000)
number("1E-2") => float(0.01)
number("1e") => float(1)
number(" 2e+2 ") => float(200)
number("e3") => float(NAN)
number(".5e1") => float(5)
1e3 => float(1000)
.5e1 => float(5)
1E+2 => float(100)
2e-3 => float(0.002)
number("1e400") => float(INF)
number("-1e2") => float(-100)
string(1e21) => string(5) "1e+21"
string(1e-7) => string(5) "1e-07"
string(123.456e5) => string(8) "12345600"
5e-1 * 2 => float(1)
number("1.e2") => float(100)
number(".") => float(NAN)
number("+1") => float(NAN)

Warning: Array to string conversion in %s on line 9
string(5) "Array"
