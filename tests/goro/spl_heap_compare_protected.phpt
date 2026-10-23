--TEST--
SplHeap::compare() is protected
--FILE--
<?php
foreach ([new SplMinHeap, new SplMaxHeap] as $h) {
try { var_dump($h->compare(1,2)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
class Mi extends SplMinHeap{}
try { var_dump((new Mi)->compare(1,2)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class X { function f($h) { return $h->compare(1,2); } }
try { var_dump((new X)->f(new Mi)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Mi2 extends SplMinHeap{ function f() { return $this->compare(1,2); } }
var_dump((new Mi2)->f());
class Mi3 extends SplMinHeap{ protected function compare($a, $b): int { return parent::compare($a, $b); } function f() { return $this->compare(1,2); } }
var_dump((new Mi3)->f());
var_dump((new SplPriorityQueue)->compare(1,2));
try { var_dump(call_user_func([new SplMinHeap, 'compare'], 1, 2)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(is_callable([new SplMinHeap, 'compare']));
--EXPECT--
Call to protected method SplMinHeap::compare() from global scope
Call to protected method SplMaxHeap::compare() from global scope
Call to protected method SplMinHeap::compare() from global scope
Call to protected method SplMinHeap::compare() from scope X
int(1)
int(1)
int(-1)
call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access protected method SplMinHeap::compare()
bool(false)
