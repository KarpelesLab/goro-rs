--TEST--
Exception::__toString() / uncaught report stop at a previous exception met again; uncaught trace warnings come from no active file
--FILE--
<?php
$p = new Exception("p");
$q = new Exception("q", 0, $p);
$r = new ReflectionProperty(Exception::class, 'previous');
$r->setValue($p, $q);
echo $q, "\n---\n";
$e = new Exception("self");
$r->setValue($e, $e);
echo $e, "\n---\n";
$t = new Exception("t", 0, $p);
(new ReflectionProperty(Exception::class, 'trace'))->setValue($t, [['function' => 'f'], 'junk']);
echo $t->getTraceAsString(), "\n---\n";
throw $t;
--EXPECTF--
Exception: p in %s:2
Stack trace:
#0 {main}

Next Exception: q in %s:3
Stack trace:
#0 {main}
---
Exception: self in %s:7
Stack trace:
#0 {main}
---

Warning: Expected array for frame 1 in %s on line 12
#0 [internal function]: f()
#1 {main}
---

Warning: Expected array for frame 1 in Unknown on line 0

Fatal error: Uncaught Exception: q in %s:3
Stack trace:
#0 {main}

Next Exception: p in %s:2
Stack trace:
#0 {main}

Next Exception: t in %s:10
Stack trace:
#0 [internal function]: f()
#1 {main}
  thrown in %s on line 10
