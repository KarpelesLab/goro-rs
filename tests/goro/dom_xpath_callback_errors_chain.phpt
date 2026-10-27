--TEST--
DOMXPath/Dom\XPath php:function() failures chain (zend_throw_error()) and evaluation goes on with an empty string; a thrown or skipped registered callback aborts it
--FILE--
<?php
function show($f) { try { var_dump($f()); } catch (Throwable $e) { do { echo get_class($e), ": ", $e->getMessage(), "\n"; } while ($e = $e->getPrevious()); } }
function boom($s) { echo "boom($s)\n"; throw new RuntimeException("boom $s"); }
function up($s) { echo "up($s)\n"; return strtoupper($s); }
function obj() { echo "obj\n"; return new stdClass; }
$d = new DOMDocument; $d->loadXML('<r><a>x</a><a>y</a></r>');
$x = new DOMXPath($d);
$x->registerNamespace('php', 'http://php.net/xpath');
show(fn() => $x->evaluate('php:function("up", "a")'));
$x->registerPhpFunctions();
show(fn() => $x->evaluate('php:function("boom", "a")'));
show(fn() => $x->evaluate('concat(php:function("boom", "a"), php:function("up", "b"))'));
show(fn() => $x->evaluate('concat(php:function("boom", "a"), php:function("nonexist", "b"), php:function("boom", "c"))'));
show(fn() => $x->evaluate('concat(php:function("obj"), php:function("obj"))'));
show(fn() => $x->evaluate('concat(php:function("nonexist"), php:function("Nope::m"))'));
show(fn() => $x->evaluate('concat(php:function(), php:function())'));
show(fn() => $x->query('//a[php:function("boom", string(.))]'));
show(fn() => $x->query('//a[php:functionString("boom", .)]'));
show(fn() => $x->evaluate('php:functionString("up", //a)'));
show(fn() => $x->evaluate('concat(php:functionString("boom", //a), php:functionString("boom", //a))'));
$x = new DOMXPath($d);
$x->registerNamespace('php', 'http://php.net/xpath');
$x->registerPhpFunctions(['up', 'boom']);
show(fn() => $x->evaluate('concat(php:function("up", "a"), php:function("lo", "b"), php:function("up", "c"))'));
show(fn() => $x->evaluate('concat(php:function("boom", "a"), php:function("lo", "b"), php:function("up", "c"))'));
show(fn() => $x->evaluate('concat(php:function("lo", "a"), php:function("boom", "b"), php:function("obj"))'));
show(fn() => $x->registerPhpFunctions(['up', 'nope']));
show(fn() => $x->evaluate('php:function("up", "a")'));
show(fn() => $x->registerPhpFunctions('nope'));
show(fn() => $x->evaluate('php:function("up", "a")'));
$x = new DOMXPath($d);
$x->registerNamespace('php', 'http://php.net/xpath');
show(fn() => $x->registerPhpFunctions(['nope']));
show(fn() => $x->evaluate('php:function("up", "a")'));
$x = new DOMXPath($d);
$x->registerNamespace('f', 'urn:f');
$x->registerPhpFunctionNS('urn:f', 'b', 'boom');
$x->registerPhpFunctionNS('urn:f', 'u', 'up');
show(fn() => $x->evaluate('concat(f:b("a"), f:u("b"), f:b("c"))'));
$x->registerPhpFunctionNS('urn:f', 'o', 'obj');
show(fn() => $x->evaluate('concat(f:o(), f:o(), f:u("x"))'));
$d2 = Dom\XMLDocument::createFromString('<r><a>x</a></r>');
$x = new Dom\XPath($d2);
$x->registerNamespace('php', 'http://php.net/xpath');
$x->registerPhpFunctions();
show(fn() => $x->evaluate('concat(php:function("boom", "a"), php:function("nonexist", "b"))'));
show(fn() => $x->query('//a[php:function("boom", "a")]'));
show(fn() => $x->evaluate('php:function("nonexist") = php:function("up", "z")'));
$x = new DOMXPath($d);
$x->registerNamespace('php', 'http://php.net/xpath');
$x->registerPhpFunctions(['up', 'boom', 'obj']);
show(fn() => $x->evaluate('concat(php:function("lo", "a"), php:function("lo2", "b"))'));
show(fn() => $x->evaluate('concat(php:function("lo", "a"), "-", php:function("lo2", "b"))'));
show(fn() => $x->evaluate('concat(php:function("obj"), php:function("lo2", "b"))'));
show(fn() => $x->evaluate('concat(php:function(), php:function("lo2", "b"))'));
show(fn() => $x->evaluate('concat(php:function(), php:function())'));
show(fn() => $x->evaluate('concat(php:function(1), php:function())'));
show(fn() => $x->evaluate('concat(php:function("lo"), php:function())'));
show(fn() => $x->evaluate('php:function("lo") = php:function("lo2")'));
show(fn() => $x->evaluate('php:function("lo") or php:function("lo2")'));
show(fn() => $x->evaluate('php:function("lo") | php:function("lo2")'));
show(fn() => $x->evaluate('string(php:function("lo")) = string(php:function("lo2"))'));
show(fn() => $x->evaluate('count(//a[php:function("lo")])'));
show(fn() => $x->evaluate('count(//a[php:function()])'));
show(fn() => $x->evaluate('php:function(php:function("lo"))'));
show(fn() => $x->evaluate('php:function("up", php:function("lo"))'));
show(fn() => $x->evaluate('php:function("up", php:function())'));
--EXPECTF--
Error: No callbacks were registered
boom(a)
RuntimeException: boom a
boom(a)
RuntimeException: boom a
boom(a)
RuntimeException: boom a
obj
TypeError: Only objects that are instances of DOM nodes can be converted to an XPath expression
Error: Invalid callback nonexist, function "nonexist" not found or invalid function name
Error: Function name must be passed as the first argument
Error: Function name must be passed as the first argument
boom(x)
RuntimeException: boom x
boom(x)
RuntimeException: boom x
up(x)
string(1) "X"
boom(x)
RuntimeException: boom x
up(a)
Error: No callback handler "lo" registered
boom(a)
RuntimeException: boom a
Error: No callback handler "lo" registered
TypeError: DOMXPath::registerPhpFunctions(): Argument #1 ($restrict) must be an array with valid callbacks as values, function "nope" not found or invalid function name
up(a)
string(1) "A"
TypeError: DOMXPath::registerPhpFunctions(): Argument #1 ($restrict) must be a callable, function "nope" not found or invalid function name
up(a)
string(1) "A"
TypeError: DOMXPath::registerPhpFunctions(): Argument #1 ($restrict) must be an array with valid callbacks as values, function "nope" not found or invalid function name
Error: No callback handler "up" registered
boom(a)
RuntimeException: boom a
obj
TypeError: Only objects that are instances of DOM nodes can be converted to an XPath expression
boom(a)
RuntimeException: boom a
boom(a)
RuntimeException: boom a
Error: Invalid callback nonexist, function "nonexist" not found or invalid function name
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
obj
Error: No callback handler "lo2" registered
TypeError: Only objects that are instances of DOM nodes can be converted to an XPath expression
Error: No callback handler "lo2" registered
Error: Function name must be passed as the first argument
Error: Function name must be passed as the first argument
Error: Function name must be passed as the first argument
Error: Function name must be passed as the first argument
TypeError: Handler name must be a string
Error: Function name must be passed as the first argument
Error: No callback handler "lo" registered
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo2" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo" registered
Error: Function name must be passed as the first argument
Error: Function name must be passed as the first argument
Error: No callback handler "" registered
Error: No callback handler "lo" registered
Error: No callback handler "lo" registered
Error: Function name must be passed as the first argument
