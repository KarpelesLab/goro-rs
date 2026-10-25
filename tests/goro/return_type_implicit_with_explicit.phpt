--TEST--
A function with explicit returns that falls off its end returns none for its return type
--FILE--
<?php
function f2(): ?int { if (false) return 1; }
function run() {
    try { var_dump(f2()); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
run();
try { var_dump(f2()); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
f2(): Return value must be of type ?int, none returned
f2(): Return value must be of type ?int, none returned
