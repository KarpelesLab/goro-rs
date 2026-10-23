--TEST--
Special builtin calls in namespaces: a namespaced function of the same name wins
--FILE--
<?php
namespace {
    function show(...$v) { echo json_encode($v), "\n"; }
}
namespace Foo {
    function is_array($x) { return 'mine'; }
    \show(is_array([1]), \is_array([1]), count([1, 2]), strlen('abc'), is_string('x'));
    for ($i = 0; $i < 3; $i++) { \show(is_array([$i]), is_int($i)); }
}
namespace Bar {
    \show(is_array([1]), count([1]), array_key_exists('a', ['a' => 1]));
}
?>
--EXPECT--
["mine",true,2,3,true]
["mine",true]
["mine",true]
["mine",true]
[true,1,true]
