--TEST--
__get/__isset dispatch has no depth cap (each object has its own guard)
--FILE--
<?php
class Two { public $d = 0; function __get($n) { $t = new Two; $t->d = $this->d + 1; if ($t->d > 50000) return "deep"; return $t->$n . "!"; } }
try { $x = (new Two)->q; var_dump(strlen($x)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Three { function __get($n) { static $d = 0; $d++; if ($d > 10) return "x"; echo "get $d\n"; return (new Three)->$n . "!"; } }
var_dump((new Three)->q);
--EXPECT--
Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
get 1
get 2
get 3
get 4
get 5
get 6
get 7
get 8
get 9
get 10
string(11) "x!!!!!!!!!!"
