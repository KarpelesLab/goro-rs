--TEST--
IteratorIterator over an IteratorAggregate whose getIterator() returns another aggregate iterates the innermost iterator
--FILE--
<?php
class It implements Iterator {
    private $i = 0;
    function __construct(private array $a) {}
    public function current(): mixed { echo "current\n"; return $this->a[$this->i]; }
    public function next(): void { echo "next\n"; $this->i++; }
    public function key(): mixed { echo "key\n"; return $this->i; }
    public function valid(): bool { echo "valid\n"; return $this->i < count($this->a); }
    public function rewind(): void { echo "rewind\n"; $this->i = 0; }
}
class Inner implements IteratorAggregate {
    function __construct(public $v) {}
    public function getIterator(): Traversable { echo "Inner::getIterator\n"; return $this->v; }
}
class Outer implements IteratorAggregate {
    function __construct(public $v) {}
    public function getIterator(): Traversable { echo "Outer::getIterator\n"; return $this->v; }
}
$o = new Outer(new Inner(new It(['a', 'b'])));
$ii = new IteratorIterator($o);
echo get_class($ii->getInnerIterator()), "\n";
foreach ($ii as $k => $v) echo "$k => $v\n";
echo "-- nested twice\n";
$ii = new IteratorIterator(new Outer(new Inner(new Inner(new ArrayIterator([1, 2])))));
foreach ($ii as $k => $v) echo "$k => $v\n";
echo "-- ArrayObject\n";
$ii = new IteratorIterator(new Outer(new ArrayObject(['x' => 1, 'y' => 2])));
echo get_class($ii->getInnerIterator()), "\n";
foreach ($ii as $k => $v) echo "$k => $v\n";
echo "-- generator\n";
$ii = new IteratorIterator(new Outer(new Inner((function () { yield 1; yield 2; })())));
foreach ($ii as $k => $v) echo "$k => $v\n";
echo "-- RecursiveIteratorIterator\n";
class RI extends IteratorIterator implements RecursiveIterator {
    function hasChildren(): bool { return false; }
    function getChildren(): ?RecursiveIterator { return null; }
}
var_dump(iterator_to_array(new RecursiveIteratorIterator(new RI(new Outer(new Inner(new ArrayIterator([5, 6])))))));
echo "-- throwing\n";
class Bad implements IteratorAggregate {
    public function getIterator(): Traversable { throw new Exception("boom"); }
}
try {
    new IteratorIterator(new Outer(new Bad));
} catch (Exception $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
class NotTrav implements IteratorAggregate {
    public function getIterator(): Traversable { return new Outer(new stdClass); }
}
try {
    new IteratorIterator(new Outer(new NotTrav));
} catch (Throwable $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}

?>
--EXPECT--
Outer::getIterator
Inner::getIterator
Inner
rewind
valid
current
key
0 => a
next
valid
current
key
1 => b
next
valid
-- nested twice
Outer::getIterator
Inner::getIterator
Inner::getIterator
0 => 1
1 => 2
-- ArrayObject
Outer::getIterator
ArrayObject
x => 1
y => 2
-- generator
Outer::getIterator
Inner::getIterator
0 => 1
1 => 2
-- RecursiveIteratorIterator
Outer::getIterator
Inner::getIterator
array(2) {
  [0]=>
  int(5)
  [1]=>
  int(6)
}
-- throwing
Outer::getIterator
Exception: boom
Outer::getIterator
Outer::getIterator
TypeError: Outer::getIterator(): Return value must be of type Traversable, stdClass returned
