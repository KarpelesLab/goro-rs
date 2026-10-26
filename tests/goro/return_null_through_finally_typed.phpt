--TEST--
return null; through a finally block in a function with a return type is not "none returned" (PHPUnit TestCase::runTest())
--FILE--
<?php
// PHPUnit's TestCase::runTest(): `return null;` from a catch, with a finally
function stop() { }
function a(): mixed { try { throw new Exception; } catch (Exception $e) { return null; } finally { } }
function b(): mixed { try { throw new Exception; } catch (Exception $e) { return null; } finally { stop(); } }
function d(): mixed { try { return null; } finally { echo ""; } }
function e(): ?int { try { throw new Exception; } catch (Exception $e) { return null; } finally { stop(); } }
function f(): mixed { try { throw new Exception; } catch (Exception $e) { $x = null; return $x; } finally { echo ""; } }
function g(): ?int { try { try { return null; } finally { stop(); } } finally { stop(); } }
// falling off the end is still "none returned"
function h(): mixed { try { stop(); } finally { stop(); } }
function i(): ?int { try { if (false) return 1; } finally { } }
function j(): mixed { try { throw new Exception; } catch (Exception $e) { } finally { stop(); } }
foreach (['a', 'b', 'd', 'e', 'f', 'g', 'h', 'i', 'j'] as $fn) {
    try { var_dump($fn()); } catch (TypeError $t) { echo $t->getMessage(), ' @', $t->getLine(), "\n"; }
}

?>
--EXPECT--
NULL
NULL
NULL
NULL
NULL
NULL
h(): Return value must be of type mixed, none returned @11
i(): Return value must be of type ?int, none returned @12
j(): Return value must be of type mixed, none returned @13
