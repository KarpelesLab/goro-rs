--TEST--
compact(): "Recursion detected" is thrown from compact()'s frame
--FILE--
<?php
function f() {
    $a = 1;
    $r = ['a'];
    $r[] = &$r;
    try {
        compact($r, 'a');
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
        foreach ($e->getTrace() as $fr) {
            echo $fr['function'], " ", count($fr['args']), "\n";
        }
    }
}
f();
?>
--EXPECT--
Recursion detected
compact 2
f 0
