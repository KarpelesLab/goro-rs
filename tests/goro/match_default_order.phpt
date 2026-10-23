--TEST--
match: the default arm applies only after every other arm failed
--FILE--
<?php
// match: the default arm is taken only when no other arm matches, wherever
// it is written
function m($v) {
    return match(true) { $v < 0 => 'neg', default => 'other', $v > 10 => 'big', $v === 5 => 'five' };
}
var_dump(m(-1), m(5), m(20), m(7));
$i = 0;
var_dump(match($i++) { default => 'd', 0 => 'zero' }, $i);
var_dump(match('x') { 'a', 'b' => 1, default => 2, 'x' => 3 });
--EXPECT--
string(3) "neg"
string(4) "five"
string(3) "big"
string(5) "other"
string(4) "zero"
int(1)
int(3)
