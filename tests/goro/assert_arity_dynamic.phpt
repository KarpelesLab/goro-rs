--TEST--
assert() checks its argument count (1 to 2) like zpp, also when called dynamically
--FILE--
<?php
try { assert(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { assert(true, "a", 3); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { assert(false, "a", 3); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = 'assert';
try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(1, 2, 3); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(0, 2, 3); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { call_user_func('assert'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { assert(description: "x"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(description: "x"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(1, foo: "x"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(1, []); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
ini_set('zend.assertions', 0);
try { $f(1, 2, 3); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo "end\n";
--EXPECT--
ArgumentCountError: assert() expects at least 1 argument, 0 given
ArgumentCountError: assert() expects at most 2 arguments, 3 given
ArgumentCountError: assert() expects at most 2 arguments, 3 given
ArgumentCountError: assert() expects at least 1 argument, 0 given
ArgumentCountError: assert() expects at most 2 arguments, 3 given
ArgumentCountError: assert() expects at most 2 arguments, 3 given
ArgumentCountError: assert() expects at least 1 argument, 0 given
Error: Named parameter $description overwrites previous argument
ArgumentCountError: assert(): Argument #1 ($assertion) not passed
Error: Unknown named parameter $foo
TypeError: assert(): Argument #2 ($description) must be of type Throwable|string|null, array given
end
