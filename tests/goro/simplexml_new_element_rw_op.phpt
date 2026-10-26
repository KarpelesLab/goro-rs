--TEST--
SimpleXML: $sxe->new op= v / $sxe->new++ create an empty element and write nothing back
--FILE--
<?php
$s = simplexml_load_string('<r a="1"><a>1</a><c>x</c></r>');
$s->a++;
$s->d++;
$s->c .= 'y';
$s->e .= 'z';
$s->f += 2;
$x = ($s->g .= 'w');
var_dump($x);
$s->h->i = 'deep';
$s->j[] = 'app';
$s['a'] .= '2';
$s['b'] .= '3';
$s->k[0] .= 'k';
var_dump(new stdClass);
echo $s->asXML();
--EXPECT--
string(1) "w"
object(stdClass)#3 (0) {
}
<?xml version="1.0"?>
<r a="12" b="3"><a>2</a><c>xy</c><d/><e/><f/><g/><h><i>deep</i></h><j>app</j><k>k</k></r>
