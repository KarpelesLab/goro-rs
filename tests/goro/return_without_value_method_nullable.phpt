--TEST--
return; in a method with a nullable return type names a method
--FILE--
<?php
function f(): ?int { return 1; }
class A {
    function f(): ?int {
        $g = function (): int { return 1; };
        return;
    }
}
--EXPECTF--

Fatal error: A method with return type must return a value (did you mean "return null;" instead of "return;"?) in %s on line 6
