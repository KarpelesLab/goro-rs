--TEST--
php:function() with an invalid callback name throws Invalid callback
--FILE--
<?php
$d = new DOMDocument; $d->loadXML('<r><a>x</a></r>');
$x = new DOMXPath($d);
$x->registerNamespace('php', 'http://php.net/xpath');
$x->registerPhpFunctions();
var_dump($x->evaluate('php:function("strtoupper", "a")'));
foreach (['nonexist', 'Nope::m', 'DOMDocument::nope', 'DOMDocument::loadXML'] as $f) {
    try { var_dump($x->evaluate("php:function('$f')")); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
?>
--EXPECT--
string(1) "A"
Error: Invalid callback nonexist, function "nonexist" not found or invalid function name
Error: Invalid callback Nope::m, class "Nope" not found
Error: Invalid callback DOMDocument::nope, class DOMDocument does not have a method "nope"
Error: Invalid callback DOMDocument::loadXML, non-static method DOMDocument::loadXML() cannot be called statically
