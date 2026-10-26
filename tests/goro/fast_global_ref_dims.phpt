--TEST--
Fast nested writes into a global's array and isset() of a null global from lean callees
--FILE--
<?php
function add_support($post_type, $feature, ...$args) {
    global $features;
    foreach ((array) $feature as $f) {
        $features[$post_type][$f] = $args ? $args : true;
    }
}
function is_rest() { global $rest_server; return isset($rest_server) && $rest_server; }
function byref_write(&$a, $k) { $a[$k]['x'] = 1; return count($a); }
function shared_write() { global $g2; $copy = $g2; $g2['a']['b'] = 2; return [$g2, $copy]; }
$rest_server = null;
$g2 = ['a' => ['b' => 1]];
for ($i = 0; $i < 40; $i++) {
    add_support('post', ['title', 'editor']);
    add_support("t$i", 'thumb', 1);
    $r = [is_rest()];
    $arr = ['k' => ['y' => 0]];
    $keep = $arr;
    $r[] = byref_write($arr, 'k');
    $r[] = [$arr, $keep];
    $r[] = shared_write();
}
echo json_encode([$features['post'], $features['t39'], count($features), $r]), "\n";
$rest_server = 1;
var_dump(is_rest());
--EXPECT--
[{"title":true,"editor":true},{"thumb":[1]},41,[false,1,[{"k":{"y":0,"x":1}},{"k":{"y":0}}],[{"a":{"b":2}},{"a":{"b":2}}]]]
bool(true)
