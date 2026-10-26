--TEST--
Assigning a hooked property by reference: indirect modification, write-only
--FILE--
<?php
class A {
    public $x { get => 1; set {} }
    public $y { set {} }
    private $s = 2;
    public $z { &get => $this->s; }
    public $t { get { throw new Exception("from get"); } }
}
$a = new A;
$v = 1;
foreach (['x', 'y', 'z', 't'] as $p) {
    try {
        $a->$p = &$v;
        echo "$p: assigned\n";
    } catch (Throwable $e) {
        echo "$p: ", get_class($e), ": ", $e->getMessage();
        if ($prev = $e->getPrevious()) {
            echo " (previous: ", $prev->getMessage(), ")";
        }
        echo "\n";
    }
}
--EXPECT--
x: Error: Cannot assign by reference to overloaded object (previous: Indirect modification of A::$x is not allowed)
y: Error: Property A::$y is write-only
z: Error: Cannot assign by reference to overloaded object
t: Exception: from get
