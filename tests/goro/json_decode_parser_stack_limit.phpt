--TEST--
json_decode()/json_validate(): nesting deeper than the parser stack (YYMAXDEPTH) is a syntax error
--FILE--
<?php
function t($s, $assoc = true) {
    $r = json_decode($s, $assoc, 100000);
    echo $r === null ? 'NULL' : 'ok', ' ', json_last_error_msg(), ' ', json_validate($s, 100000) ? 'valid' : 'invalid', "\n";
}
foreach ([4997, 4998, 4999, 5000] as $n) t(str_repeat('[', $n) . str_repeat(']', $n));
foreach ([2498, 2499, 2500] as $n) t(str_repeat('[1,', $n) . '1' . str_repeat(']', $n));
foreach ([2498, 2499, 2500] as $n) t(str_repeat('[1,', $n) . '[]' . str_repeat(']', $n));
foreach ([2498, 2499, 2500] as $n) t(str_repeat('{"a":', $n) . '1' . str_repeat('}', $n));
foreach ([2498, 2499, 2500] as $n) t(str_repeat('{"a":', $n) . '{}' . str_repeat('}', $n), false);
foreach ([1665, 1666, 1667] as $n) t(str_repeat('{"b":1,"a":', $n) . '1' . str_repeat('}', $n));
foreach ([3332, 3333, 3334] as $n) t(str_repeat('[[1],', $n) . '1' . str_repeat(']', $n));
t(str_repeat('[', 6000) . '"\xff"');
t(str_repeat('[', 6000));
var_dump(json_decode(str_repeat('[', 5000) . str_repeat(']', 5000), true), json_last_error_msg());
--EXPECT--
ok No error valid
ok No error valid
NULL Syntax error invalid
NULL Syntax error invalid
ok No error valid
ok No error valid
NULL Syntax error invalid
ok No error valid
NULL Syntax error invalid
NULL Syntax error invalid
ok No error valid
ok No error valid
NULL Syntax error invalid
ok No error valid
NULL Syntax error invalid
NULL Syntax error invalid
ok No error valid
ok No error valid
NULL Syntax error invalid
NULL Syntax error invalid
NULL Syntax error invalid
NULL Syntax error invalid
NULL Syntax error invalid
NULL Syntax error invalid
NULL
string(28) "Maximum stack depth exceeded"
