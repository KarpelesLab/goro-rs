--TEST--
Calls of functions and methods with attributes, run repeatedly: only #[\Deprecated] and #[\NoDiscard] do something at the call
--FILE--
<?php
#[Attribute]
class Pure {}
interface V { function enter(int $n): int; }
abstract class Base implements V {
    function enter(int $n): int { return $n; }
}
class A extends Base {
    #[Override, Pure]
    function enter(int $n): int { return $n + 1; }
    #[\Deprecated("use enter")]
    function old(): int { return 0; }
    #[\NoDiscard]
    function must(): int { return 7; }
    #[Pure]
    static function st(int $x): int { return $x * 2; }
}
#[Pure]
function f(int $x): int { return $x + 10; }
#[\Deprecated]
function g(): int { return 1; }
#[\NoDiscard("keep it")]
function h(): int { return 2; }
$cl = #[Pure] fn($x) => $x * 3;
$a = new A;
for ($i = 0; $i < 3; $i++) {
    echo $a->enter($i), ' ', A::st($i), ' ', f($i), ' ', $cl($i), ' ', implode(',', array_map('f', [$i])), ' ',
        implode(',', array_map([$a, 'enter'], [$i])), "\n";
    echo $a->old(), g(), "\n";
    $a->must();
    h();
    $x = h() + $a->must();
    echo $x, "\n";
    echo implode(',', array_map('g', [1])), "\n";
}
?>
--EXPECTF--
1 0 10 0 10 1

Deprecated: Method A::old() is deprecated, use enter in %s on line 29
0
Deprecated: Function g() is deprecated in %s on line 29
1

Warning: The return value of method A::must() should either be used or intentionally ignored by casting it as (void) in %s on line 30

Warning: The return value of function h() should either be used or intentionally ignored by casting it as (void), keep it in %s on line 31
9

Deprecated: Function g() is deprecated in %s on line 34
1
2 2 11 3 11 2

Deprecated: Method A::old() is deprecated, use enter in %s on line 29
0
Deprecated: Function g() is deprecated in %s on line 29
1

Warning: The return value of method A::must() should either be used or intentionally ignored by casting it as (void) in %s on line 30

Warning: The return value of function h() should either be used or intentionally ignored by casting it as (void), keep it in %s on line 31
9

Deprecated: Function g() is deprecated in %s on line 34
1
3 4 12 6 12 3

Deprecated: Method A::old() is deprecated, use enter in %s on line 29
0
Deprecated: Function g() is deprecated in %s on line 29
1

Warning: The return value of method A::must() should either be used or intentionally ignored by casting it as (void) in %s on line 30

Warning: The return value of function h() should either be used or intentionally ignored by casting it as (void), keep it in %s on line 31
9

Deprecated: Function g() is deprecated in %s on line 34
1
