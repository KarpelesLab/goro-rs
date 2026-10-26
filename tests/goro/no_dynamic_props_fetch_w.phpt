--TEST--
Internal classes without dynamic properties: write fetches and ++ of an undefined property are Errors
--FILE--
<?php
foreach ([new Random\Randomizer, function() {}, new WeakMap] as $o) {
 foreach ([fn() => $o->a = 1, function() use ($o) { $o->b[] = 1; }, function() use ($o) { $x = &$o->c; }, function() use ($o) { $o->d++; }, function() use ($o) { --$o->e; }] as $f) { try { $f(); echo "ok\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; } }
}
--EXPECT--
Cannot create dynamic property Random\Randomizer::$a
Cannot create dynamic property Random\Randomizer::$b
Cannot create dynamic property Random\Randomizer::$c
Cannot create dynamic property Random\Randomizer::$d
Cannot create dynamic property Random\Randomizer::$e
Cannot create dynamic property Closure::$a
Cannot create dynamic property Closure::$b
Cannot create dynamic property Closure::$c
Cannot create dynamic property Closure::$d
Cannot create dynamic property Closure::$e
Cannot create dynamic property WeakMap::$a
Cannot create dynamic property WeakMap::$b
Cannot create dynamic property WeakMap::$c
Cannot create dynamic property WeakMap::$d
Cannot create dynamic property WeakMap::$e
