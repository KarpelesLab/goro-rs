--TEST--
DOMNodeList / DOMNamedNodeMap dimension offsets: objects warn, numeric-prefix strings are indexes
--FILE--
<?php
$d = new DOMDocument; $d->loadXML('<r a="1" b="2"><a/><b/></r>');
$l = $d->documentElement->childNodes; $m = $d->documentElement->attributes;
foreach ([new stdClass, [], [1], 1.9, "1.5", " 1", "1 ", "0x1", true, null, "-1", "9999999999999999999"] as $k) {
    echo json_encode($k), ": ";
    try { $v = $l[$k]; echo $v ? $v->nodeName : 'null'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(); }
    echo " | ";
    try { echo json_encode(isset($l[$k])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(); }
    echo " | ";
    try { $v = $m[$k]; echo $v ? $v->nodeName : 'null'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(); }
    echo " | ";
    try { echo json_encode(isset($m[$k])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(); }
    echo "\n";
}
$md = Dom\XMLDocument::createFromString('<r a="1"><a/><b/></r>');
?>
--EXPECTF--
{}: 
Warning: Object of class stdClass could not be converted to int in %s on line 6
b | 
Warning: Object of class stdClass could not be converted to int in %s on line 8
true | 
Warning: Object of class stdClass could not be converted to int in %s on line 10
b | 
Warning: Object of class stdClass could not be converted to int in %s on line 12
true
[]: a | true | a | true
[1]: b | true | b | true
1.9: b | true | b | true
"1.5": b | true | b | true
" 1": b | true | b | true
"1 ": b | true | b | true
"0x1": a | true | a | true
true: b | true | b | true
null: a | true | a | true
"-1": null | false | ValueError: must be between 0 and 2147483647 | false
"9999999999999999999": null | false | ValueError: must be between 0 and 2147483647 | false
