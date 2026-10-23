--TEST--
An output handler is not called while an exception is pending
--FILE--
<?php
// With an exception pending (thrown by an inner handler), an outer user
// output handler is not called: its buffer passes through.
function cb_throw($b, $p) { if ($p & PHP_OUTPUT_HANDLER_FINAL) throw new Exception('cbx'); return $b; }
ob_start(fn($b) => strrev($b), 1, PHP_OUTPUT_HANDLER_CLEANABLE);
ob_start('cb_throw');
echo "abc\n";
try { ob_end_flush(); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo "after\n";
--EXPECT--
abc
Exception: cbx
after
