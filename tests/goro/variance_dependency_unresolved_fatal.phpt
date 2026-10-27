--TEST--
A class extending one still linking resolves its obligations: an unresolved one is fatal inside the autoloader
--INI--
fatal_error_backtraces=1
--FILE--
<?php
spl_autoload_register(function ($class) {
    echo "autoload $class\n";
    if ($class == 'A') {
        class A {
            function m(): B {}
        }
    } elseif ($class == 'B') {
        class B extends A {
            function m(): X {}
        }
    } else {
        echo "declaring C\n";
        class C extends B {}
        echo "declared C\n";
    }
});
new B;
--EXPECTF--
autoload B
autoload A
autoload X
declaring C

Fatal error: Could not check compatibility between B::m(): X and A::m(): B, because class X is not available in %s on line 10
Stack trace:
#0 %s(9): {closure:%s:2}('X')
#1 %s(18): {closure:%s:2}('B')
#2 {main}
