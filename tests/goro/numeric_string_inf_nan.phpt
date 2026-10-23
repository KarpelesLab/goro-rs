--TEST--
"INF"/"NAN" strings are not numeric (comparison, decrement, is_numeric)
--FILE--
<?php
// "INF", "NAN" and friends are not numeric strings
foreach (["INF", "-INF", "NAN", "inf", "Infinity", " 1", "1 ", "1e3", ".5", "0x1A", "1e"] as $s) {
    echo str_pad(var_export($s, true), 12), is_numeric($s) ? "numeric" : "non-numeric", "\n";
}
var_dump("1" == "NAN", "1" < "NAN", "1" <=> "NAN", "NAN" <=> "5.", "INF" == "1e1000", 1.0 <=> "NAN");
$x = "INF"; $x--; var_dump($x);
$y = "NAN"; $y--; var_dump($y);
var_dump(max("INF", "9"), "INF" > 1);
--EXPECTF--
'INF'       non-numeric
'-INF'      non-numeric
'NAN'       non-numeric
'inf'       non-numeric
'Infinity'  non-numeric
' 1'        numeric
'1 '        numeric
'1e3'       numeric
'.5'        numeric
'0x1A'      non-numeric
'1e'        non-numeric
bool(false)
bool(true)
int(-1)
int(1)
bool(false)
int(-1)

Deprecated: Decrement on non-numeric string has no effect and is deprecated in %s on line 7
string(3) "INF"

Deprecated: Decrement on non-numeric string has no effect and is deprecated in %s on line 8
string(3) "NAN"
string(3) "INF"
bool(true)
