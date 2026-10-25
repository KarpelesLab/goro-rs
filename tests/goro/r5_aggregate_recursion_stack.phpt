--TEST--
An endless getIterator() chain runs out of stack
--FILE--
<?php
class I implements IteratorAggregate { function getIterator(): Traversable { return new I; } }
try { foreach (new I as $v) {} } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { var_dump(iterator_to_array(new I)); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { (function () { yield from new I; })()->current(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
echo "done\n";
--EXPECT--
Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
done
