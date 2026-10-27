--TEST--
Exception::getMessage() / getFile() / getLine() cast a property not of their type
--FILE--
<?php
class E extends Exception {
    public function __construct() {
        parent::__construct('');
        $this->message = new class { public function __toString(): string { return "lazy message"; } };
        $this->file = new class { public function __toString(): string { return "lazy file"; } };
        $this->line = "42";
    }
}
$e = new E;
var_dump($e->getMessage(), $e->getFile(), $e->getLine());
var_dump(call_user_func([$e, 'getMessage']));
var_dump(array_map(fn($x) => $x->getMessage(), [$e]));
$e2 = new Exception('plain', 3);
var_dump($e2->getMessage(), $e2->getCode());

?>
--EXPECT--
string(12) "lazy message"
string(9) "lazy file"
int(42)
string(12) "lazy message"
array(1) {
  [0]=>
  string(12) "lazy message"
}
string(5) "plain"
int(3)
