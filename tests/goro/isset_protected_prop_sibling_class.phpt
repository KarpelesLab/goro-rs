--TEST--
isset()/?? on a protected property of a sibling subclass object (scope relative to the declaring class) (symfony/config)
--FILE--
<?php
// symfony/config's ArrayNodeDefinition::append(): $this->children[$node->name ?? ''] = ...
// (a protected property of NodeDefinition read on a sibling subclass's object)
abstract class NodeDef {
    protected ?string $name;
    protected $plain;
    protected ?NodeDef $parent = null;
    protected static $count = 0;
    public function __construct(?string $name) { $this->name = $name; $this->plain = $name; }
    protected function prot() { return "prot({$this->name})"; }
}
class ArrayDef extends NodeDef {
    public array $children = [];
    public function append(NodeDef $n) {
        var_dump($n->name ?? 'dflt', isset($n->name), $n->plain ?? 'dflt2', isset($n->plain), empty($n->name));
        $this->children[$n->name ?? ''] = $n;
        $n->parent ??= $this;
        $n->plain = strtoupper($n->plain);
        var_dump($n->prot(), $n->plain, $n->parent === $this);
        unset($n->plain);
        var_dump(isset($n->plain));
    }
}
class ScalarDef extends NodeDef {}
class Redecl extends NodeDef { protected ?string $name = 'r'; }
$a = new ArrayDef('root');
$a->append(new ScalarDef('a'));
$a->append(new ScalarDef('b'));
$a->append(new Redecl('c'));
var_dump(array_keys($a->children));
class Other { function peek(NodeDef $n) { return isset($n->name); } }
var_dump((new Other)->peek(new ScalarDef('x')));

?>
--EXPECT--
string(1) "a"
bool(true)
string(1) "a"
bool(true)
bool(false)
string(7) "prot(a)"
string(1) "A"
bool(true)
bool(false)
string(1) "b"
bool(true)
string(1) "b"
bool(true)
bool(false)
string(7) "prot(b)"
string(1) "B"
bool(true)
bool(false)
string(1) "c"
bool(true)
string(1) "c"
bool(true)
bool(false)
string(7) "prot(c)"
string(1) "C"
bool(true)
bool(false)
array(3) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
  [2]=>
  string(1) "c"
}
bool(false)
