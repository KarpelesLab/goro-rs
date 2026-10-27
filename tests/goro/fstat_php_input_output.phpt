--TEST--
fstat() of php://input and php://output fails (no stat operation)
--FILE--
<?php
var_dump(fstat(fopen('php://input', 'r')));
var_dump(fstat(fopen('php://output', 'w')));
var_dump(fstat(fopen('php://memory', 'r'))['size']);
var_dump(stream_get_meta_data(fopen('php://input', 'r'))['stream_type']);

?>
--EXPECT--
bool(false)
bool(false)
int(0)
string(5) "Input"
