--TEST--
property_exists() of SimpleXMLElement children and attributes asks has_property
--EXTENSIONS--
simplexml
--FILE--
<?php
$xml = new SimpleXMLElement('<root><message></message><m2>a</m2><m2>b</m2></root>');
var_dump(property_exists('SimpleXMLElement', 'message'), property_exists($xml, 'message'), property_exists($xml, 'missing'), property_exists($xml, 'm2'));
var_dump(count($xml->message), count($xml->m2));
$a = (new SimpleXMLElement('<root message="Hello"/>'))->attributes();
var_dump(property_exists($a, 'message'), property_exists($a, 'nope'), count($a->message));
$s = new class('<root><message>Hello</message></root>') extends SimpleXMLElement {};
var_dump(property_exists($s, 'message'), property_exists(get_class($s), 'message'), count($s->message));
var_dump(isset($xml->message), isset($a->message), isset($xml->missing));

?>
--EXPECT--
bool(false)
bool(true)
bool(false)
bool(true)
int(1)
int(2)
bool(true)
bool(false)
int(0)
bool(true)
bool(false)
int(1)
bool(true)
bool(true)
bool(false)
