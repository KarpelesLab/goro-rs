--TEST--
Conditions (if/elseif/while/do-while/for/match arms/?:/??) report the line of their last compiled expression
--FILE--
<?php
if (false) {
} elseif ($undef1) {
} else if ($undef2) {
}
$i = 0;
while ($i++ < 2 && $undef3) {
}
do {
} while (
    $undef4
);
for ($j = 0;
     $j < $undef5;
     $j++) {}
switch (1) {
    case $undef6:
        break;
}
$m = match (true) {
    $undef7 => 1,
    default => 2,
};
if (false):
elseif ($undef8):
endif;
$t = $undef9
    ? 1
    : 2;
$t2 = false
    ?: $undef10;
$t3 = null
    ?? $undef11;
--EXPECTF--

Warning: Undefined variable $undef1 in %s on line 3

Warning: Undefined variable $undef2 in %s on line 4

Warning: Undefined variable $undef3 in %s on line 7

Warning: Undefined variable $undef4 in %s on line 11

Warning: Undefined variable $undef5 in %s on line 14

Warning: Undefined variable $undef6 in %s on line 17

Warning: Undefined variable $undef7 in %s on line 21

Warning: Undefined variable $undef8 in %s on line 25

Warning: Undefined variable $undef9 in %s on line 27

Warning: Undefined variable $undef10 in %s on line 31

Warning: Undefined variable $undef11 in %s on line 33
