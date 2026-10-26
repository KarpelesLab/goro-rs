--TEST--
isset($$name) / empty($$name) with $name "this" look in the symbol table, which has no $this
--FILE--
<?php
class A { function m() { $n = 'this'; var_dump(isset($$n), empty($$n), $$n ?? 'dflt', isset(${'this'}), isset($this)); var_dump(isset($$n->p)); $this->p = 1; var_dump(isset($$n->p)); } }
(new A)->m();
?>
--EXPECTF--
bool(false)
bool(true)
object(A)#1 (0) {
}
bool(true)
bool(true)
bool(false)

Deprecated: Creation of dynamic property A::$p is deprecated in %s on line 2
bool(true)
