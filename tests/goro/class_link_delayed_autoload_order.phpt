--TEST--
Delayed variance autoloads are loaded by the class with unresolved obligations, after the one being loaded; the link error shows the autoloader frames
--INI--
fatal_error_backtraces=1
--FILE--
<?php
spl_autoload_register(function($class) {
    echo "load $class\n";
    if ($class == 'A') {
        class A { function m(): B {} function m2(): B {} }
        var_dump(new A);
    } elseif ($class == 'B') {
        class B extends A { function m(): X {} function m2(): Y {} }
        var_dump(new B);
    } elseif ($class == 'X') {
        class X extends A {}
        var_dump(new X);
    } else {
        class Y extends B {}
        var_dump(new Y);
    }
});
new B;
--EXPECTF--
load B
load A
object(A)#2 (0) {
}
load X
object(X)#2 (0) {
}
load Y

Fatal error: Declaration of B::m(): X must be compatible with A::m(): B in %s on line 8
Stack trace:
#0 %s(8): {closure:%s:2}('Y')
#1 %s(18): {closure:%s:2}('B')
#2 {main}
