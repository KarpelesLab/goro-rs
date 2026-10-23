--TEST--
fprintf()/vfprintf() take a resource, not an int
--FILE--
<?php
try { fprintf(5, "x\n"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vfprintf(5, "x\n", []); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { fprintf("a", "x\n"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { fprintf(1.5, "x\n"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { fprintf(new stdClass, "x\n"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo fprintf(STDOUT, "ok %d\n", 5), "\n";
echo vfprintf(STDOUT, "ok %d\n", [6]), "\n";
--EXPECT--
TypeError: fprintf(): Argument #1 ($stream) must be of type resource, int given
TypeError: vfprintf(): Argument #1 ($stream) must be of type resource, int given
TypeError: fprintf(): Argument #1 ($stream) must be of type resource, string given
TypeError: fprintf(): Argument #1 ($stream) must be of type resource, float given
TypeError: fprintf(): Argument #1 ($stream) must be of type resource, stdClass given
ok 5
5
ok 6
5
