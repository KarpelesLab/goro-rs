--TEST--
stream_wrapper_register() allocates a "stream factory" resource (freed again when the registration fails)
--FILE--
<?php
class RO { public $context; }
var_dump(stream_wrapper_register("ro", "RO"));
var_dump(@stream_wrapper_register("ro", "RO"));
var_dump(get_resources());
$h = fopen("php://memory", "r");
var_dump((int)$h);
?>
--EXPECT--
bool(true)
bool(false)
array(4) {
  [1]=>
  resource(1) of type (stream)
  [2]=>
  resource(2) of type (stream)
  [3]=>
  resource(3) of type (stream)
  [4]=>
  resource(4) of type (stream factory)
}
int(7)
