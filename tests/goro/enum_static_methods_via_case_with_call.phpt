--TEST--
An enum's cases()/from()/tryFrom() called on a case are not routed to its __call()
--FILE--
<?php
enum E: int { case A = 1; case B = 2; }
var_dump(E::A->from(2));
var_dump(E::A->tryFrom(3));
var_dump(E::B->cases());
$m = 'from';
var_dump(E::A->$m(1));
var_dump([E::A, 'from'](2));
var_dump(call_user_func([E::A, 'tryFrom'], 1));
enum F: int { case X = 1;
  public function __call($n, $a) { return "c $n"; } }
var_dump(F::X->from(1), F::X->cases());
$c = new ArrayObject();
var_dump(Closure::fromCallable([E::A, 'from'])(1));
--EXPECT--
enum(E::B)
NULL
array(2) {
  [0]=>
  enum(E::A)
  [1]=>
  enum(E::B)
}
enum(E::A)
enum(E::B)
enum(E::A)
enum(F::X)
array(1) {
  [0]=>
  enum(F::X)
}
enum(E::A)
