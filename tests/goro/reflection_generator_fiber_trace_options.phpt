--TEST--
ReflectionGenerator/ReflectionFiber getTrace() options; closed generator getThis()/getExecutingGenerator()
--FILE--
<?php
function g($x) { yield 1; yield 2; }
function outer($a) { yield from g($a); }
$gen = outer(5);
$gen->current();
$r = new ReflectionGenerator($gen);
var_dump($r->getTrace(DEBUG_BACKTRACE_IGNORE_ARGS));
var_dump($r->getTrace());
foreach ($gen as $v) {}
try { $r->getThis(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->getExecutingGenerator(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->getTrace(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->getExecutingLine(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r->getFunction(); echo "fn ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function fb($q) { Fiber::suspend(1); }
$f = new Fiber(function($z) { fb($z); });
$f->start(3);
$rf = new ReflectionFiber($f);
var_dump($rf->getTrace(DEBUG_BACKTRACE_IGNORE_ARGS));
--EXPECTF--
array(2) {
  [0]=>
  array(3) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(3)
    ["function"]=>
    string(1) "g"
  }
  [1]=>
  array(1) {
    ["function"]=>
    string(5) "outer"
  }
}
array(2) {
  [0]=>
  array(4) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(3)
    ["function"]=>
    string(1) "g"
    ["args"]=>
    array(1) {
      [0]=>
      int(5)
    }
  }
  [1]=>
  array(2) {
    ["function"]=>
    string(5) "outer"
    ["args"]=>
    array(1) {
      [0]=>
      int(5)
    }
  }
}
ReflectionException: Cannot fetch information from a closed Generator
ReflectionException: Cannot fetch information from a closed Generator
ReflectionException: Cannot fetch information from a closed Generator
ReflectionException: Cannot fetch information from a closed Generator
fn ok
array(3) {
  [0]=>
  array(5) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(15)
    ["function"]=>
    string(7) "suspend"
    ["class"]=>
    string(5) "Fiber"
    ["type"]=>
    string(2) "::"
  }
  [1]=>
  array(3) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(16)
    ["function"]=>
    string(2) "fb"
  }
  [2]=>
  array(1) {
    ["function"]=>
    string(%d) "{closure:%s:16}"
  }
}
