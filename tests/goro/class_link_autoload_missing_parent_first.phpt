--TEST--
Class linking: a missing parent fails before the traits are loaded
--FILE--
<?php
spl_autoload_register(function ($c) { echo "load $c\n"; if ($c === 'A') eval('class A {}'); });
try { eval('class B extends A { use Missing; }'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { eval('class C extends Missing2 { use Missing3; }'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
load A
load Missing
Error: Trait "Missing" not found
load Missing2
Error: Class "Missing2" not found
