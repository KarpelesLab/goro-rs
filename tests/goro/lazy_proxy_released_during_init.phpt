--TEST--
Lazy proxy released by its factory: in-place operations throw, held objects do not
--FILE--
<?php
class C {
    public $s;
    public function m() { return 1; }
    public function __destruct() { var_dump(__METHOD__); }
}
$r = new ReflectionClass(C::class);
$mkg = function () use ($r) {
    return $r->newLazyProxy(function ($obj) { global $o; $o = null; return new C; });
};
$tests = [
    'write' => function () { global $o; $o->s = 1; },
    'read' => function () { global $o; var_dump($o->s); },
    'isset' => function () { global $o; var_dump(isset($o->s)); },
    'unset' => function () { global $o; unset($o->s); },
    'incdec' => function () { global $o; $o->s++; },
    'foreach' => function () { global $o; foreach ($o as $k => $v) { echo "$k\n"; } },
    'clone' => function () { global $o; $x = clone $o; },
    'kept' => function () { global $o; $keep = $o; $o->s = 1; var_dump($keep->s); },
    'ret' => function () use ($r) { $r->newLazyGhost(function ($obj) {})->s = 5; echo "ok\n"; },
];
foreach ($tests as $name => $t) {
    echo "# $name\n";
    $o = $mkg();
    $a = [$r->newLazyGhost(function ($obj) { global $a; $a = null; })];
    $h = new stdClass;
    $h->o = $r->newLazyGhost(function ($obj) { global $h; $h = null; });
    try { $t(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    $a = $h = null;
}
echo "done\n";
?>
--EXPECT--
# write
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# read
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# isset
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# unset
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# incdec
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# foreach
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# clone
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# kept
int(1)
string(13) "C::__destruct"
# ret
string(13) "C::__destruct"
ok
done
