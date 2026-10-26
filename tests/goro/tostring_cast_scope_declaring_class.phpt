--TEST--
(string) runs __toString() in the scope of its declaring class (a subclass declaring its own private property)
--FILE--
<?php
// symfony/dependency-injection: TypedReference extends Reference; (string) $ref runs
// Reference::__toString() which reads Reference's private $id
class Base implements ArrayAccess, IteratorAggregate, Countable, JsonSerializable {
    public function __construct(private string $id) {}
    public function __toString(): string { return "str:" . $this->id; }
    public function __get($n) { return "get:" . $this->id; }
    public function __isset($n) { return $this->id === 'x'; }
    public function __call($m, $a) { return "call:" . $this->id; }
    public function __invoke() { return "invoke:" . $this->id; }
    public function offsetExists($o): bool { return $this->id === 'x'; }
    public function offsetGet($o): mixed { return "off:" . $this->id; }
    public function offsetSet($o, $v): void {}
    public function offsetUnset($o): void {}
    public function getIterator(): Iterator { return new ArrayIterator([$this->id]); }
    public function count(): int { return strlen($this->id); }
    public function jsonSerialize(): mixed { return ['id' => $this->id]; }
    public function __serialize(): array { return ['id' => $this->id]; }
    public function __unserialize(array $d): void { $this->id = $d['id']; }
    public function __destruct() { echo "destruct:", $this->id, "\n"; }
}
class Child extends Base {
    private ?string $name = null;
    public function __construct(string $id) { parent::__construct($id); }
}
$c = new Child('x');
echo (string) $c, "\n", $c . "", "\n";
echo $c->nope, "\n", var_export(isset($c->nope), true), "\n", $c->meth(), "\n", $c(), "\n";
echo $c['k'], "\n", var_export(isset($c['k']), true), "\n";
foreach ($c as $v) echo "iter:$v\n";
echo count($c), "\n", json_encode($c), "\n";
$u = unserialize(serialize($c));
echo (string) $u, "\n";
echo call_user_func($c), "\n", implode(',', array_map($c, [1])), "\n";
echo str_pad($c, 8, '-'), "\n", strlen($c), "\n", in_array('str:x', [$c]) ? 'in' : 'out', "\n";
unset($u);
echo "end\n";

?>
--EXPECT--
str:x
str:x
get:x
true
call:x
invoke:x
off:x
true
iter:x
1
{"id":"x"}
str:x
invoke:x
invoke:x
str:x---
5
in
destruct:x
end
destruct:x
