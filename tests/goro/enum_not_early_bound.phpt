--TEST--
Enums are never early-bound: they exist from their declaration on
--FILE--
<?php
var_dump(enum_exists("E0"));
var_dump(class_exists('C1'));
var_dump(interface_exists('I1'));
enum E0 { case A; }
class C1 {}
interface I1 {}
var_dump(enum_exists("E0"));
var_dump(E0::A);
#[Attr(E::A)]
class X {
    public E $p = E::A;
    public ?E $q = null;
    const C = [E::A, E::B];
    function f(E|int $e = E::B): E { return $e; }
    static function g(E ...$e) { return $e; }
}
try { var_dump(E::A); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(X::C); } catch (Error $e) { echo $e->getMessage(), "\n"; }
function early() { return E::cases(); }
try { var_dump(early()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
enum E: int {
    case A = 1;
    case B = 2;
    const D = X::C;
    public function lbl(): string { return ucfirst(strtolower($this->name)); }
}
interface I {}
var_dump(E::A instanceof UnitEnum, E::D);
var_dump((new X)->p, (new X)->f(), X::g(E::A), X::C, early());
var_dump((new ReflectionClass('X'))->getAttributes()[0]->getArguments());
var_dump(E::from(2)->lbl());
enum F { case Z; }
var_dump(F::Z);
--EXPECTF--
bool(false)
bool(true)
bool(true)
bool(true)
enum(E0::A)
Class "E" not found
Class "E" not found
Class "E" not found
bool(true)
array(2) {
  [0]=>
  enum(E::A)
  [1]=>
  enum(E::B)
}
enum(E::A)
enum(E::B)
array(1) {
  [0]=>
  enum(E::A)
}
array(2) {
  [0]=>
  enum(E::A)
  [1]=>
  enum(E::B)
}
array(2) {
  [0]=>
  enum(E::A)
  [1]=>
  enum(E::B)
}
array(1) {
  [0]=>
  enum(E::A)
}
string(1) "B"
enum(F::Z)
