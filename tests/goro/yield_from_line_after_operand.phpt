--TEST--
yield from: the opline line is the line where its operand's compilation ended
--FILE--
<?php
function show() {
    $t = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS);
    echo $t[1]['function'], ' :', $t[1]['line'] ?? '-', "\n";
}
function gen() { show(); yield 1; }
function g1() {
    yield from
        gen(
        );
}
function g2() {
    yield from (function () {
        show();
        yield 1;
    })(
        1
    );
}
function g3() {
    $x = yield
        from gen();
}
function g4() {
    yield from ($f = function () {
        show();
        yield 1;
    })(
        1
    ) ;
}
function g5() {
    $a = [(function () {
        show();
        return 1;
    })(
        1
    ), 2];
}
foreach (g1() as $_) {}
foreach (g2() as $_) {}
foreach (g3() as $_) {}
foreach (g4() as $_) {}
g5();
function g6() {
    yield from (function () {
        show();
        yield 1;
    })();
}
function g8() {
    $g = gen();
    yield from
        $g;
}
foreach (g6() as $_) {}
foreach (g8() as $_) {}

?>
--EXPECT--
gen :9
{closure:g2():13} :17
gen :22
{closure:g4():25} :29
{closure:g5():33} :36
{closure:g6():46} :49
gen :54
