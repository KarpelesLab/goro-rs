--TEST--
The first fetch of a backed enum's case creates all its case objects (in order); a pure enum's cases are created one by one
--FILE--
<?php
enum Suit: string { case Hearts = 'H'; case Spades = 'S'; case Clubs = 'C'; }
enum U { case A; case B; case C; }
var_dump(spl_object_id(new stdClass));
var_dump(spl_object_id(U::B));
var_dump(spl_object_id(new stdClass));
var_dump(spl_object_id(Suit::Spades));
var_dump(spl_object_id(new stdClass));
var_dump(spl_object_id(U::A));
var_dump(spl_object_id(Suit::Hearts), spl_object_id(Suit::Clubs));
var_dump(spl_object_id(new stdClass));
// from()/tryFrom() and a non-case constant fetch create them too; constant() does not
enum S2: string { case A = "a"; case B = "b"; }
enum S3: int { case A = 1; case B = 2; const X = 9; }
enum S4: int { case A = 1; case B = 2; }
var_dump(S2::tryFrom("z"), spl_object_id(new stdClass));
var_dump(S3::X, spl_object_id(new stdClass));
var_dump(constant("S4::B"), spl_object_id(new stdClass));
--EXPECTF--
int(1)
int(1)
int(2)
int(3)
int(5)
int(5)
int(2)
int(4)
int(6)
NULL
int(8)
int(9)
int(10)
enum(S4::B)
int(11)
