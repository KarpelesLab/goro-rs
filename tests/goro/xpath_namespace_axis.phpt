--TEST--
XPath namespace axis: xmlns="" included, axis order kept
--FILE--
<?php
$d = new DOMDocument;
$d->loadXML('<r xmlns:a="urn:a" xmlns:b="urn:b" xmlns="urn:d"><s xmlns:c="urn:c" xmlns:a="urn:a2"><t xmlns=""/></s></r>');
$x = new DOMXPath($d);
foreach (['//namespace::*', '//t/namespace::*', '//t/namespace::*[1]', '//t/namespace::*[last()]', 'count(//t/namespace::*)', 'name(//t/namespace::*[2])'] as $q) {
    $r = $x->evaluate($q);
    if (!is_object($r)) { var_dump($r); continue; }
    foreach ($r as $ns) echo $ns->nodeName, '=', $ns->nodeValue, '@', $ns->parentNode->nodeName, ' ';
    echo "\n";
}
// namespace nodes made by an earlier query keep the axis order
$d = new DOMDocument;
$d->loadXML('<h><g xmlns:c="urn:b"><f/></g></h>');
$x = new DOMXPath($d);
foreach ($x->query('//namespace::*') as $ns) echo $ns->nodeName, '@', $ns->parentNode->nodeName, ' ';
echo "\n";
$d->documentElement->setAttributeNS('urn:z', 'z:q', '1');
foreach ($x->query('//namespace::*') as $ns) echo $ns->nodeName, '@', $ns->parentNode->nodeName, ' ';
echo "\n";
--EXPECT--
xmlns:xml=http://www.w3.org/XML/1998/namespace@r xmlns=urn:d@r xmlns:b=urn:b@r xmlns:a=urn:a@r xmlns:xml=http://www.w3.org/XML/1998/namespace@s xmlns=urn:d@s xmlns:b=urn:b@s xmlns:a=urn:a2@s xmlns:c=urn:c@s xmlns:xml=http://www.w3.org/XML/1998/namespace@t xmlns:b=urn:b@t xmlns:a=urn:a2@t xmlns:c=urn:c@t xmlns=@t 
xmlns:xml=http://www.w3.org/XML/1998/namespace@t xmlns:b=urn:b@t xmlns:a=urn:a2@t xmlns:c=urn:c@t xmlns=@t 
xmlns:xml=http://www.w3.org/XML/1998/namespace@t 
xmlns=@t 
float(5)
string(1) "b"
xmlns:xml@h xmlns:xml@g xmlns:c@g xmlns:xml@f xmlns:c@f 
xmlns:xml@h xmlns:z@h xmlns:xml@g xmlns:z@g xmlns:c@g xmlns:xml@f xmlns:z@f xmlns:c@f 
