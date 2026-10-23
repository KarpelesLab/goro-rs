--TEST--
Numeric string comparison (zendi_smart_str*: ASCII whitespace only, overflow fallbacks), <=> and max()/min() with objects
--FILE--
<?php
$cases = [["\u{a0}1", "1"], ["1\u{a0}", "1"], ["\u{2003}1", "1"], [" 1", "1"], ["1 ", "1"], ["\n1\t", "1"],
  ["1e1000", "1e1001"], ["-1e1000", "-1e1001"], ["1e1000", "-1e1000"], ["99999999999999999999", "100000000000000000000"],
  ["9223372036854775808", "9223372036854775809"], ["9223372036854775807", "9223372036854775808"],
  ["-9223372036854775808", "-9223372036854775809"], ["9223372036854775808", "9.2233720368547758E+18"],
  ["1e3", "1000"], ["0x1A", "26"], ["abc", "abd"], ["10", "9"], ["10", "9a"], ["1.0", "1"], [".5", "0.5"],
  ["00001", "1"], ["1e400", "INF"], ["123456789012345678901.5", "123456789012345678901.5"], ["", "0"], ["0", "-0"],
  ["99999999999999999999", "99999999999999999999.0"], ["1", "1.0000000000000000001"]];
foreach ($cases as [$a, $b]) {
    echo json_encode([$a, $b]), ' ', var_export($a == $b, true), ' ', $a <=> $b, ' ', $b <=> $a, ' ', var_export($a < $b, true), ' ', var_export(in_array($a, [$b]), true), "\n";
}
switch ("1e1000") { case "1e1001": echo "switch eq\n"; break; default: echo "switch ne\n"; }
class S { function __toString(): string { return "10"; } }
echo json_encode([new S <=> "abc", "abc" <=> new S, new S <=> "10", new S <=> "9", "9" <=> new S]), "\n";
var_dump(new stdClass <=> 3);
var_dump(3 <=> new stdClass);
var_dump(max(new S, "abc"), get_class(min(new S, "abc")), max([new S, "abc"]), get_class(min([new S, "abc"])));
$x = ["10", "9", "1e1", "abc", "9a"]; sort($x); echo json_encode($x), "\n";
--EXPECTF--
["\u00a01","1"] false 1 -1 false false
["1\u00a0","1"] false 1 -1 false false
["\u20031","1"] false 1 -1 false false
[" 1","1"] true 0 0 false true
["1 ","1"] true 0 0 false true
["\n1\t","1"] true 0 0 false true
["1e1000","1e1001"] false -1 1 true false
["-1e1000","-1e1001"] false -1 1 true false
["1e1000","-1e1000"] false 1 -1 false false
["99999999999999999999","100000000000000000000"] false 1 -1 false false
["9223372036854775808","9223372036854775809"] false -1 1 true false
["9223372036854775807","9223372036854775808"] false -1 1 true false
["-9223372036854775808","-9223372036854775809"] false 1 -1 false false
["9223372036854775808","9.2233720368547758E+18"] true 0 0 false true
["1e3","1000"] true 0 0 false true
["0x1A","26"] false -1 1 true false
["abc","abd"] false -1 1 true false
["10","9"] false 1 -1 false false
["10","9a"] false -1 1 true false
["1.0","1"] true 0 0 false true
[".5","0.5"] true 0 0 false true
["00001","1"] true 0 0 false true
["1e400","INF"] false -1 1 true false
["123456789012345678901.5","123456789012345678901.5"] true 0 0 false true
["","0"] false -1 1 true false
["0","-0"] true 0 0 false true
["99999999999999999999","99999999999999999999.0"] false -1 1 true false
["1","1.0000000000000000001"] true 0 0 false true
switch ne
[-1,1,0,1,-1]

Notice: Object of class stdClass could not be converted to int in %s on line 15
int(-1)

Notice: Object of class stdClass could not be converted to int in %s on line 16
int(1)
string(3) "abc"
string(1) "S"
string(3) "abc"
string(1) "S"
["9","10","1e1","9a","abc"]
