--TEST--
A special-compiled builtin (strlen, is_int...) passed by reference to a user function: Error, not a notice
--FILE--
<?php
function t(&$r) {}
try { t(1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$s = "ab";
try { t(strlen($s)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { t(is_int($s)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { t(count([])); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { t(strtoupper($s)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { t($s . "x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
t(): Argument #1 ($r) could not be passed by reference
t(): Argument #1 ($r) could not be passed by reference
t(): Argument #1 ($r) could not be passed by reference
t(): Argument #1 ($r) could not be passed by reference

Notice: Only variables should be passed by reference in %s on line 8
t(): Argument #1 ($r) could not be passed by reference
