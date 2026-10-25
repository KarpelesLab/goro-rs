--TEST--
BcMath\Number: virtual properties (foreach, get_mangled_object_vars, ArrayObject)
--FILE--
<?php
$n = new BcMath\Number("1.5");
foreach ($n as $k => $v) { echo "never\n"; }
foreach ($n as $k => &$v) { echo "never\n"; }
var_dump(get_mangled_object_vars($n), (array)$n, get_object_vars($n));
var_dump(array_walk($n, function () { echo "never\n"; }));
foreach (['SimpleXMLElement' => new SimpleXMLElement('<a/>'), 'Number' => $n, 'SplObjectStorage' => new SplObjectStorage, 'DateTime' => new DateTime('2020-01-01')] as $k => $o) {
    try { $a = @new ArrayObject($o); echo "$k: ", count($a), "\n"; } catch (InvalidArgumentException $e) { echo $e->getMessage(), "\n"; }
    try { $a = @new ArrayIterator($o); echo "$k: ", count($a), "\n"; } catch (InvalidArgumentException $e) { echo $e->getMessage(), "\n"; }
}
foreach ((new ReflectionObject($n))->getProperties() as $p) echo $p;
--EXPECT--
array(0) {
}
array(2) {
  ["value"]=>
  string(3) "1.5"
  ["scale"]=>
  int(1)
}
array(2) {
  ["value"]=>
  string(3) "1.5"
  ["scale"]=>
  int(1)
}
bool(true)
Overloaded object of type SimpleXMLElement is not compatible with ArrayObject
Overloaded object of type SimpleXMLElement is not compatible with ArrayIterator
Overloaded object of type BcMath\Number is not compatible with ArrayObject
Overloaded object of type BcMath\Number is not compatible with ArrayIterator
SplObjectStorage: 0
SplObjectStorage: 0
Overloaded object of type DateTime is not compatible with ArrayObject
Overloaded object of type DateTime is not compatible with ArrayIterator
Property [ public protected(set) readonly virtual string $value ]
Property [ public protected(set) readonly virtual int $scale ]
