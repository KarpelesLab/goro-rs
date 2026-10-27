--TEST--
Objects made by ReflectionClass::newInstanceWithoutConstructor() are destructed
--FILE--
<?php
class P {
    public function __construct(public $n) {}
    public function __destruct() { echo "dtor {$this->n}\n"; }
}
$o = new P(1);
$r = new ReflectionObject($o);
unset($o);
$o2 = new P(2);
$r2 = new ReflectionProperty($o2, 'n');
$m = new ReflectionMethod($o2, '__construct');
unset($o2);
$o3 = new P(3);
$c = Closure::bind(function () {}, $o3, P::class);
unset($o3);
$o4 = (new ReflectionClass('P'))->newInstanceWithoutConstructor();
$o4->n = 4;
$ro4 = new ReflectionObject($o4);
$ro4->getConstructor()->invokeArgs($o4, [44]);
unset($o4);
echo "end\n";

?>
--EXPECT--
dtor 2
end
dtor 44
dtor 3
dtor 1
