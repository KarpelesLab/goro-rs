--TEST--
UnhandledMatchError line: conditions folded by can_match_use_jumptable() are on the subject's line
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo $e->getLine(), "\n"; } }
t(function () { $x = 1 + match(true) {
        false => 1,
    }; });
t(function () { $x = match(true) {
        false => 1,
    }; });
t(function () { $x = match(5) {
        1 => 1,
    }; });
t(function () { $y = 3; $x = match(true) {
        $y > 5 => 1,
    }; });
t(function () { $x = match(true) {
        false,
        null => 1,
    }; });
t(function () { $x = 1 +
     match(true) {
        false => 1,
    }; });
t(function () { $x = match(true) {
        null => 1,
    }; });
t(function () { $x = match(true) {
        false,
        false => 1,
    }; });
t(function () { $x = match(true) {
        false => 1,
        null => 2,
    }; });
t(function () { $x = match(true) {
        0 => 1,
    }; });
t(function () { $x = match(true) {
        "a" => 1,
    }; });
t(function () { $x = match(true) {
        1.5 => 1,
    }; });
t(function () { $x = match(true) {
        PHP_INT_MAX => 1,
    }; });
t(function () { $x = match(true) {
        [] => 1,
    }; });
--EXPECT--
3
6
10
13
17
20
23
28
32
35
38
41
43
46
