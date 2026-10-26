--TEST--
NumberFormatter::parse()/parseCurrency(): lenient parsing ignores white space and does not require the positive affixes
--EXTENSIONS--
intl
--FILE--
<?php
// ICU parses leniently by default: [:Zs:] and tab are ignorable (a plain
// space matches the no-break space of "1.234,56 €"), and the positive
// prefix / suffix are optional (only a strict parse requires them); the
// negative ones still decide the sign, and parseCurrency() needs a currency
$de = new NumberFormatter("de_DE", NumberFormatter::CURRENCY);
foreach (["1.234,56\u{a0}€", "1.234,56 €", "1.234,56 USD", "1.234,56", "-1.234,56 €", "€"] as $s) {
    $p = 0;
    $c = null;
    echo json_encode($s, JSON_UNESCAPED_UNICODE), " parse=", var_export($de->parse($s, NumberFormatter::TYPE_DOUBLE, $p), true), "@$p parseCurrency=", var_export($de->parseCurrency($s, $c), true), " ", var_export($c, true), "\n";
}
$en = new NumberFormatter("en_US", NumberFormatter::CURRENCY);
foreach (["$1,234.56", "1,234.56", "-$5.00", "\t$ 7"] as $s) {
    $p = 0;
    echo json_encode($s), " parse=", var_export($en->parse($s, NumberFormatter::TYPE_DOUBLE, $p), true), "@$p\n";
}
$d = new NumberFormatter("en_US", NumberFormatter::DECIMAL);
foreach (["12 abc", "  -5", "+3"] as $s) {
    $p = 0;
    echo json_encode($s), " parse=", var_export($d->parse($s, NumberFormatter::TYPE_DOUBLE, $p), true), "@$p\n";
}
$d->setAttribute(NumberFormatter::LENIENT_PARSE, 0);
var_dump($d->parse("  -5"));
?>
--EXPECT--
"1.234,56 €" parse=1234.56@10 parseCurrency=1234.56 'EUR'
"1.234,56 €" parse=1234.56@10 parseCurrency=1234.56 'EUR'
"1.234,56 USD" parse=1234.56@12 parseCurrency=1234.56 'USD'
"1.234,56" parse=1234.56@8 parseCurrency=false NULL
"-1.234,56 €" parse=-1234.56@11 parseCurrency=-1234.56 'EUR'
"€" parse=false@0 parseCurrency=false NULL
"$1,234.56" parse=1234.56@9
"1,234.56" parse=1234.56@8
"-$5.00" parse=-5.0@6
"\t$ 7" parse=7.0@4
"12 abc" parse=12.0@2
"  -5" parse=-5.0@4
"+3" parse=3.0@2
bool(false)
