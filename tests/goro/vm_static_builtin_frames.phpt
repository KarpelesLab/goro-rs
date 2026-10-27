--TEST--
Internal static methods the VM implements (enum from(), Closure::bind(), ...) have a frame in traces
--FILE--
<?php
enum E: int { case A = 1; }
enum S: string { case A = 'a'; }
function f() { return E::from(2); }
try { f(); } catch (ValueError $e) { echo $e->getTraceAsString(), "\n"; }
try { S::from('zz'); } catch (ValueError $e) { echo $e->getTraceAsString(), "\n"; }
try { E::from('x'); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { E::tryFrom(); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { SplFixedArray::fromArray(['a' => 1]); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { Closure::getCurrent(); } catch (Throwable $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { Closure::bind(1, null); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { Closure::bind(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { Closure::bind(fn() => 1, 5); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { Closure::bind(fn() => 1, null, 'A', 4); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
set_error_handler(function ($n, $s) { echo "EH $s\n"; debug_print_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS); return true; });
Closure::bind(function() {}, null, 'Nope');
E::from(9);
--EXPECTF--
#0 %s(4): E::from(2)
#1 %s(5): f()
#2 {main}
#0 %s(6): S::from('zz')
#1 {main}
E::from(): Argument #1 ($value) must be of type int, string given
#0 %s(7): E::from('x')
#1 {main}
E::tryFrom() expects exactly 1 argument, 0 given
#0 %s(8): E::tryFrom()
#1 {main}
array must contain only positive integer keys
#0 %s(9): SplFixedArray::fromArray(Array)
#1 {main}
Current function is not a closure
#0 %s(10): Closure::getCurrent()
#1 {main}
Closure::bind(): Argument #1 ($closure) must be of type Closure, int given
#0 %s(11): Closure::bind(1, NULL)
#1 {main}
ArgumentCountError: Closure::bind() expects at least 2 arguments, 0 given
TypeError: Closure::bind(): Argument #2 ($newThis) must be of type ?object, int given
ArgumentCountError: Closure::bind() expects at most 3 arguments, 4 given
EH Class "Nope" not found
#0 [internal function]: {closure:%s:15}()
#1 %s(16): Closure::bind()

Fatal error: Uncaught ValueError: 9 is not a valid backing value for enum E in %s:17
Stack trace:
#0 %s(17): E::from(9)
#1 {main}
  thrown in %s on line 17
