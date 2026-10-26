--TEST--
Each constant of a grouped class constant declaration takes the doc comment lexed up to its value
--FILE--
<?php
class A {
    /** const doc */
    const X = 1, /** second */ Y = 2, Z = 3;
    const W = 4 /** after value */, V = 5;
}
foreach ((new ReflectionClass('A'))->getReflectionConstants() as $c) {
    var_dump($c->name, $c->getDocComment());
}
--EXPECT--
string(1) "X"
string(16) "/** const doc */"
string(1) "Y"
string(13) "/** second */"
string(1) "Z"
bool(false)
string(1) "W"
string(18) "/** after value */"
string(1) "V"
bool(false)
