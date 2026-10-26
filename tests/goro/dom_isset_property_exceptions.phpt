--TEST--
isset()/empty()/?? on DOM properties: exceptions thrown by the read handler propagate
--FILE--
<?php
$d = Dom\XMLDocument::createFromString('<r><b/></r>');
$b = $d->documentElement->firstChild;
$b->setAttribute('xmlns:t', '');
try { var_dump($b->outerHTML); } catch (Throwable $e) { echo "1 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($b->outerHTML ?? 'dflt'); } catch (Throwable $e) { echo "2 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(isset($b->outerHTML)); } catch (Throwable $e) { echo "3 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(empty($b->outerHTML)); } catch (Throwable $e) { echo "4 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($d->documentElement->innerHTML ?? 'dflt'); } catch (Throwable $e) { echo "5 ", get_class($e), ": ", $e->getMessage(), "\n"; }
$old = new DOMDocument; $old->loadXML('<r/>');
try { var_dump($old->documentElement->nodeValue ?? 'dflt', isset($old->documentElement->nodeValue), isset($old->documentElement->textContent)); } catch (Throwable $e) { echo "6 ", get_class($e), ": ", $e->getMessage(), "\n"; }
$x = new XMLReader;
try { var_dump($x->name ?? 'dflt'); } catch (Throwable $e) { echo "7 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(isset($x->name)); } catch (Throwable $e) { echo "8 ", get_class($e), ": ", $e->getMessage(), "\n"; }
class X extends DOMElement { function __construct() {} }
$x = new X;
try { var_dump(isset($x->nodeValue)); } catch (Throwable $e) { echo "1 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($x->nodeValue ?? 'dflt'); } catch (Throwable $e) { echo "2 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(empty($x->tagName)); } catch (Throwable $e) { echo "3 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(property_exists($x, 'nodeValue')); } catch (Throwable $e) { echo "4 ", get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($x->nodeValue); } catch (Throwable $e) { echo "5 ", get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new DOMAttr('a');
var_dump(isset($a->ownerElement), isset($a->value), isset($a->nope));
?>
--EXPECT--
1 DOMException: The resulting XML serialization is not well-formed
2 DOMException: The resulting XML serialization is not well-formed
3 DOMException: The resulting XML serialization is not well-formed
4 DOMException: The resulting XML serialization is not well-formed
5 DOMException: The resulting XML serialization is not well-formed
string(0) ""
bool(true)
bool(true)
string(0) ""
bool(true)
1 DOMException: Invalid State Error
2 DOMException: Invalid State Error
3 DOMException: Invalid State Error
bool(true)
5 DOMException: Invalid State Error
bool(false)
bool(true)
bool(false)
