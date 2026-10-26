--TEST--
Loose comparison of strings: numeric-looking ones compare as numbers, others as bytes
--FILE--
<?php
function eq($a, $b) { return $a == $b; }
$cases = [
    ['abc', 'abc'], ['abc', 'abd'], ['', ''], ['', '0'], ['0', ''], ['1e3', '1000'], ['1000', '1e3'],
    [' 1', '1'], ["\t1", '1'], ["\n1", "1"], ["\v1", '1'], ["\f1", '1'], ['1 ', '1'], ['+1', '1'], ['-0', '0'],
    ['.5', '0.5'], ['0.50', '.5'], ['abc', '0'], ['0', 'abc'], ['1abc', '1'], ['10', '1e1'], ['INF', 'INF'],
    ['NAN', 'NAN'], ['1e1000', '1e1000'], ['1e1000', '2e1000'], ['9223372036854775807', '9223372036854775808'],
    ['9223372036854775808', '9223372036854775809'], ['0x1A', '26'], ['00', '0'], ['a', 'A'], ['é', 'é'],
];
foreach ($cases as [$a, $b]) {
    echo json_encode($a), ' == ', json_encode($b), ': ', var_export(eq($a, $b), true), ' ', var_export($a != $b, true), "\n";
}
?>
--EXPECT--
"abc" == "abc": true false
"abc" == "abd": false true
"" == "": true false
"" == "0": false true
"0" == "": false true
"1e3" == "1000": true false
"1000" == "1e3": true false
" 1" == "1": true false
"\t1" == "1": true false
"\n1" == "1": true false
"\u000b1" == "1": true false
"\f1" == "1": true false
"1 " == "1": true false
"+1" == "1": true false
"-0" == "0": true false
".5" == "0.5": true false
"0.50" == ".5": true false
"abc" == "0": false true
"0" == "abc": false true
"1abc" == "1": false true
"10" == "1e1": true false
"INF" == "INF": true false
"NAN" == "NAN": true false
"1e1000" == "1e1000": true false
"1e1000" == "2e1000": false true
"9223372036854775807" == "9223372036854775808": false true
"9223372036854775808" == "9223372036854775809": false true
"0x1A" == "26": false true
"00" == "0": true false
"a" == "A": false true
"\u00e9" == "\u00e9": true false
