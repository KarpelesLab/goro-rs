--TEST--
An output handler throwing during the shutdown flush discards the remaining buffers
--FILE--
<?php
// An output handler throwing while the buffers are flushed at shutdown:
// the flush is abandoned and the remaining buffers are discarded.
echo "before\n";
ob_start(function ($b) { return "[outer:$b]"; });
echo "outer\n";
ob_start(function ($b, $p) { if ($p & PHP_OUTPUT_HANDLER_FINAL) throw new Exception("cbx"); return $b; });
echo "inside\n";
--EXPECT--
before
