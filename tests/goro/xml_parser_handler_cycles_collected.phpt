--TEST--
XMLParser cycles through xml_set_object() / method-callable handlers are collected (xml_parser_get_gc)
--FILE--
<?php
class D { public $r; public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
class P { public $p; public $n = 'parser'; function __construct() { $this->p = xml_parser_create(); xml_set_object($this->p, $this); xml_set_element_handler($this->p, 's', 'e'); } function s() {} function e() {} function __destruct() { echo "dtor parser\n"; } }
$p = new P; xml_parse($p->p, '<a/>', true); unset($p);
var_dump(gc_collect_cycles());
class P2 { public $p; function __construct() { $this->p = xml_parser_create(); xml_set_element_handler($this->p, [$this, 's'], [$this, 'e']); } function s() {} function e() {} function __destruct() { echo "dtor parser2\n"; } }
$p = new P2; unset($p);
var_dump(gc_collect_cycles());
echo "end\n";
?>
--EXPECTF--

Deprecated: Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions in %s on line 3

Deprecated: xml_set_element_handler(): Passing non-callable strings is deprecated since 8.4 in %s on line 3
dtor parser
int(2)
dtor parser2
int(2)
end
