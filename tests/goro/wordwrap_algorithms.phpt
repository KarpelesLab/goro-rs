--TEST--
wordwrap(): php-src's single-character and multi-character/cut algorithms
--FILE--
<?php
// wordwrap(): php-src's two algorithms
$cases = [
    ['Hello World', 1, 'Hello World', true], ['Hello World', 1, 'Hello World', false], ['abc', 1, 'abc', true],
    ['The quick brown fox', 10, "\n", false], ['The quick brown fox', 10, '<br>', false], ['A very long woooooooooooord.', 8, "\n", true],
    ['A very long woooooooooooord. and something', 8, "\n", false], ["line one\nline two is longer", 10, "\n", false],
    ["x\n", 1, "\n", true], ["ab\ncd", 1, "\n", true], ['aaa bbb', 3, 'XX', true], ['aaa bbbXXccc', 3, 'XX', false],
    ['a b c d e', 0, "\n", false], ['a b c d e', -1, "|", false], ['short', 10, "\n", true], ['  leading', 2, '-', false],
];
foreach ($cases as [$s, $w, $b, $c]) echo json_encode(wordwrap($s, $w, $b, $c)), "\n";
--EXPECT--
"HHello WorldeHello WorldlHello WorldlHello WorldoHello WorldWHello WorldoHello WorldrHello WorldlHello Worldd"
"HelloHello WorldWorld"
"aabcbabcc"
"The quick\nbrown fox"
"The quick<br>brown fox"
"A very\nlong\nwooooooo\nooooord."
"A very\nlong\nwoooooooooooord.\nand\nsomething"
"line one\nline two\nis longer"
"x\n\n"
"a\nb\nc\nd"
"aaaXXbbb"
"aaaXXbbbXXccc"
"a\nb\nc\nd\ne"
"a|b|c|d|e"
"short"
" -leading"
