--TEST--
Enum cases are uncomparable: <=>, ==, sort() and max()/min() see ZEND_UNCOMPARABLE
--FILE--
<?php
// Enum cases are not comparable (zend_objects_not_comparable): <=> gives 1
// both ways, no int cast notice, they only equal themselves.
enum E { case A; case B; }
var_dump(E::A <=> E::A, E::A == E::A, E::A <=> E::B, E::B <=> E::A);
var_dump(E::A <=> 1, 1 <=> E::A, E::A == 1, E::A < 1, E::A > 1);
var_dump(E::A <=> null, null <=> E::A, E::A == true, E::A <=> false, false <=> E::A);
var_dump(E::A <=> "A", "A" <=> E::A, [E::A] <=> [E::B], E::A <=> [1], [1] <=> E::A);
var_dump(max(E::A, 1), min(E::A, 1), max(1, E::A), min(1, E::A));
$a = [E::B, E::A, 1, E::A];
sort($a);
var_dump($a);
--EXPECT--
int(0)
bool(true)
int(1)
int(1)
int(1)
int(1)
bool(false)
bool(false)
bool(false)
int(1)
int(-1)
bool(true)
int(1)
int(-1)
int(1)
int(1)
int(1)
int(1)
int(1)
int(1)
enum(E::A)
enum(E::A)
int(1)
array(4) {
  [0]=>
  int(1)
  [1]=>
  enum(E::A)
  [2]=>
  enum(E::A)
  [3]=>
  enum(E::B)
}
