--TEST--
parent:: calls of DOM / XMLReader constructors and methods forward $this; static calls of them fail
--FILE--
<?php
class E2 extends DOMElement {
  function __construct() { parent::__construct('e2', 'val'); var_dump($this->tagName); }
  function setAttribute($n, $v): DOMAttr|bool { echo "override\n"; return parent::setAttribute($n, strtoupper($v)); }
}
$x = new E2();
$d = new DOMDocument; $d->appendChild($x); $x->setAttribute('a', 'b'); echo $d->saveXML();
class D extends DOMDocument {
  function loadXML($s, $o = 0): bool { echo "myload\n"; return parent::loadXML("<w>$s</w>", $o); }
  function saveXML(?DOMNode $n = null, $o = 0): string|false { return "SAVED:" . parent::saveXML($n, $o); }
}
$dd = new D; $dd->loadXML('in'); echo $dd->saveXML();
class X extends DOMXPath { function __construct(DOMDocument $d) { parent::__construct($d); } function q($e) { return parent::query($e)->length; } }
var_dump((new X($dd))->q('//w'));
class T extends DOMText { function __construct() { parent::__construct('tt'); } function appendData($s): true { return parent::appendData("[$s]"); } }
$t = new T; $t->appendData('z'); var_dump($t->data);
class A extends DOMAttr { function __construct() { parent::__construct('nm', 'vv'); } }
$a = new A; var_dump($a->name, $a->value);
class P extends DOMProcessingInstruction { function __construct() { parent::__construct('tgt', 'dd'); } }
var_dump((new P)->target);
class R extends XMLReader { function read(): bool { $r = parent::read(); echo "read ", $this->name, "\n"; return $r; } }
$r = new R; $r->XML('<a><b/></a>'); while ($r->read());
try { DOMDocument::loadXML('<a/>'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { DOMElement::__construct('x'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { DOMElement::appendChild(new DOMText('x')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(DOMXPath::quote("a'b"));
?>
--EXPECT--
string(2) "e2"
override
<?xml version="1.0"?>
<e2 a="B">val</e2>
myload
SAVED:<?xml version="1.0"?>
<w>in</w>
int(1)
string(5) "tt[z]"
string(2) "nm"
string(2) "vv"
string(3) "tgt"
read a
read b
read a
read 
Error: Non-static method DOMDocument::loadXML() cannot be called statically
Error: Non-static method DOMElement::__construct() cannot be called statically
Error: Non-static method DOMNode::appendChild() cannot be called statically
string(5) ""a'b""
