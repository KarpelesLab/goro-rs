--TEST--
Declared return types: values of the type already, coerced ones, objects for a string type, implicit returns; temporary containers released while objects with destructors exist
--FILE--
<?php
class S { function __toString(): string { return "S!"; } }
interface I {}
class A implements I {}
class B extends A {}
function r_int(): int { return 5; }
function r_float_from_int(): float { return 5; }
function r_string_from_obj(): string { return new S; }
function r_string_from_int(): string { return 42; }
function r_int_from_numstr(): int { return "17"; }
function r_nullable(): ?A { return null; }
function r_union(): int|string { return 1.0; }
function r_iface(): I { return new B; }
function r_self_like(): A { return new B; }
function r_mixed(): mixed { return new S; }
function r_object(): object { return new S; }
function r_iterable(): iterable { return [1]; }
function r_void(): void { return; }
function r_false(): false { return false; }
function r_static_cls() { return B::make(); }
class B2 extends A {
    static function make(): static { return new static; }
}
function r_bool_from_int(): bool { return 1; }
function r_nan_bool(): bool { return NAN; }
function r_implicit_nullable(): ?int { if (false) return 1; }
var_dump(r_int(), r_float_from_int(), r_string_from_obj(), r_string_from_int(), r_int_from_numstr());
var_dump(r_nullable(), r_union(), get_class(r_iface()), get_class(r_self_like()), get_class(r_mixed()));
var_dump(get_class(r_object()), r_iterable(), r_void(), r_false(), get_class(B2::make()));
var_dump(r_bool_from_int(), r_nan_bool());
try {
    var_dump(r_implicit_nullable());
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
function r_wrong(): int { return "abc"; }
try {
    r_wrong();
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
$keep = new D('keep');
function arr_of($n) { return [$n, [$n]]; }
function tmp_containers($i) {
    $x = count(arr_of($i)) + count([new D("t$i")]);
    echo "after $i\n";
    return $x;
}
for ($i = 0; $i < 3; $i++) {
    echo tmp_containers($i), "\n";
}
$shared = [1, 2, 3];
function pass_through($a) { return $a; }
for ($i = 0; $i < 3; $i++) {
    $n = count(pass_through($shared)) + strlen(implode(',', pass_through($shared)));
    echo $n, "\n";
}
echo "end\n";
--EXPECTF--
int(5)
float(5)
string(2) "S!"
string(2) "42"
int(17)
NULL
int(1)
string(1) "B"
string(1) "B"
string(1) "S"
string(1) "S"
array(1) {
  [0]=>
  int(1)
}
NULL
bool(false)
string(2) "B2"

Warning: unexpected NAN value was coerced to bool in %s on line 25
bool(true)
bool(true)
r_implicit_nullable(): Return value must be of type ?int, none returned
r_wrong(): Return value must be of type int, string returned
dtor t0
after 0
3
dtor t1
after 1
3
dtor t2
after 2
3
8
8
8
end
dtor keep
