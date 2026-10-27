--TEST--
throw of an undefined variable warns once; $undef::C warns before its Error
--FILE--
<?php
try { throw $u; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $u9::C; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $u9::$p; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $u9::m(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $u9::class; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $u9::$p = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--

Warning: Undefined variable $u in %s on line 2
Can only throw objects

Warning: Undefined variable $u9 in %s on line 3
Class name must be a valid object or a string

Warning: Undefined variable $u9 in %s on line 4
Class name must be a valid object or a string

Warning: Undefined variable $u9 in %s on line 5
Class name must be a valid object or a string

Warning: Undefined variable $u9 in %s on line 6
Cannot use "::class" on null

Warning: Undefined variable $u9 in %s on line 7
Class name must be a valid object or a string
