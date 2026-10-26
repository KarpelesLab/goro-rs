--TEST--
SPL: iterators created by getChildren() run their constructor from an internal frame
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
--FILE--
<?php
class F extends RecursiveFilterIterator { function accept(): bool { return true; } }
class G extends RecursiveFilterIterator { function __construct($it) { echo "G ctor\n"; parent::__construct($it); } function accept(): bool { return true; } }
$f = new F(new RecursiveArrayIterator([1, 'a']));
try { $f->getChildren(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$g = new G(new RecursiveArrayIterator([1]));
try { $g->getChildren(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$c = new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([1]), fn() => true);
try { $c->getChildren(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$p = new ParentIterator(new RecursiveArrayIterator([1]));
try { $p->getChildren(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$r = new RecursiveCachingIterator(new RecursiveArrayIterator([1]));
try { $r->getChildren(); var_dump($r->getChildren()); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$i = new RecursiveRegexIterator(new RecursiveArrayIterator(["ab", ["ac"]]), "/^a/"); try { $i->getChildren(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
?>
--EXPECTF--
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(5): RecursiveFilterIterator->getChildren()
#3 {main}
G ctor
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(7): RecursiveFilterIterator->getChildren()
#3 {main}
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(9): RecursiveCallbackFilterIterator->getChildren()
#3 {main}
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(11): RecursiveFilterIterator->getChildren()
#3 {main}
NULL
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, string given
#0 [internal function]: ArrayIterator->__construct('ab', 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(14): RecursiveRegexIterator->getChildren()
#3 {main}
