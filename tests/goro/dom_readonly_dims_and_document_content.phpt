--TEST--
DOM: node lists refuse dimension writes; textContent/nodeValue writes on a document do nothing
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML('<r><a>t</a></r>');
foreach (['[]', '++', '&', '.='] as $k) {
  try {
    if ($k === '++') $d->nodeType++;
    elseif ($k === '&') $r = &$d->nodeType;
    elseif ($k === '.=') $d->nodeName .= 'x';
    else $d->childNodes[] = 1;
  } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
$l = $d->childNodes;
try { $l[] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $l[0] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { unset($l[0]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $d->documentElement->attributes[] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($l[0]->nodeName, isset($l[0]), isset($l[5]));
$d->textContent = "new";
$d->nodeValue = "zz";
echo $d->saveXML();
$d->documentElement->textContent = "<b>&</b>";
echo $d->saveXML();
$d->documentElement->nodeValue = "nv";
echo $d->saveXML();
$x = Dom\XMLDocument::createFromString('<r><a>t</a></r>');
$x->textContent = "q";
try { $x->childNodes[] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo $x->saveXml(), "\n";
--EXPECTF--
Error: Cannot use object of type DOMNodeList as array
Error: Cannot modify readonly property DOMDocument::$nodeType
Error: Cannot modify readonly property DOMDocument::$nodeName
Error: Cannot use object of type DOMNodeList as array
Error: Cannot use object of type DOMNodeList as array
Error: Cannot use object of type DOMNodeList as array
Error: Cannot use object of type DOMNamedNodeMap as array
string(1) "r"
bool(true)
bool(false)
<?xml version="1.0"?>
<r><a>t</a></r>
<?xml version="1.0"?>
<r>&lt;b&gt;&amp;&lt;/b&gt;</r>
<?xml version="1.0"?>
<r>nv</r>

Fatal error: Uncaught Error: Cannot modify readonly property Dom\XMLDocument::$textContent in %s:26
Stack trace:
#0 {main}
  thrown in %s on line 26
