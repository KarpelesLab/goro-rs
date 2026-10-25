--TEST--
Unpacking an unknown named argument fails before a later positional one
--FILE--
<?php
try { var_dump((fn()=>1)(...['a'=>1, 5=>'c'])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump((fn($a)=>$a)(...['a'=>1, 5=>'c'])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function f0() { return 1; }
try { var_dump(f0(...['a'=>1, 5=>'c'])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(f0(...['a'=>1])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(strlen(...['a'=>1, 5=>'c'])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
Error: Unknown named parameter $a
Error: Cannot use positional argument after named argument during unpacking
Error: Unknown named parameter $a
Error: Unknown named parameter $a
Error: Unknown named parameter $a
