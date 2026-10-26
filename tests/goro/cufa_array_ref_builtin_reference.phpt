--TEST--
call_user_func_array() passes references to the array-modifying builtins
--FILE--
<?php
function f(&$a){$a=1;}
$x=0; call_user_func_array("f", [&$x]); var_dump($x);
$m=null; call_user_func_array("preg_match", ["/a/", "a", &$m]); var_dump($m);
$arr=[3,1,2]; call_user_func_array("rsort", [&$arr]); var_dump($arr[0]);
$arr=[3,1,2]; call_user_func_array("shuffle", [&$arr]); var_dump(count($arr));
$arr=[3,1,2]; call_user_func_array("array_push", [&$arr, 5]); var_dump(count($arr));
$arr=[3,1,2]; call_user_func_array("sort", [&$arr]); var_dump($arr[0]);
--EXPECT--
int(1)
array(1) {
  [0]=>
  string(1) "a"
}
int(3)
int(3)
int(4)
int(1)
