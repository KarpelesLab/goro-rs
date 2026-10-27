--TEST--
Without a compile-time known callee, a variable argument is sent on the argument list's line (a25)
--FILE--
<?php
namespace NS;
function known($a, $b = 0) {}
known(
    1,
    $u1
);
\NS\known(
    1,
    $u2
);
strlen2(...[]) ?? null;
$f = 'NS\known';
$f(
    1,
    $u3
);
function inner() {
    known(
        1,
        $u4
    );
    \str_repeat(
        "a",
        $u5
    );
    str_repeat(
        "a",
        $u6
    );
}
inner();
function strlen2() { return 1; }
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 5

Warning: Undefined variable $u2 in %s on line 10

Warning: Undefined variable $u3 in %s on line 15

Warning: Undefined variable $u4 in %s on line 20

Warning: Undefined variable $u5 in %s on line 25

Deprecated: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated in %s on line 23

Warning: Undefined variable $u6 in %s on line 28

Deprecated: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated in %s on line 27
