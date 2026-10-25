--TEST--
stream contexts: params "options" are merged, integer option keys skipped, closed streams rejected
--FILE--
<?php
$c = stream_context_create(['http' => ['method' => 'POST']]);
var_dump(stream_context_set_params($c, ['options' => ['ftp' => ['overwrite' => true], 'http' => ['header' => 'X: y']]]));
var_dump(stream_context_get_params($c));
var_dump(stream_context_get_options(stream_context_create(['x' => [2 => 3], 'y' => [], 'z' => ['a' => 1, 5 => 6]])));
$a = ['k' => 'v'];
$ref = &$a;
var_dump(stream_context_get_options(stream_context_create(['w' => $ref])));
try {
    stream_context_create([], ['notification' => 'no_such_function']);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
try {
    stream_context_set_params($c, ['notification' => 'no_such_function']);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
$f = fopen(__FILE__, 'r');
var_dump(stream_context_get_options($f));
fclose($f);
try {
    stream_context_get_options($f);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
?>
--EXPECT--
bool(true)
array(1) {
  ["options"]=>
  array(2) {
    ["http"]=>
    array(2) {
      ["method"]=>
      string(4) "POST"
      ["header"]=>
      string(4) "X: y"
    }
    ["ftp"]=>
    array(1) {
      ["overwrite"]=>
      bool(true)
    }
  }
}
array(1) {
  ["z"]=>
  array(1) {
    ["a"]=>
    int(1)
  }
}
array(1) {
  ["w"]=>
  array(1) {
    ["k"]=>
    string(1) "v"
  }
}
stream_context_create(): Argument #1 ($options) must be an array with valid callbacks as values, function "no_such_function" not found or invalid function name
stream_context_set_params(): Argument #1 ($context) must be an array with valid callbacks as values, function "no_such_function" not found or invalid function name
array(0) {
}
stream_context_get_options(): Argument #1 ($stream_or_context) must be a valid stream/context
