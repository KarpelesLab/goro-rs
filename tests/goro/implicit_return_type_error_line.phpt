--TEST--
Return type error of an implicit return: on the function's last line
--FILE--
<?php
function f(): int {
    if (false) {
        return 1;
    }
}
class C {
    function m(): ?array
    {
        echo "m\n";

    }
}
try {
    f();
} catch (TypeError $e) {
    echo $e->getMessage(), ' @ ', $e->getLine(), "\n";
}
try {
    (new C)->m();
} catch (TypeError $e) {
    echo $e->getMessage(), ' @ ', $e->getLine(), "\n";
}
$c = function (): string {
    echo "c\n";
};
try {
    $c();
} catch (TypeError $e) {
    echo $e->getMessage(), ' @ ', $e->getLine(), "\n";
}
--EXPECTF--
f(): Return value must be of type int, none returned @ 6
m
C::m(): Return value must be of type ?array, none returned @ 12
c
{closure:%s:24}(): Return value must be of type string, none returned @ 26
