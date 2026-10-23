--TEST--
Nested Serializable::serialize() calls leave no frames behind
--FILE--
<?php
class In implements Serializable {
    function serialize() { debug_print_backtrace(); return "x"; }
    function unserialize($s) { }
}
class Out implements Serializable {
    public $in;
    function __construct() { $this->in = new In; }
    function serialize() { $r = serialize($this->in); echo "--after inner\n"; debug_print_backtrace(); return $r; }
    function unserialize($s) { }
}
$s = serialize(new Out);
echo "--top\n";
debug_print_backtrace();
--EXPECTF--

Deprecated: In implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in %s on line 2

Deprecated: Out implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in %s on line 6
#0 [internal function]: In->serialize()
#1 %s(9): serialize(Object(In))
#2 [internal function]: Out->serialize()
#3 %s(12): serialize(Object(Out))
--after inner
#0 [internal function]: Out->serialize()
#1 %s(12): serialize(Object(Out))
--top
