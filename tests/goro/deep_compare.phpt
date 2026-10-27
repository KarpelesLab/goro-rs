--TEST--
== / <=> / === of deeply nested arrays and objects
--FILE--
<?php
function mk($n) { $a = []; for ($i = 0; $i < $n; $i++) $a = [$a]; return $a; }
function mko($n) { $o = new stdClass; for ($i = 0; $i < $n; $i++) { $p = new stdClass; $p->c = $o; $o = $p; } return $o; }
$a = mk(20000);
$b = mk(20000);
var_dump($a == $b, $a <=> $b, $a === $b, $a != mk(19999));
$a = mko(15000);
$b = mko(15000);
var_dump($a == $b, $a <=> $b);
// object comparison checks the stack (php-src: ~22.6k levels)
$a = mko(30000);
$b = mko(30000);
try {
    var_dump($a == $b);
} catch (Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
try {
    var_dump($a <=> $b);
} catch (Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
// recursion is still detected
$r = [];
$r[0] = &$r;
$s = [];
$s[0] = &$s;
try {
    var_dump($r == $s);
} catch (Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
--EXPECT--
bool(true)
int(0)
bool(true)
bool(true)
bool(true)
int(0)
Error: Maximum call stack size reached during object comparison
Error: Maximum call stack size reached during object comparison
Error: Nesting level too deep - recursive dependency?
