--TEST--
??= on a nested dimension whose key is an expression writes with the evaluated key when a level is missing
--FILE--
<?php
// symfony DefinitionErrorExceptionPass: $this->sourceReferences[$targetId][$this->currentId ?? ''] ??= true
$id = 'svc';
$a = [];
$a['t'][$id ?? ''] ??= true;
var_dump($a);
$o = new stdClass; $o->id = 'pp';
$b = [];
$b['t'][$o->id ?? ''] ??= true;
$b[$o->id ?? 'q'] ??= 1;
$b[$id ?: 'z'] ??= 2;
$b[$o->id . 'x'] ??= 3;
var_dump($b);

?>
--EXPECT--
array(1) {
  ["t"]=>
  array(1) {
    ["svc"]=>
    bool(true)
  }
}
array(4) {
  ["t"]=>
  array(1) {
    ["pp"]=>
    bool(true)
  }
  ["pp"]=>
  int(1)
  ["svc"]=>
  int(2)
  ["ppx"]=>
  int(3)
}
