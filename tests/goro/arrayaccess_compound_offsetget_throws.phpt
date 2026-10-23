--TEST--
Compound assignment to an ArrayAccess element whose offsetGet() throws: Cannot use object as array
--FILE--
<?php
class A implements ArrayAccess {
    public $foo = [0];
    function offsetGet($x): mixed { throw new LogicException("get"); }
    function offsetSet($x, $y): void { echo "set\n"; }
    function offsetExists($x): bool { return true; }
    function offsetUnset($x): void {}
}
$a = new A;
foreach (['+=' => fn() => $a[0] += 1, '.=' => fn() => $a[0] .= 'x', '++' => fn() => $a[0]++, 'read' => fn() => $a[0], '??=' => fn() => $a[0] ??= 1] as $k => $f) {
    try { $f(); } catch (Throwable $e) {
        echo $k, ": ", get_class($e), ": ", $e->getMessage();
        if ($p = $e->getPrevious()) echo " <- ", get_class($p), ": ", $p->getMessage();
        echo "\n";
    }
}
?>
--EXPECT--
+=: Error: Cannot use object of type A as array <- LogicException: get
.=: Error: Cannot use object of type A as array <- LogicException: get
++: LogicException: get
read: LogicException: get
??=: LogicException: get
