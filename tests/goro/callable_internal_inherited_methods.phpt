--TEST--
Callables naming methods an internal class inherits from an internal parent (DOMDocument::loadXML)
--FILE--
<?php
$d = new DOMDocument;
var_dump(is_callable('DOMDocument::loadXML'), is_callable([$d, 'loadXML']), is_callable(['DOMDocument', 'loadXML']), is_callable([$d, 'nope']));
var_dump(is_callable('DOMXPath::quote'), is_callable('XMLReader::open'), is_callable('XMLReader::read'), is_callable([new XMLReader, 'read']));
var_dump(is_callable('SimpleXMLElement::asXML'), is_callable([simplexml_load_string('<a/>'), 'asXML']));
try { array_map('DOMDocument::loadXML', ['<a/>']); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { array_map([$d, 'nope'], [1]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(array_map('DOMXPath::quote', ['a']));
var_dump(call_user_func([$d, 'createElement'], 'x')->nodeName);
var_dump(call_user_func('XMLReader::XML', '<a/>') instanceof XMLReader);
$f = [$d, 'createTextNode']; var_dump($f('t')->data);
$c = Closure::fromCallable([$d, 'createComment']); var_dump($c('c')->data);
$c2 = $d->createElement(...); var_dump($c2('y')->nodeName);
?>
--EXPECT--
bool(false)
bool(true)
bool(false)
bool(false)
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, non-static method DOMDocument::loadXML() cannot be called statically
TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, class DOMDocument does not have a method "nope"
array(1) {
  [0]=>
  string(3) "'a'"
}
string(1) "x"
bool(true)
string(1) "t"
string(1) "c"
string(1) "y"
