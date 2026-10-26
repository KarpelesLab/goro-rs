--TEST--
Compound assignments to properties and elements of a call's result: values and release order
--FILE--
<?php
class D {
    function __construct(public $n) {}
    function __toString(): string { return "<{$this->n}>"; }
    function __destruct() { echo "destroy {$this->n}\n"; }
}
function num($n) { return $n; }
function str($s) { return $s; }
function obj($n) { return new D($n); }
function arr() { return [1, 2]; }
class A {
    public $n = 0;
    public $s = '';
    public $a = ['k' => 1, 'l' => []];
    function run() {
        for ($i = 0; $i < 3; $i++) {
            $this->n += num($i);
            $this->n *= num(2);
            $this->s .= str("s$i");
            $this->a['k'] += num(10);
            $this->a['l'] += arr();
        }
        echo $this->n, ' ', $this->s, ' ', json_encode($this->a), "\n";
        $this->s .= obj(1);
        echo "after concat\n";
        $local = ['x' => 1];
        for ($i = 0; $i < 2; $i++) {
            $local['x'] += strlen(str('abc'));
            $local['y'] = ($local['y'] ?? '') . str('q');
        }
        echo json_encode($local), "\n";
        try {
            $this->n += obj(2);
        } catch (TypeError $e) {
            echo $e->getMessage(), "\n";
        }
        echo $this->s, "\n";
    }
}
(new A)->run();
echo "done\n";
?>
--EXPECT--
8 s0s1s2 {"k":31,"l":[1,2]}
destroy 1
after concat
{"x":7,"y":"qq"}
destroy 2
Unsupported operand types: int + D
s0s1s2<1>
done
