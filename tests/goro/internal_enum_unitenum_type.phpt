--TEST--
An internal enum's case satisfies a UnitEnum parameter / return / property type (RoundingMode, Random\IntervalBoundary)
--FILE--
<?php
function g(UnitEnum $x) { return get_class($x); }
function h(UnitEnum|array|int $x): string { return get_class($x); }
function k(?UnitEnum $x): ?UnitEnum { return $x; }
class P { public UnitEnum $e; public ?UnitEnum $n = null; }
var_dump(g(RoundingMode::HalfAwayFromZero));
var_dump(h(Random\IntervalBoundary::ClosedOpen));
var_dump(k(RoundingMode::HalfEven));
$p = new P;
$p->e = RoundingMode::TowardsZero;
$p->n = Random\IntervalBoundary::OpenOpen;
var_dump($p->e, $p->n);
try { g(new stdClass); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
function b(BackedEnum $x) { return 1; }
try { b(RoundingMode::HalfOdd); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--
string(12) "RoundingMode"
string(23) "Random\IntervalBoundary"
enum(RoundingMode::HalfEven)
enum(RoundingMode::TowardsZero)
enum(Random\IntervalBoundary::OpenOpen)
g(): Argument #1 ($x) must be of type UnitEnum, stdClass given, called in %s on line %d
b(): Argument #1 ($x) must be of type BackedEnum, RoundingMode given, called in %s on line %d
