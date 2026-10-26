--TEST--
Fast static property reads: visibility, inheritance, traits, typed and deferred defaults
--FILE--
<?php
trait T { public static $tp = 5; }
class A { public static $a = 1; protected static $p = 2; private static $q = 3; public static ?int $t; public static $arr = [1, 2];
  static function get() { return [self::$a, self::$p, self::$q, A::$a, static::$a, A::$arr]; } }
class B extends A { public static $b = 4; static function g() { return [B::$a, B::$p, B::$b, A::$a]; } }
class U { use T; }
function f() { return [A::$a, B::$a, B::$b, U::$tp]; }
for ($i = 0; $i < 3; $i++) {
  echo json_encode(f()), json_encode(A::get()), json_encode(B::g()), "\n";
  A::$a++; B::$b .= 'x'; A::$arr[] = $i;
  $r = &A::$a; $r += 10; unset($r);
}
function t() { try { return A::$t; } catch (Error $e) { return $e->getMessage(); } }
echo t(), "\n", t(), "\n";
A::$t = 7; echo t(), "\n";
function pq() { try { return A::$q; } catch (Error $e) { return $e->getMessage(); } }
echo pq(), "\n", pq(), "\n";
function tr() { return T::$tp; }
echo tr(), tr(), "\n";
class D { public static $d = DD; }
const DD = 9;
function d() { return D::$d; }
echo d(), d(), "\n";
--EXPECTF--
[1,1,4,5][1,2,3,1,1,[1,2]][1,2,4,1]
[12,12,"4x",5][12,2,3,12,12,[1,2,0]][12,2,"4x",12]
[23,23,"4xx",5][23,2,3,23,23,[1,2,0,1]][23,2,"4xx",23]
Typed static property A::$t must not be accessed before initialization
Typed static property A::$t must not be accessed before initialization
7
Cannot access private property A::$q
Cannot access private property A::$q

Deprecated: Accessing static trait property T::$tp is deprecated, it should only be accessed on a class using the trait in %s on line 18
5
Deprecated: Accessing static trait property T::$tp is deprecated, it should only be accessed on a class using the trait in %s on line 18
5
99
