<?php
class I implements IteratorAggregate { function getIterator(): Traversable { return new I; } }
try { foreach (new I as $v) {} } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { var_dump(iterator_to_array(new I)); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { (function () { yield from new I; })()->current(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
echo "done\n";
