--TEST--
get_object_vars()/json_encode()/var_export() read an uninitialized set-hooked property silently as NULL
--FILE--
<?php
class H {
    public int $plain;
    public array|string $wide { set(array|string|int $v) { $this->wide = (array)$v; } }
    public int $vs { set { echo "set\n"; } }
    private int $priv { set { $this->priv = $value; } }
    function dump() { var_dump(get_object_vars($this)); }
}
$h = new H;
var_dump(get_object_vars($h));
echo json_encode($h), "\n";
var_export($h); echo "\n";
foreach ($h as $k => $v) { echo "$k\n"; }
var_dump((array)$h);
$h->dump();
$h->wide = 1;
var_dump(get_object_vars($h));
--EXPECT--
array(1) {
  ["wide"]=>
  NULL
}
{"wide":null}
\H::__set_state(array(
   'wide' => NULL,
   'priv' => NULL,
))
array(0) {
}
array(2) {
  ["wide"]=>
  NULL
  ["priv"]=>
  NULL
}
array(1) {
  ["wide"]=>
  array(1) {
    [0]=>
    int(1)
  }
}
