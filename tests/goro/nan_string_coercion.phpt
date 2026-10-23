--TEST--
NAN coerced to string warns in builtins too (print_r, strval, implode, string parameters)
--FILE--
<?php
$n = NAN; $f = fdiv(0, 0);
echo "1:"; print_r($n); echo "\n";
echo "2:", strval($n), "\n";
echo "3:", implode(",", [$n, 1.5]), "\n";
echo "4:", str_repeat($n, 1), "\n";
echo "5:", $n . "", "\n";
echo "6:", strlen($n), "\n";
$y = $n; settype($y, "string"); echo "7:", $y, "\n";
echo "8:", sprintf("%s|%d|%f", $n, 1, $n), "\n";
echo "9:", number_format($n), "\n";
echo "10:", var_export($n, true), "\n";
echo "11:", json_encode([$n], JSON_PARTIAL_OUTPUT_ON_ERROR), "\n";
echo "12:", serialize($n), "\n";
echo "13:", print_r([$n], true), "\n";
echo "14:", "x$n", "\n";
echo "15:", str_pad($n, 5, '*'), "\n";
echo "16:", in_array("NAN", [$n]) ? 'y' : 'n', "\n";
echo "17:", (string) $f, "\n";
echo "18:", ucfirst($n), "\n";
echo "20:", htmlspecialchars($n), "\n";
--EXPECTF--
1:
Warning: unexpected NAN value was coerced to string in %s on line 3
NAN
2:
Warning: unexpected NAN value was coerced to string in %s on line 4
NAN
3:
Warning: unexpected NAN value was coerced to string in %s on line 5
NAN,1.5
4:
Warning: unexpected NAN value was coerced to string in %s on line 6
NAN
5:
Warning: unexpected NAN value was coerced to string in %s on line 7
NAN
6:
Warning: unexpected NAN value was coerced to string in %s on line 8
3

Warning: unexpected NAN value was coerced to string in %s on line 9
7:NAN
8:
Warning: unexpected NAN value was coerced to string in %s on line 10
NAN|1|NaN
9:nan
10:NAN
11:[0]
12:d:NAN;
13:
Warning: unexpected NAN value was coerced to string in %s on line 15
Array
(
    [0] => NAN
)

14:
Warning: unexpected NAN value was coerced to string in %s on line 16
xNAN
15:
Warning: unexpected NAN value was coerced to string in %s on line 17
NAN**
16:n
17:
Warning: unexpected NAN value was coerced to string in %s on line 19
NAN
18:
Warning: unexpected NAN value was coerced to string in %s on line 20
NAN
20:
Warning: unexpected NAN value was coerced to string in %s on line 21
NAN
