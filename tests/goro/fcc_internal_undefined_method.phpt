--TEST--
First-class callables of undefined methods of internal classes throw
--FILE--
<?php
$r = new ReflectionProperty('ReflectionProperty','name');
try { $f = $r->isWritable(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f = $r->getName(...); var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$r = new ReflectionClass('stdClass');
try { $f = $r->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new ArrayObject([]);
try { $f = $a->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new DateTime();
try { $f = $a->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new Exception();
try { $f = $a->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new SplStack();
try { $f = $a->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$a = new ReflectionFunction('strlen');
try { $f = $a->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f = $a->isWritable(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f = [$a, 'nope'](...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(is_callable([$a, 'nope'])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(method_exists($a, 'nope')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$r = new ReflectionProperty('ReflectionProperty','name');
try { var_dump(is_callable([$r, 'isWritable'])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($r->isWritable()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class MyAO extends ArrayObject {}
$m = new MyAO([1, 2]);
try { $f = $m->nope(...); var_dump($f); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($m->count(...)());
$a = new ArrayObject([1]);
var_dump($a->count(...)());
$d = new DateTime("2020-01-01");
var_dump($d->format(...)("Y"));
$x = new Exception("m");
var_dump($x->getMessage(...)());
--EXPECT--
Error: Call to undefined method ReflectionProperty::isWritable()
string(4) "name"
Error: Call to undefined method ReflectionClass::nope()
Error: Call to undefined method ArrayObject::nope()
Error: Call to undefined method DateTime::nope()
Error: Call to undefined method Exception::nope()
Error: Call to undefined method SplStack::nope()
Error: Call to undefined method ReflectionFunction::nope()
Error: Call to undefined method ReflectionFunction::isWritable()
Error: Call to undefined method ReflectionFunction::nope()
bool(false)
bool(false)
bool(false)
Error: Call to undefined method ReflectionProperty::isWritable()
Error: Call to undefined method MyAO::nope()
int(2)
int(1)
string(4) "2020"
string(1) "m"
