--TEST--
A Closure over a protected/private method is called by builtins and __invoke() whatever the calling scope (no __call() fallback)
--FILE--
<?php
class G {
    public function __call($m, $a) { return "__call $m"; }
    protected function w($v, $k = 0) { return "w$v"; }
    private function p($v, $k = 0) { return "p$v"; }
    function get() { return $this->w(...); }
    function getP() { return $this->p(...); }
    function getFc() { return Closure::fromCallable([$this, 'w']); }
}
class H {
    protected function w($v, $k = 0) { return "Hw$v"; }
    function get() { return $this->w(...); }
}
class Other {
    function run($f) { return [array_map($f, [1])[0], array_map($f, [1], [2])[0], call_user_func($f, 1)]; }
}
foreach ([(new G)->get(), (new G)->getP(), (new G)->getFc(), (new H)->get()] as $f) {
    var_dump($f(1));
    var_dump(call_user_func($f, 2));
    var_dump(call_user_func_array($f, [3]));
    var_dump(array_map($f, [4]));
    var_dump($f->__invoke(5));
    var_dump((new Other)->run($f));
    $a = [6];
    var_dump(array_walk($a, $f));
    $a = [[7]];
    var_dump(array_walk_recursive($a, $f));
    $a = [2, 1];
    usort($a, fn ($x, $y) => strcmp($f($x), $f($y)));
    var_dump($a);
}

?>
--EXPECT--
string(2) "w1"
string(2) "w2"
string(2) "w3"
array(1) {
  [0]=>
  string(2) "w4"
}
string(2) "w5"
array(3) {
  [0]=>
  string(2) "w1"
  [1]=>
  string(2) "w1"
  [2]=>
  string(2) "w1"
}
bool(true)
bool(true)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
string(2) "p1"
string(2) "p2"
string(2) "p3"
array(1) {
  [0]=>
  string(2) "p4"
}
string(2) "p5"
array(3) {
  [0]=>
  string(2) "p1"
  [1]=>
  string(2) "p1"
  [2]=>
  string(2) "p1"
}
bool(true)
bool(true)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
string(2) "w1"
string(2) "w2"
string(2) "w3"
array(1) {
  [0]=>
  string(2) "w4"
}
string(2) "w5"
array(3) {
  [0]=>
  string(2) "w1"
  [1]=>
  string(2) "w1"
  [2]=>
  string(2) "w1"
}
bool(true)
bool(true)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
string(3) "Hw1"
string(3) "Hw2"
string(3) "Hw3"
array(1) {
  [0]=>
  string(3) "Hw4"
}
string(3) "Hw5"
array(3) {
  [0]=>
  string(3) "Hw1"
  [1]=>
  string(3) "Hw1"
  [2]=>
  string(3) "Hw1"
}
bool(true)
bool(true)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
