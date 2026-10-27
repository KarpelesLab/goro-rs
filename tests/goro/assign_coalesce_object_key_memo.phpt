--TEST--
$a[State::Host->value] ??= v: the memoized enum case (an object) the key reads a property of is evaluated once and read again
--FILE--
<?php
enum State: string { case Host = 'host'; case Port = 'port'; }
class P {
    const K = 'k';
    private array $components = [];
    public function f() {
        $this->components[State::Host->value] ??= '';
        $this->components[State::Port->value] ??= 'p';
        $this->components[self::K] ??= 'kk';
        return $this->components;
    }
}
var_dump((new P)->f());
$a = [];
$a[State::Host->value] ??= 'x';
$a[State::Host->value] ??= 'y';
var_dump($a);
$o = new stdClass;
$o->{State::Port->value} ??= 'z';
var_dump($o);

?>
--EXPECTF--
array(3) {
  ["host"]=>
  string(0) ""
  ["port"]=>
  string(1) "p"
  ["k"]=>
  string(2) "kk"
}
array(1) {
  ["host"]=>
  string(1) "x"
}
object(stdClass)#1 (1) {
  ["port"]=>
  string(1) "z"
}
