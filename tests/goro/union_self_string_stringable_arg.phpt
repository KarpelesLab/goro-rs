--TEST--
A Stringable object passed to a self|string parameter from another class's scope is not converted to string
--FILE--
<?php
class Node {
    public array $children = [];
    public function __construct(public string $v) {}
    public function add(self|string|callable $n): void {
        var_dump(get_debug_type($n));
        $this->children[] = $n;
    }
    public function __toString(): string { return $this->v; }
}
class Other {
    function run() {
        $root = new Node('root');
        $c = new Node('child');
        $root->add($c);
        $root->add('str');
        var_dump($root->children[0] === $c);
    }
}
(new Other)->run();

?>
--EXPECT--
string(4) "Node"
string(6) "string"
bool(true)
