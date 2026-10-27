--TEST--
Typed variadic parameters: arguments of their types, coerced ones, wrong ones
--FILE--
<?php
class TI {
    private array $toks = [[1, 'a'], [2, 'b'], [3, 'c']];
    private int $i = 0;
    public function is(int ...$types): bool { return in_array($this->toks[$this->i][0], $types, true); }
    public function next(): void { $this->i = ($this->i + 1) % 3; }
    public function names(string $sep, string ...$names): string { return implode($sep, $names); }
}
function sum(int|float ...$n): int|float { return array_sum($n); }
function objs(Countable ...$c): int { $t = 0; foreach ($c as $x) { $t += count($x); } return $t; }
function run() {
    $t = new TI;
    $hits = 0;
    for ($k = 0; $k < 30; $k++) {
        if ($t->is(1, 3)) {
            $hits++;
        }
        if ($t->is()) {
            $hits += 100;
        }
        $t->next();
    }
    echo $hits, "\n";
    echo $t->names(",", "x", "y", "z"), "|", $t->names("-"), "\n";
    echo sum(1, 2, 3.5), " ", sum(), " ", sum("4", 5), "\n";
    echo objs(new ArrayObject([1, 2]), new ArrayObject([3])), "\n";
    try {
        $t->is(1, "two");
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    try {
        objs(new ArrayObject([]), new stdClass);
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    echo $t->names(",", 1, 2.5), "\n";
}
run();
?>
--EXPECTF--
20
x,y,z|
6.5 0 9
3
TI::is(): Argument #2 must be of type int, string given, called in %s on line 28
objs(): Argument #2 must be of type Countable, stdClass given, called in %s on line 33
1,2.5
