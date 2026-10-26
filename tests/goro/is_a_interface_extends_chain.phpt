--TEST--
is_a() / is_subclass_of() follow the interfaces an implemented interface extends
--FILE--
<?php
interface I {}
interface K {}
interface J extends I {}
interface L extends K, J {}
abstract class A implements J {}
class X extends A {}
class Y implements J {}
class Z implements L {}
foreach (['X', 'Y', 'Z', 'J', 'L'] as $c) {
    foreach (['I', 'J', 'K', 'L'] as $i) {
        echo "$c $i: ", var_export(is_a($c, $i, true), true), " ", var_export(is_subclass_of($c, $i), true), "\n";
    }
}
var_dump(is_a(new X, 'I'), is_a(new Z, 'I'), is_subclass_of(new Y, 'I'));

?>
--EXPECT--
X I: true true
X J: true true
X K: false false
X L: false false
Y I: true true
Y J: true true
Y K: false false
Y L: false false
Z I: true true
Z J: true true
Z K: true true
Z L: true true
J I: true true
J J: true false
J K: false false
J L: false false
L I: true true
L J: true true
L K: true true
L L: true false
bool(true)
bool(true)
bool(true)
