--TEST--
SimpleXML non-debug property table keeps its child objects
--FILE--
<?php
// json_encode()/(array) keep the child objects in sxe->properties
$x = simplexml_load_string('<root><item><sub>s</sub></item><e/></root>');
echo json_encode($x), " ", spl_object_id(new stdClass), "\n";
var_dump((array)$x);
var_dump(new stdClass);
$y = simplexml_load_string('<root><item><sub>s</sub></item></root>');
var_dump(get_object_vars($y));
var_dump($y->children());
--EXPECT--
{"item":{"sub":"s"},"e":{}} 4
array(2) {
  ["item"]=>
  object(SimpleXMLElement)#3 (1) {
    ["sub"]=>
    string(1) "s"
  }
  ["e"]=>
  object(SimpleXMLElement)#2 (0) {
  }
}
object(stdClass)#4 (0) {
}
array(1) {
  ["item"]=>
  object(SimpleXMLElement)#5 (1) {
    ["sub"]=>
    string(1) "s"
  }
}
object(SimpleXMLElement)#6 (1) {
  ["item"]=>
  object(SimpleXMLElement)#7 (1) {
    ["sub"]=>
    string(1) "s"
  }
}
