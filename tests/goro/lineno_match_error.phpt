--TEST--
UnhandledMatchError: after the subject with a jump table, else after the last condition
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; } }
$x = 5;
t(fn() => match ($x) {
    1 => 'a',
    2 => 'b',
});
t(fn() => match ($x) {
    1 => 'a',
    $x + 1 => 'b',
    PHP_INT_MAX => 'c',
});
t(fn() => match (
    $x
) {
    'a', 'b' => 1,
    'c'
        => 2,
});
t(fn() => match ($x) {
    1 => 'a',
});
t(fn() => match ($x) {
});
t(fn() => match (true) {
    $x < 1 => 'a',
    $x
      > 10 => 'b',
});
t(fn() => match ($x) {
    -1, -2 => 'a',
});
--EXPECT--
Unhandled match case 5 @4
Unhandled match case 5 @11
Unhandled match case 5 @14
Unhandled match case 5 @21
Unhandled match case 5 @23
Unhandled match case true @28
Unhandled match case 5 @30
