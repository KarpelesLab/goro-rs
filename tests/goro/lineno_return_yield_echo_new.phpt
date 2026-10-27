--TEST--
return/yield/yield from/echo/print are on the line of their operand; new on the class name's
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; } }
function r1() {
    return
        $u1;
}
r1();
function r2(): int {
    return
        "x";
}
t('r2');
function r3(): int {
    return [
        1,
    ];
}
t('r3');
function g() {
    yield
        $u2;
    yield 1
        => $u3;
    $x = yield
        from
        $u4;
}
t(function () { foreach (g() as $v) {} });
function r4(): int {
    return $a
        ?? "zz";
}
t('r4');
function &r5() {
    return
        5;
}
$v = &r5();
function r6() {
    throw
        new Exception("x");
}
t('r6');
function r7() {
    throw new
        Exception("x");
}
t('r7');
function r8() {
    $e = new Exception("y");
    throw
        $e;
}
t('r8');
echo
    $u5;
print
    $u6;
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 5
r2(): Return value must be of type int, string returned @10
r3(): Return value must be of type int, array returned @15

Warning: Undefined variable $u2 in %s on line 21

Warning: Undefined variable $u3 in %s on line 23

Warning: Undefined variable $u4 in %s on line 26
Can use "yield from" only with arrays and Traversables @26
r4(): Return value must be of type int, string returned @31

Notice: Only variable references should be returned by reference in %s on line 36
x @41
x @46
y @50

Warning: Undefined variable $u5 in %s on line 56

Warning: Undefined variable $u6 in %s on line 58
