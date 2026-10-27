--TEST--
zend_dtoa() small-integer path: an exact tie kept at an even digit keeps its trailing zeros (echo, printf %G, json_encode, var_export, serialize)
--FILE--
<?php
echo 100000000000005.0, " ", 100000000000015.0, " ", 100000000000025.0, " ", 120000000000005.0, " ", -100000000000005.0, " ", 999999999999995.0, " ", 100000000000004.0, "\n";
var_dump(100000000000005.0, 1e14 + 50);
ini_set('precision', 3);
echo 1005.0, " ", 1015.0, " ", 1025.0, " ", 10050.0, " ", 100500.0, " ", 2005.0, " ", 1000.5, " ", 0.1005, " ", 1e15 + 5e12, "\n";
ini_set('precision', 1);
echo 5.0, " ", 25.0, " ", 15.0, " ", 450.0, " ", 0.5, "\n";
ini_set('precision', 14);
printf("%.3G %.3g %.2e\n", 1005.0, 1005.0, 1005.0);
echo number_format(1005.0, -1), " ", round(1005.0, -1), "\n";
ini_set('serialize_precision', 3);
echo json_encode(1005.0), " ", var_export(1005.0, true), " ", serialize(10050.0), "\n";
?>
--EXPECT--
1.0000000000000E+14 1.0000000000002E+14 1.0000000000002E+14 1.2000000000000E+14 -1.0000000000000E+14 1.0E+15 1.0E+14
float(100000000000005)
float(100000000000050)
1.00E+3 1.02E+3 1.02E+3 1.00E+4 1.00E+5 2.00E+3 1.0E+3 0.101 1.0E+15
5 2.0E+1 2.0E+1 4.0E+2 0.5
1.00E+3 1.00e+3 1.00e+3
1,010 1010
1.00e+3 1.00E+3 d:1.00E+4;
