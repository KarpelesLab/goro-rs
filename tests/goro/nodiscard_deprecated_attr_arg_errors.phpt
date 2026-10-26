--TEST--
#[\NoDiscard] / #[\Deprecated] arguments are bound to the attribute's constructor
--FILE--
<?php
#[NoDiscard("a", "b")] function f1() { return 1; }
#[NoDiscard(nope: "x")] function f2() { return 1; }
#[Deprecated("a", "b", "c")] function f3() { return 1; }
#[Deprecated(nope: "x")] function f4() { return 1; }
foreach (['f1', 'f2', 'f3', 'f4'] as $f) {
    try {
        $f();
    } catch (Error $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
        foreach ($e->getTrace() as $fr) {
            echo "  ", $fr['class'] ?? '', $fr['type'] ?? '', $fr['function'], "(", implode(", ", $fr['args']), ")\n";
        }
    }
}
?>
--EXPECT--
ArgumentCountError: NoDiscard::__construct() expects at most 1 argument, 2 given
  NoDiscard->__construct(a, b)
Error: Unknown named parameter $nope
ArgumentCountError: Deprecated::__construct() expects at most 2 arguments, 3 given
  Deprecated->__construct(a, b, c)
Error: Unknown named parameter $nope
