--TEST--
Calls of user methods on objects of user classes deriving from internal ones
--EXTENSIONS--
pdo_sqlite
--FILE--
<?php
class MyPdo extends PDO {
    private $n = 0;
    function run($x) { return $this->step($x) . $this->step($x + 1); }
    private function step($x) { $this->n++; return "[$x:{$this->n}]"; }
    protected function prot() { return $this->getAttribute(PDO::ATTR_DRIVER_NAME); }
    function driver() { return $this->prot(); }
    function trace() { return $this->deep(); }
    private function deep() { return array_map(fn($f) => ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'], debug_backtrace()); }
}
class MyEx extends RuntimeException {
    function describe() { return $this->fmt($this->getMessage()); }
    private function fmt($m) { return "<$m>"; }
}
class MyAo extends ArrayObject {
    function total() { $s = 0; for ($i = 0; $i < 3; $i++) { $s += $this->at($i); } return $s; }
    private function at($i) { return $this[$i] ?? 0; }
}
class NoCtorFile extends SplFileObject {
    function __construct() {}
    function mine() { return 1; }
}
class NoCtorRii extends RecursiveIteratorIterator {
    function __construct() {}
    function mine() { return 2; }
}
function run() {
    $p = new MyPdo('sqlite::memory:');
    for ($i = 0; $i < 3; $i++) { echo $p->run($i), "\n"; }
    echo $p->driver(), "\n";
    print_r($p->trace());
    $e = new MyEx('boom');
    echo $e->describe(), "\n";
    $a = new MyAo([1, 2, 3]);
    echo $a->total(), "\n";
    foreach ([new NoCtorFile, new NoCtorRii] as $o) {
        try { echo $o->mine(), "\n"; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    }
}
run();
?>
--EXPECT--
[0:1][1:2]
[1:3][2:4]
[2:5][3:6]
sqlite
Array
(
    [0] => MyPdo->deep
    [1] => MyPdo->trace
    [2] => run
)
<boom>
6
Error: The parent constructor was not called: the object is in an invalid state
Error: The NoCtorRii instance wasn't initialized properly
