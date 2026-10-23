--TEST--
Lazy ghost released by its initializer: each in-place operation throws
--FILE--
<?php
class C {
    public $s;
    public function m() { return 1; }
    public function __destruct() { var_dump(__METHOD__); }
}
$r = new ReflectionClass(C::class);
$mk = function () use ($r) {
    return $r->newLazyGhost(function ($obj) { global $o; $o = null; });
};
$tests = [
    'write' => function () { global $o; $o->s = 1; },
    'read' => function () { global $o; var_dump($o->s); },
    'isset' => function () { global $o; var_dump(isset($o->s)); },
    'unset' => function () { global $o; unset($o->s); },
    'incdec' => function () { global $o; $o->s++; },
    'assignop' => function () { global $o; $o->s .= 'x'; },
    'fetchdim' => function () { global $o; $o->s[] = 1; },
    'foreach' => function () { global $o; foreach ($o as $k => $v) { echo "$k\n"; } },
    'gov' => function () { global $o; var_dump(get_object_vars($o)); },
    'json' => function () { global $o; var_dump(json_encode($o)); },
    'arrcast' => function () { global $o; var_dump(count((array)$o)); },
    'clone' => function () { global $o; $x = clone $o; },
];
foreach ($tests as $name => $t) {
    echo "# $name\n";
    $o = $mk();
    try { $t(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
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
# assignop
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# fetchdim
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# foreach
string(13) "C::__destruct"
Error: Lazy object was released during initialization
# gov
string(13) "C::__destruct"
array(1) {
  ["s"]=>
  NULL
}
# json
string(13) "C::__destruct"
string(10) "{"s":null}"
# arrcast
int(0)
# clone
string(13) "C::__destruct"
Error: Lazy object was released during initialization
done
