--TEST--
An inherited __toString() run by a string conversion sees its declaring class's private properties
--FILE--
<?php
class Node
{
    private ?string $documentation = null;
    private $p = 1;
    public function __toString(): string
    {
        var_dump($this->documentation, $this->p, self::class, static::class);
        return "ok";
    }
}
class TextNode extends Node
{
    private $p = 2;
}
trait T {
    private $q = 'q';
    public function __toString(): string { return $this->q . self::class . static::class; }
}
class A { use T; }
class B extends A { private $q = 'B'; }

$n = new TextNode;
echo $n, "\n";
echo (string)$n, "\n";
echo "x" . $n, "\n";
echo new B, "\n";
echo strlen(new B), "\n";

?>
--EXPECT--
NULL
int(1)
string(4) "Node"
string(8) "TextNode"
ok
NULL
int(1)
string(4) "Node"
string(8) "TextNode"
ok
NULL
int(1)
string(4) "Node"
string(8) "TextNode"
xok
qAB
3
