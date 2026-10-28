<?php
$doc = new DOMDocument();
$doc->loadXML('<root xmlns:a="urn:a"><x n="1">1</x><x n="2">2.5</x><y>abc</y><a:z/></root>');
$xp = new DOMXPath($doc);
$exprs = [
 '1 div 3', '2 div 3', '1 div 0', '-1 div 0', '0 div 0', '-0', '0 * -1', '1 - 1',
 'string(1 div 3)', 'string(2 div 3)', 'string(1 div 0)', 'string(-1 div 0)', 'string(0 div 0)', 'string(0 * -1)',
 'string(0.1 + 0.2)', 'string(1e21)', 'string(123456789012345678901234567890)', 'string(1000000)', 'string(100000000000000000000)',
 'string(1000000000000000000000)', 'string(12345678901234567890)', 'string(0.000001)', 'string(0.0000001)', 'string(1.5)', 'string(-1.5)',
 'string(3.14159265358979)', 'string(1 div 7)', 'string(2.5 * 4)', 'string(100 div 3)', 'string(1e-10 * 3)',
 'string(0.1)', 'string(123.456)', 'string(-0.5)', 'string(9007199254740993)', 'string(4.35)', 'string(1.0)', 'string(5e-324 * 1)',
 '0.1 + 0.2', '1e21', '123456789012345678901234567890', 'number("  12  ")', 'number("1e3")', 'number("abc")', 'number("-")', 'number(".5")', 'number("5.")', 'number("+5")',
 'round(2.5)', 'round(-2.5)', 'round(-0.4)', 'string(round(-0.4))', 'floor(-1.5)', 'ceiling(-0.5)', 'string(ceiling(-0.5))', 'round(1 div 0)',
 'substring("12345", 1.5, 2.6)', 'substring("12345", 0, 3)', 'substring("12345", 0 div 0, 3)', 'substring("12345", 1, 0 div 0)',
 'substring("12345", -42, 1 div 0)', 'substring("12345", -1 div 0, 1 div 0)', 'substring-before("1999/04/01","/")', 'substring-after("1999/04/01","19")',
 'translate("bar","abc","ABC")', 'translate("--aaa--","abc-","ABC")', 'normalize-space("  a  b   c ")', 'concat("a", 1, true())', 'string(true())',
 'contains("abc", "")', 'starts-with("abc", "ab")', 'string-length("héllo")', 'count(//x)', 'sum(//x)', 'sum(//y)', 'string(//x)', 'string(//x/@n)',
 'boolean("")', 'boolean("0")', 'boolean(0)', 'boolean(//nothing)', 'not(1)', 'true() = 1', '"1" = 1', '//x = 1', '//x = "2.5"', '//x != 1', '//x < //x',
 '//x > 2', 'local-name(//a:z)', 'name(//a:z)', 'namespace-uri(//a:z)', 'local-name()', 'name()', 'lang("en")', 'id("foo")',
 '1 = 1 = 1', '- - 1', '3 mod 2', '-3 mod 2', '3 mod -2', '5.5 mod 2', '1 mod 0', '2 + 3 * 4', '(2 + 3) * 4', '10 div 4 div 5',
 '//x[1]', '//x[last()]', '//x[position() = 2]/@n', 'count(//x[@n > 1])', 'count(//*)', 'count(//node())', 'count(//text())', 'count(/)',
 'count(//@*)', 'count(//namespace::*)', 'count(/descendant-or-self::node())', 'count(//x/following-sibling::*)', 'count(//y/preceding::*)',
 'count(//x/ancestor::*)', 'count(//x/..)', 'count(//x/self::x)', 'string(//x[2]/preceding-sibling::x)', 'name(//*[2])', 'string(number(//y))',
];
foreach ($exprs as $e) {
    $r = @$xp->evaluate($e);
    echo str_pad($e, 45), " => ";
    if ($r instanceof DOMNodeList) echo "nodelist(", $r->length, ")";
    else var_export($r);
    echo "\n";
}
echo "---- errors ----\n";
libxml_use_internal_errors(true);
$bad = ['//[', '1+', '-', 'foo(', 'foo()', 'unknown:foo()', 'x:y', '//x:y', '//@', '1 1', '"abc', "'abc", '//x[', '//x]', '()', '$var', 'count()', 'count(1,2)', 'string(1,2)', 'substring("a")',
  '/..', '//x/following::', 'child::', 'foo::x', '@@x', 'processing-instruction(1)', 'node(1)', '..[1]', '. .', '1 + + 1', 'concat("a")', 'php:function("x")', '&', '!x', '1 div', 'mod 1', '//x | 1', '1 | //x', 'count(1)', 'sum("a")', 'name(1)', 'boolean()', '//x[name()=]'];
foreach ($bad as $e) {
    libxml_clear_errors();
    try {
        $r = $xp->evaluate($e);
        echo str_pad($e, 25), " => ";
        var_export($r instanceof DOMNodeList ? "nodelist(" . $r->length . ")" : $r);
    } catch (Throwable $t) {
        echo str_pad($e, 25), " => EXC ", get_class($t), ": ", $t->getMessage();
    }
    echo "\n";
    foreach (libxml_get_errors() as $er) {
        echo "    ERR level={$er->level} code={$er->code} col={$er->column} line={$er->line} msg=", json_encode($er->message), "\n";
    }
}
libxml_use_internal_errors(false);
echo "---- warnings displayed ----\n";
$xp->evaluate('//[');
$xp->query('1+');
$xp->evaluate('foo()');
$xp->evaluate('x:y');
$xp->query('count(1)');
$r = $xp->query('1+1');
var_dump($r);
echo "---- modern ----\n";
$d2 = Dom\XMLDocument::createFromString('<root xmlns:a="urn:a"><a:z/></root>');
$x2 = new Dom\XPath($d2);
try { var_dump($x2->evaluate('//[')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->evaluate('x:y')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->query('count(1)')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->evaluate('count(//a:z)')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->evaluate('count(//a:z)', null, false)); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->evaluate('//namespace::*')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
try { var_dump($x2->evaluate('count(//namespace::*)')); } catch (Throwable $t) { echo get_class($t), ": ", $t->getMessage(), "\n"; }
