--TEST--
return $undef: an exception the error handler throws escapes the function's catch
--FILE--
<?php
set_error_handler(function($n, $s) { throw new Exception($s); });
function f() {
    try {
        return $undef;
    } catch (Exception $e) {
        echo "caught in f: ", $e->getMessage(), "\n";
        return 1;
    }
}
function g() {
    try {
        $x = $undef;
        return $x;
    } catch (Exception $e) {
        echo "caught in g: ", $e->getMessage(), "\n";
        return 2;
    }
}
function h() {
    try {
        return $undef . "x";
    } catch (Exception $e) {
        echo "caught in h: ", $e->getMessage(), "\n";
        return 3;
    }
}
function k() {
    try {
        try {
            return $undef;
        } catch (Exception $e) {
            echo "caught inner k\n";
        }
    } catch (Exception $e) {
        echo "caught outer k: ", $e->getMessage(), "\n";
        return 4;
    }
}
function m() {
    try {
        return $undef;
    } catch (Exception $e) {
        echo "caught m\n";
    } finally { echo "finally m\n"; }
}
function n() {
    foreach ([1] as $v) {
        try {
            return $undef;
        } catch (Exception $e) { echo "caught n\n"; }
    }
}
function p() {
    try {
        return $undef[0];
    } catch (Exception $e) { echo "caught p\n"; return 5; }
}
function q() {
    try {
        return $this_undef->x ?? $undef;
    } catch (Exception $e) { echo "caught q\n"; return 6; }
}
function r() {
    try {
        return (int)$undef;
    } catch (Exception $e) { echo "caught r\n"; return 7; }
}
function s(&$ref = null) {
    try {
        return $undef;
    } catch (Exception $e) { echo "caught s\n"; return 8; }
}
function &rr() {
    try {
        return $undef;
    } catch (Exception $e) { echo "caught rr\n"; $z = 9; return $z; }
}
function t(): int {
    try {
        return $undef;
    } catch (Exception $e) { echo "caught t\n"; return 10; }
}
function u() {
    $a = 1;
    try {
        return $a + $undef;
    } catch (Exception $e) { echo "caught u\n"; return 11; }
}
function gen() {
    try {
        return $undef;
    } catch (Exception $e) { echo "caught gen\n"; }
    yield 1;
}
class C { static function sm() { try { return $undef; } catch (Exception $e) { echo "caught sm\n"; return 12; } }
          function im() { try { return $undef; } catch (Exception $e) { echo "caught im\n"; return 13; } } }
foreach (['f','g','h','k','m','n','p','q','r','s','rr','t','u','C::sm'] as $fn) {
    try { var_dump($fn()); } catch (Exception $e) { echo "outer $fn: ", $e->getMessage(), "\n"; }
}
try { var_dump((new C)->im()); } catch (Exception $e) { echo "outer im\n"; }
try { $g = gen(); foreach ($g as $_) {} var_dump($g->getReturn()); } catch (Throwable $e) { echo "outer gen: ", get_class($e), ' ', $e->getMessage(), "\n"; }
try { var_dump((function() { try { return $u; } catch (Exception $e) { return 'c'; } })()); } catch (Exception $e) { echo "outer closure\n"; }
try { var_dump((fn() => $u)()); } catch (Exception $e) { echo "outer arrow\n"; }
try { return $undef_top; } catch (Exception $e) { echo "caught top\n"; }
echo "end\n";
--EXPECTF--
outer f: Undefined variable $undef
caught in g: Undefined variable $undef
int(2)
caught in h: Undefined variable $undef
int(3)
outer k: Undefined variable $undef
caught m
finally m
NULL
outer n: Undefined variable $undef
caught p
int(5)
caught q
int(6)
caught r
int(7)
outer s: Undefined variable $undef
NULL
caught t
int(10)
caught u
int(11)
outer C::sm: Undefined variable $undef
outer im
outer gen: Exception Undefined variable $undef
outer closure
outer arrow

Fatal error: Uncaught Exception: Undefined variable $undef_top in %s:2
Stack trace:
#0 %s(105): {closure:%s:2}(2, 'Undefined varia...', '%s', 105)
#1 {main}
  thrown in %s on line 2
