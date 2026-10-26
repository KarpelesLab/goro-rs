--TEST--
Unknown named parameter of an internal method: no frame for the method
--FILE--
<?php
function f() {
    try { (new DateTime)->format(foo: 1); } catch (Error $e) {
        echo $e->getMessage(), "\n";
        foreach ($e->getTrace() as $fr) echo $fr['function'], "\n";
    }
    try { (new DateTimeZone('UTC'))->getOffset(bar: 1); } catch (Error $e) {
        echo $e->getMessage(), "\n";
        foreach ($e->getTrace() as $fr) echo $fr['function'], "\n";
    }
}
f();
--EXPECT--
Unknown named parameter $foo
f
Unknown named parameter $bar
f
