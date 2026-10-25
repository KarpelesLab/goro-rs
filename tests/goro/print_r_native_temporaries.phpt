--TEST--
print_r() memo is not fooled by a reused address of a freed temporary
--FILE--
<?php $s = simplexml_load_string("<r><e/><e/><q a=\"1\"/></r>"); print_r($s); var_dump($s); $s = simplexml_load_string("<r><e/><q a=\"1\"/></r>"); print_r($s);
 $s = simplexml_load_string("<r><a><b/><c/></a><d><e/></d><f/></r>"); print_r($s); $o = [new stdClass, new stdClass, new stdClass]; var_dump(array_map("spl_object_id", $o));
--EXPECT--
SimpleXMLElement Object
(
    [e] => Array
        (
            [0] => SimpleXMLElement Object
                (
                )

            [1] => SimpleXMLElement Object
                (
                )

        )

    [q] => SimpleXMLElement Object
        (
            [@attributes] => Array
                (
                    [a] => 1
                )

        )

)
object(SimpleXMLElement)#1 (2) {
  ["e"]=>
  array(2) {
    [0]=>
    object(SimpleXMLElement)#4 (0) {
    }
    [1]=>
    object(SimpleXMLElement)#3 (0) {
    }
  }
  ["q"]=>
  object(SimpleXMLElement)#2 (1) {
    ["@attributes"]=>
    array(1) {
      ["a"]=>
      string(1) "1"
    }
  }
}
SimpleXMLElement Object
(
    [e] => SimpleXMLElement Object
        (
        )

    [q] => SimpleXMLElement Object
        (
            [@attributes] => Array
                (
                    [a] => 1
                )

        )

)
SimpleXMLElement Object
(
    [a] => SimpleXMLElement Object
        (
            [b] => SimpleXMLElement Object
                (
                )

            [c] => SimpleXMLElement Object
                (
                )

        )

    [d] => SimpleXMLElement Object
        (
            [e] => SimpleXMLElement Object
                (
                )

        )

    [f] => SimpleXMLElement Object
        (
        )

)
array(3) {
  [0]=>
  int(4)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
