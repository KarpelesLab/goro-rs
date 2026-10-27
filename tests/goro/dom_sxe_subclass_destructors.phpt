--TEST--
Destructors of user subclasses of DOM, DOMXPath and SimpleXMLElement run when their objects are released
--FILE--
<?php
class E extends DOMElement { function __destruct() { echo "destruct {$this->nodeName}\n"; } }
class E2 extends DOMElement { function __construct() { parent::__construct('e2'); } function __destruct() { echo "destruct {$this->nodeName}\n"; } }
class D extends DOMDocument { function __destruct() { echo "doc destruct\n"; } }
class XP extends DOMXPath { function __destruct() { echo "xpath destruct\n"; } }
class SX extends SimpleXMLElement { function __destruct() { echo "sx destruct\n"; } }
$x = new E('direct'); unset($x); echo "1\n";
$x = new E2(); unset($x); echo "2\n";
$x = new D(); $x->loadXML('<a/>'); unset($x); echo "3\n";
new E('tmp'); echo "4\n";
for ($i = 0; $i < 2; $i++) { $x = new E("loop$i"); } unset($x); echo "5\n";
$x = new XP(new DOMDocument); unset($x); echo "6\n";
$d = new DOMDocument; $d->registerNodeClass('DOMElement', 'E'); $d->loadXML('<r><a/><b/></r>');
$e = $d->documentElement; unset($e); echo "7\n";
$d->documentElement->firstChild; echo "8\n";
$s = simplexml_load_string('<a><b/></a>', 'SX'); $b = $s->b; unset($b); echo "9\n"; unset($s); echo "10\n";
?>
--EXPECT--
destruct direct
1
destruct e2
2
doc destruct
3
destruct tmp
4
destruct loop0
destruct loop1
5
xpath destruct
6
destruct r
7
destruct r
destruct a
8
sx destruct
9
sx destruct
10
