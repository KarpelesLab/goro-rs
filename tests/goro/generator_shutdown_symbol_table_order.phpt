--TEST--
Generators in the global symbol table are destroyed in its reverse order at shutdown, after the shutdown functions
--FILE--
<?php
class Dd { function __construct(public $n) {} function __destruct() { echo "Dd{$this->n} dtor\n"; } }
function endg($n) { try { yield 1; } finally { echo "gen$n finally\n"; } }
register_shutdown_function(function() { echo "shutdown fn\n"; });
$a = new Dd(1);
$e = endg(1); $e->current();
$c = new Dd(3);
echo "script end\n";
--EXPECT--
script end
shutdown fn
Dd3 dtor
gen1 finally
Dd1 dtor
