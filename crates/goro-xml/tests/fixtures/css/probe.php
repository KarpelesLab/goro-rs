<?php
$dom = Dom\HTMLDocument::createFromString('<!doctype html><html><body><div id="Main" class="A b"><p lang="en-US" title="x">t</p><svg><foreignObject/><circle viewBox="1"/></svg><input type="checkbox" checked><select><option selected>a</option><option>b</option></select><a href="x">l</a><a>nl</a></div></body></html>');
$d = $dom->createElementNS('http://www.w3.org/1999/xhtml', 'Div');
$dom->body->appendChild($d);
var_dump($d->localName);
$cases = ['div', 'DIV', 'Div', '*', 'p[LANG]', 'p[lang="en-us"]', 'p[title="X"]', 'p[lang|=en]', 'circle', 'CIRCLE', 'foreignObject', 'foreignobject', '[viewBox]', '[viewbox]',
 '#Main', '#main', '.A', '.a', 'div.b', ':checked', 'option:checked', ':link', 'a:not([href])', 'p:first-child', ':nth-child(2n of p)', 'p:only-child', ':root', 'html:root',
 'div > p', 'div>p', 'div  p', 'p ~ svg', 'p + svg', ':is(p, svg)', ':where(p)', ':has(> p)', 'div:has(p, a)', ':not(p, div)', '*|div', 'svg|circle', '|p', ':nth-child( 2n + 1 )', ':nth-child(odd)', ':nth-child(-n+3)', ':nth-child(+5)',
 '', ' ', ',', 'div,', ',div', 'div[', 'div[a', 'div[a=]', 'div[a="b" x]', 'div[a="b" I]', 'div >', '> div', 'div >> p', ':unknown', '::unknown', ':nth-child(foo)', ':nth-child()', ':not()', ':is()', ':has()', '#1', '#', '.', '.1', 'a b c', 'div:hover', ':focus', 'p::before', '::before', ':first-line', 'a:not(:hover)', 'div ! p', 'div{', 'p:nth-of-type(1)', ':empty', 'p::', ':', '[', ']', '(', 'a)', '"a"', 'p:lang(en)', ':host', ':defined', ':scope > p', '[a~=]', '[|a]', '[*|a]', 'a\\:b', '\\31 a', ':NTH-CHILD(1)', ':FIRST-CHILD', 'DIV:not(P)'];
foreach ($cases as $c) {
    try {
        $r = $dom->querySelectorAll($c);
        $names = [];
        foreach ($r as $n) $names[] = $n->localName;
        echo str_pad(json_encode($c), 28), " => ", count($r), " ", implode(",", $names), "\n";
    } catch (Throwable $e) {
        echo str_pad(json_encode($c), 28), " => ", get_class($e), " ", $e->getCode(), " ", $e->getMessage(), "\n";
    }
}
$x = Dom\XMLDocument::createFromString('<r><DIV/><div/><p lang="EN"/></r>');
foreach (['div', 'DIV', 'p[LANG]', 'p[lang="en"]', 'p[lang="en" s]'] as $c) {
    echo "XML ", str_pad($c, 20), " => ", count($x->querySelectorAll($c)), "\n";
}
