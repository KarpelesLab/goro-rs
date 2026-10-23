--TEST--
parent:: calls of Exception's internal methods from a subclass
--FILE--
<?php
// A subclass method calling Exception's / Error's internal methods through
// parent:: (or the class name).
class E extends Exception {
    function m() { return parent::getMessage() . "|" . parent::getCode() . "|" . basename(parent::getFile()) . "|" . parent::getLine(); }
    function p() { return get_class(parent::getPrevious()) . "|" . count(parent::getTrace()) . "|" . Exception::getMessage(); }
    function __toString(): string { return "E:" . strtok(parent::__toString(), "\n"); }
    function __wakeup(): void { echo "wakeup\n"; parent::__wakeup(); }
}
$e = new E("hi", 3, new LogicException("prev"));
echo $e->m(), "\n", $e->p(), "\n", $e, "\n";
$u = unserialize(serialize($e));
echo $u->getMessage(), "\n";
class R extends Error { function m() { return parent::getMessage() . parent::getTraceAsString(); } }
echo (new R("err"))->m(), "\n";
class X extends ErrorException { function s() { return parent::getSeverity(); } }
echo (new X("x", 0, E_WARNING))->s(), "\n";
class A extends ArrayObject { function count(): int { return parent::count() + 100; } }
echo count(new A([1, 2])), "\n";
--EXPECTF--
hi|3|parent_call_throwable_internal_methods.php|10
LogicException|0|hi
E:LogicException: prev in %s:10
wakeup
hi
err#0 {main}
2
102
