--TEST--
Diagnostics of a builtin after it ran user code (__toString, callbacks) report the builtin's line
--FILE--
<?php
class S { function __toString(): string {
    return "abc"; } }
echo implode(',', [new S, []]), "\n";
echo str_pad(new S, 5, '-'), "\n";
try { str_pad(new S, 5, ''); } catch (ValueError $e) { echo $e->getLine(), "\n"; }
try { mb_strpos(new S, 'b', 10); } catch (ValueError $e) { echo $e->getLine(), "\n"; }
function cb($m) {
    return 'x';
}
echo preg_replace_callback(['/a/', '/['], 'cb', 'abc'), "\n";
$a = [3, 1, 2];
usort($a, function ($x, $y) {
    return $x <=> $y;
});
echo $a[0] + "5 apples", "\n";
array_map(function ($v) {
    return $v;
}, [1]);
echo [] . "", "\n";
function f($v, &$k) {}
$arr = [1, 2, 3];
array_walk($arr, 'f');
echo strlen(new S) . [], "\n";
--EXPECTF--

Warning: Array to string conversion in %s on line 4
abc,Array
abc--
6
7

Warning: preg_replace_callback(): No ending delimiter '/' found in %s on line 11


Warning: A non-numeric value encountered in %s on line 16
6

Warning: Array to string conversion in %s on line 20
Array

Warning: f(): Argument #2 ($k) must be passed by reference, value given in %s on line 23

Warning: f(): Argument #2 ($k) must be passed by reference, value given in %s on line 23

Warning: f(): Argument #2 ($k) must be passed by reference, value given in %s on line 23

Warning: Array to string conversion in %s on line 24
3Array
