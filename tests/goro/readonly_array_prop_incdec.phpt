--TEST--
++/-- of an initialized readonly array property: the readonly error (previous: the TypeError)
--FILE--
<?php
class R { public function __construct(public readonly array $a = [], public readonly int $i = 0) {} }
$r = new R([1]);
try { $r->a++; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { ++$r->a; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $x = $r->a++; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->a--; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " / ", get_class($e->getPrevious() ?? $e), "\n"; }
try { $r->i++; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { --$r->i; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = function () use ($r) { $r->a++; };
try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
Error: Cannot modify readonly property R::$a
Error: Cannot modify readonly property R::$a
Error: Cannot modify readonly property R::$a
Error: Cannot modify readonly property R::$a / TypeError
Error: Cannot modify readonly property R::$i
Error: Cannot modify readonly property R::$i
Error: Cannot modify readonly property R::$a
