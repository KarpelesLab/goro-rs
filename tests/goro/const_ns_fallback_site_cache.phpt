--TEST--
Namespaced constant fetches: a site that fell back to the global constant keeps it (runtime cache), qualified names
--FILE--
<?php
namespace NS;

const OWN = 'own';
define('GLOB', 'global');

function f() { return GLOB; }
function g() { return OWN . '/' . \NS\OWN . '/' . namespace\OWN . '/' . \PHP_INT_SIZE; }
function h() { return defined('NS\LATE') ? LATE : 'none'; }
function u() { return UNDEF; }

for ($i = 0; $i < 3; $i++) {
    echo f(), ' ', g(), ' ', h(), "\n";
}
define('NS\GLOB', 'namespaced');
define('NS\LATE', 'late');
for ($i = 0; $i < 3; $i++) {
    echo f(), ' ', g(), ' ', h(), "\n";
}
for ($i = 0; $i < 2; $i++) {
    try {
        u();
    } catch (\Error $e) {
        echo $e->getMessage(), "\n";
    }
    if ($i == 0) {
        define('UNDEF', 'now defined');
        echo u(), "\n";
    }
}
?>
--EXPECT--
global own/own/own/8 none
global own/own/own/8 none
global own/own/own/8 none
global own/own/own/8 late
global own/own/own/8 late
global own/own/own/8 late
Undefined constant "NS\UNDEF"
now defined
