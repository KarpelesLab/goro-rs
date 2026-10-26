--TEST--
Large flat documents stay linear: xml_parse_into_struct, XPath sorting, node list iteration
--FILE--
<?php
// Large flat documents: these used to take quadratic time
$n = 30000;
$big = '<r>' . str_repeat('<i a="1">text &amp; more</i>', $n) . '</r>';
$t = microtime(true);
$p = xml_parser_create();
xml_parse_into_struct($p, $big, $vals, $index);
var_dump(count($vals), count($index['I']), $vals[2]);
$d = new DOMDocument;
$d->loadXML($big);
$xp = new DOMXPath($d);
var_dump($xp->evaluate('count(//i[@a=1])'), $xp->evaluate('count(/r/i/following-sibling::i[1])'), $xp->evaluate('string(/r/i[2]/following-sibling::i[2]/@a)'));
$c = 0;
foreach ($xp->query('//i') as $e) $c++;
$l = $d->getElementsByTagName('i');
foreach ($l as $e) $c++;
for ($k = 0; $k < $l->length; $k++) if ($l->item($k)) $c++;
var_dump($c);
var_dump(microtime(true) - $t < 20);
?>
--EXPECT--
int(30002)
int(30000)
array(5) {
  ["tag"]=>
  string(1) "I"
  ["type"]=>
  string(8) "complete"
  ["level"]=>
  int(2)
  ["attributes"]=>
  array(1) {
    ["A"]=>
    string(1) "1"
  }
  ["value"]=>
  string(11) "text & more"
}
float(30000)
float(29999)
string(1) "1"
int(90000)
bool(true)
