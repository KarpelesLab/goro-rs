--TEST--
sprintf(): php_formatted_print() padding, INF/NAN, -0.0, %h/%H and argument errors
--FILE--
<?php
// php_formatted_print(): padding, signs, INF/NAN, -0.0, %h/%H, errors
$cases = [
    ['%-10.2f|', INF], ['%10.2f|', -INF], ['%-10.2f|', NAN], ['%5.1f', NAN],
    ['%+f', INF], ['%e', -0.0], ['%.2f', -0.0], ['%.2f', -0.001], ['%g', -0.0],
    ['%h', 123456789012], ['%H', 1.5e-7], ['%.3h', 0.000012345], ['%h', 255],
    ['%.*H', -1, 0.1], ['%G', 1e-10], ['%.3g', 9.999999], ['%10g', 1e15],
    ["%'010d", -42], ['%-05d|', -42], ['%+05d', 42], ['%05.1f', -2.5],
    ['%u', -1], ['%x', -1], ['%5.3s|', 'abcdef'], ['%-5c|', 65],
    ['%1$s %1$\'#6s', 'ab'], ['%e', 0], ['%.0e', 12345], ['%E', 123456789012],
];
foreach ($cases as $c) {
    $f = array_shift($c);
    echo str_pad($f, 12), ' => "', sprintf($f, ...$c), "\"\n";
}
foreach (['%z', '%', '%5.2%', '%2$s', "%'", '%0$s', '%.-1f'] as $f) {
    try {
        var_dump(sprintf($f, 1));
    } catch (\Throwable $e) {
        echo $f, ' => ', get_class($e), ': ', $e->getMessage(), "\n";
    }
}
try { vsprintf('%d %d', [1]); } catch (\ValueError $e) { echo $e->getMessage(), "\n"; }
try { sprintf('%d %d', 1); } catch (\ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
try { fprintf(STDOUT, '%d %d', 1); } catch (\ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
try { sprintf('%'); } catch (\ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
%-10.2f|     => "INF|"
%10.2f|      => "-INF|"
%-10.2f|     => "NaN|"
%5.1f        => "NaN"
%+f          => "INF"
%e           => "0.000000e+0"
%.2f         => "0.00"
%.2f         => "-0.00"
%g           => "-0"
%h           => "1.23457e+11"
%H           => "1.5E-7"
%.3h         => "1.23e-5"
%h           => "255"
%.*H         => "0.1"
%G           => "1.0E-10"
%.3g         => "10"
%10g         => "   1.0e+15"
%'010d       => "-000000042"
%-05d|       => "-42  |"
%+05d        => "+0042"
%05.1f       => "-02.5"
%u           => "18446744073709551615"
%x           => "ffffffffffffffff"
%5.3s|       => "  abc|"
%-5c|        => "A|"
%1$s %1$'#6s => "ab ####ab"
%e           => "0.000000e+0"
%.0e         => "1e+4"
%E           => "1.234568E+11"
%z => ValueError: Unknown format specifier "z"
% => ValueError: Missing format specifier at end of string
string(1) "%"
%2$s => ArgumentCountError: 3 arguments are required, 2 given
%' => ValueError: Missing padding character
%0$s => ValueError: Argument number specifier must be greater than zero and less than 2147483647
%.-1f => ValueError: Unknown format specifier "-"
The arguments array must contain 2 items, 1 given
3 arguments are required, 2 given
4 arguments are required, 3 given
2 arguments are required, 1 given
