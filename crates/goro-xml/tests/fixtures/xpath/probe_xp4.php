<?php
$d = new DOMDocument();
$d->loadXML('<!DOCTYPE r [<!ENTITY e "x">]><?pi x?><!--c--><r xmlns="urn:d" xmlns:p="urn:p" p:a="1" b="2"><p:c>t<![CDATA[u]]>v</p:c></r>');
$x = new DOMXPath($d);
foreach (['count(/node())', 'count(//@*)', 'count(//namespace::*)', 'count(//text())', 'string(//text()[2])', 'name(/node()[1])', 'name(//namespace::*[1])',
  'local-name(//namespace::*[1])', 'string(//namespace::*[1])', 'count(//*[local-name()="r"]/namespace::*)', 'name(//*[1]/namespace::*[2])', 'name(//*[1]/namespace::*[3])',
  'count(//*[local-name()="c"]/namespace::*)', 'name(//@*[1])', 'namespace-uri(//@*[1])', 'count(//*/attribute::*)', 'local-name(/)', 'name(/)', 'string(/)', 'count(/r)', 'count(/*)'] as $e) {
  $r = $x->evaluate($e); echo str_pad($e, 50), " => ", var_export($r, true), "\n";
}
echo "--- modern XML ---\n";
$d = Dom\XMLDocument::createFromString('<r xmlns="urn:d" xmlns:p="urn:p" p:a="1" b="2"><p:c/></r>');
$x = new Dom\XPath($d);
foreach (['count(//@*)', 'count(//namespace::*)', 'name(//@*[1])', 'name(/*)', 'count(//*[name()="r"])', 'count(//*[local-name()="r"])'] as $e) {
  $r = $x->evaluate($e); echo str_pad($e, 50), " => ", var_export($r, true), "\n";
}
echo "--- modern HTML ---\n";
$d = Dom\HTMLDocument::createFromString('<!DOCTYPE html><p class=a>x</p><svg xmlns:xlink="http://www.w3.org/1999/xlink" width=1><use xlink:href="#a"/></svg>');
$x = new Dom\XPath($d);
foreach (['count(//@*)', 'count(//namespace::*)', 'name(//*[local-name()="svg"]/@*[1])', 'count(//p)', 'count(//*[name()="p"])', 'name(//*[local-name()="p"])', 'count(/node())',
  'name(/node()[1])', 'count(//*[local-name()="use"]/namespace::*)', 'name(//*[local-name()="use"]/namespace::*[1])', 'name(//*[local-name()="use"]/namespace::*[2])', 'name(//*[local-name()="use"]/namespace::*[3])'] as $e) {
  try { $r = $x->evaluate($e); echo str_pad($e, 50), " => ", var_export($r, true), "\n"; } catch (Throwable $t) { echo "$e => EXC ", $t->getMessage(), "\n"; }
}
echo "--- legacy HTML ---\n";
$d = new DOMDocument(); $d->loadHTML('<p class=a>x</p>');
$x = new DOMXPath($d);
foreach (['count(//p)', 'name(/node()[1])', 'count(/node())', 'count(//namespace::*)'] as $e) {
  $r = $x->evaluate($e); echo str_pad($e, 50), " => ", var_export($r, true), "\n";
}
