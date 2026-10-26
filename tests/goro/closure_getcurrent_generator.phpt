--TEST--
Closure::getCurrent() in the body of a generator closure
--FILE--
<?php
$f = function () { yield Closure::getCurrent(); yield Closure::getCurrent(); };
$g = $f();
var_dump($g->current() === $f);
$g->next();
var_dump($g->current() === $f);
foreach ($f() as $c) {
    var_dump($c === $f);
}
$h = function () { yield from $GLOBALS['f'](); yield Closure::getCurrent(); };
foreach ($h() as $c) {
    var_dump($c === $f ? "f" : ($c === $h ? "h" : "?"));
}
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
string(1) "f"
string(1) "f"
string(1) "h"
