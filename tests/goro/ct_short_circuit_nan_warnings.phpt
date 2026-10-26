--TEST--
Constant && / || / xor fold at compile time: NAN coercion warnings then, dead right operands uncompiled
--FILE--
<?php
function f1() { return [0] || (NAN . ""); }
function f2() { return (NAN xor true); }
function f3() { return NAN ? 1 : 2; }
function f4() { return !NAN; }
function f5() { return NAN && 1; }
function f6() { return (bool)NAN; }
function f7() { return NAN . "x"; }
function f8() { return 1 || NAN; }
function f9() { return 0 || NAN; }
function f10() { $x = NAN; return $x ? 1 : 2; }
function f11() { return INF ? 1 : 2; }
function f12() { return [NAN] == [NAN]; }
function f13() { return (NAN == true); }
function f14() { return ~NAN; }
function f15() { return NAN | 1; }
function f16() { return (int)NAN; }
function f17() { return "a" . -NAN; }
function f18() { return NAN ?: 5; }
function f19() { return NAN ?? 5; }
function f20() { if (NAN) { return 1; } return 2; }
function f21() { while (NAN) { return 1; } return 2; }
function f22() { return match(true) { NAN => 1, default => 2 }; }
echo "start\n";
for ($i = 1; $i <= 22; $i++) { echo "f$i: "; try { var_dump(("f$i")()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 3

Warning: unexpected NAN value was coerced to bool in %s on line 6

Warning: unexpected NAN value was coerced to string in %s on line 8

Warning: unexpected NAN value was coerced to bool in %s on line 10

Warning: unexpected NAN value was coerced to string in %s on line 18
start
f1: bool(true)
f2: bool(false)
f3: 
Warning: unexpected NAN value was coerced to bool in %s on line 4
int(1)
f4: 
Warning: unexpected NAN value was coerced to bool in %s on line 5
bool(false)
f5: bool(true)
f6: 
Warning: unexpected NAN value was coerced to bool in %s on line 7
bool(true)
f7: string(4) "NANx"
f8: bool(true)
f9: bool(true)
f10: 
Warning: unexpected NAN value was coerced to bool in %s on line 11
int(1)
f11: int(1)
f12: bool(false)
f13: bool(true)
f14: 
Warning: The float NAN is not representable as an int, cast occurred in %s on line 15

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 15
int(-1)
f15: 
Warning: The float NAN is not representable as an int, cast occurred in %s on line 16

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 16
int(1)
f16: 
Warning: The float NAN is not representable as an int, cast occurred in %s on line 17
int(0)
f17: string(4) "aNAN"
f18: 
Warning: unexpected NAN value was coerced to bool in %s on line 19
float(NAN)
f19: float(NAN)
f20: 
Warning: unexpected NAN value was coerced to bool in %s on line 21
int(1)
f21: 
Warning: unexpected NAN value was coerced to bool in %s on line 22
int(1)
f22: int(2)
