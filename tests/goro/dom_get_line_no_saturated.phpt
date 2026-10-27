--TEST--
DOMNode::getLineNo() of nodes past line 65535 without LIBXML_BIGLINES
--FILE--
<?php
$big = '<r>' . str_repeat("\n", 70000) . '<a/><b>x</b><!--c--></r>';
foreach ([0, LIBXML_BIGLINES] as $opt) {
    $d = new DOMDocument; $d->loadXML($big, $opt);
    foreach (['a', 'b'] as $t) var_dump($d->getElementsByTagName($t)->item(0)->getLineNo());
    var_dump($d->documentElement->lastChild->getLineNo(), $d->documentElement->getLineNo());
}
?>
--EXPECT--
int(65535)
int(65535)
int(65535)
int(1)
int(70001)
int(70001)
int(70001)
int(1)
