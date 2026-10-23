--TEST--
zpp: typed by-reference parameters and the callbacks after them are checked
--FILE--
<?php
// zpp: typed by-reference parameters are checked (and the parameters after
// them, like the comparison callbacks of the u*sort() functions)
$a = [3, 1];
foreach ([
    fn() => usort($a, 'nope'),
    fn() => uasort($a, 'nope'),
    fn() => uksort($a, [new stdClass, 'm']),
    function () { $s = 'x'; return sort($s); },
    function () { return sort($undef); },
    function () { $i = 5; return array_push($i, 1); },
    function () { $o = new ArrayObject([2, 1]); return array_walk($o, fn() => 1); },
    function () { $o = 1.5; return array_walk($o, fn() => 1); },
] as $f) {
    try {
        var_dump($f());
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
}
--EXPECT--
usort(): Argument #2 ($callback) must be a valid callback, function "nope" not found or invalid function name
uasort(): Argument #2 ($callback) must be a valid callback, function "nope" not found or invalid function name
uksort(): Argument #2 ($callback) must be a valid callback, class stdClass does not have a method "m"
sort(): Argument #1 ($array) must be of type array, string given
sort(): Argument #1 ($array) must be of type array, null given
array_push(): Argument #1 ($array) must be of type array, int given
bool(true)
array_walk(): Argument #1 ($array) must be of type array, float given
