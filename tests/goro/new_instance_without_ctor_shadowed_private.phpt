--TEST--
newInstanceWithoutConstructor() gives the property slots new does (redeclared privates)
--FILE--
<?php
const N = 5;
class G {
    private int $s = 1;
    protected array $c = [N];
    public function gs() { return $this->s; }
}
class M extends G {
    private int $s = 2;
    private string $name;
    public function ms() { return $this->s; }
}
$o = (new ReflectionClass(M::class))->newInstanceWithoutConstructor();
var_dump($o->gs(), $o->ms());
var_dump($o);
var_dump(array_map('bin2hex', array_keys((array) $o)));
$p = new M;
var_dump($p == $o);

?>
--EXPECT--
int(1)
int(2)
object(M)#2 (3) {
  ["s":"G":private]=>
  int(1)
  ["c":protected]=>
  array(1) {
    [0]=>
    int(5)
  }
  ["s":"M":private]=>
  int(2)
  ["name":"M":private]=>
  uninitialized(string)
}
array(3) {
  [0]=>
  string(8) "00470073"
  [1]=>
  string(8) "002a0063"
  [2]=>
  string(8) "004d0073"
}
bool(true)
