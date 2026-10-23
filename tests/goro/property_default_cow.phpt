--TEST--
Array property defaults are shared copy-on-write: writes never reach the default
--FILE--
<?php
class C {
    public $a = [3, 1, 'k' => ['n' => 1]];
    function m() { $this->a[] = 'm'; $r = &$this->a; $r['k']['n'] = 'r'; }
}
$tests = [
    'append' => function ($o) { $o->a[] = 9; },
    'nested' => function ($o) { $o->a['k']['n'] = 9; },
    'unset' => function ($o) { unset($o->a['k']['n']); },
    'sort' => function ($o) { sort($o->a); },
    'push' => function ($o) { array_push($o->a, 9); },
    'end' => function ($o) { end($o->a); },
    'sort_nested' => function ($o) { sort($o->a['k']); },
    'ref' => function ($o) { $r = &$o->a; $r[] = 9; },
    'ref_nested' => function ($o) { $r = &$o->a['k']['n']; $r = 9; },
    'foreach_ref' => function ($o) { foreach ($o->a as &$v) { $v = 9; } },
    'foreach_obj_ref' => function ($o) { foreach ($o as &$v) { $v[] = 9; } },
    'walk' => function ($o) { array_walk($o->a, function (&$v) { $v = 9; }); },
    'walk_obj' => function ($o) { array_walk($o, function (&$v) { $v[] = 9; }); },
    'extract_refs' => function ($o) { extract($o->a, EXTR_REFS); $k['n'] = 9; },
    'preg' => function ($o) { preg_match('/a/', 'a', $o->a['k']); },
    'method' => function ($o) { $o->m(); },
    'clone' => function ($o) { $c = clone $o; $c->a['k']['n'] = 9; },
];
$default = json_encode(new C);
foreach ($tests as $name => $f) {
    $before = new C;
    $o = new C;
    $f($o);
    unset($v);
    $ok = json_encode(new C) === $default && json_encode($before) === $default;
    echo $name, ': ', $ok ? 'ok' : 'BROKEN', ' ', json_encode($o), "\n";
}
?>
--EXPECT--
append: ok {"a":{"0":3,"1":1,"k":{"n":1},"2":9}}
nested: ok {"a":{"0":3,"1":1,"k":{"n":9}}}
unset: ok {"a":{"0":3,"1":1,"k":[]}}
sort: ok {"a":[1,3,{"n":1}]}
push: ok {"a":{"0":3,"1":1,"k":{"n":1},"2":9}}
end: ok {"a":{"0":3,"1":1,"k":{"n":1}}}
sort_nested: ok {"a":{"0":3,"1":1,"k":[1]}}
ref: ok {"a":{"0":3,"1":1,"k":{"n":1},"2":9}}
ref_nested: ok {"a":{"0":3,"1":1,"k":{"n":9}}}
foreach_ref: ok {"a":{"0":9,"1":9,"k":9}}
foreach_obj_ref: ok {"a":{"0":3,"1":1,"k":{"n":1},"2":9}}
walk: ok {"a":{"0":9,"1":9,"k":9}}
walk_obj: ok {"a":{"0":3,"1":1,"k":{"n":1},"2":9}}
extract_refs: ok {"a":{"0":3,"1":1,"k":{"n":9}}}
preg: ok {"a":{"0":3,"1":1,"k":["a"]}}
method: ok {"a":{"0":3,"1":1,"k":{"n":"r"},"2":"m"}}
clone: ok {"a":{"0":3,"1":1,"k":{"n":1}}}
