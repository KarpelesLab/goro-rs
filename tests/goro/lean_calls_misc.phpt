--TEST--
Lean calls: func_num_args(), temporaries sent to builtins, a class named with one leading underscore
--FILE--
<?php
// func_num_args() in lean callees; sends of temporaries to builtins; a class
// named with one leading underscore
function go($option, $default = false) { $passed = func_num_args() > 1; return [$passed, func_num_args()]; }
function none() { return func_num_args(); }
function extra($a) { return func_num_args(); }
class K {
    function m($a, $b = 2) { return func_num_args(); }
    static function s($a = 1) { return func_num_args(); }
}
function unused($x) { func_num_args(); return $x; }
function outer($x) { return [go($x), go($x, 1), none(), extra(1, 2, 3)]; }
function rs($input, $slug) { return strtr($input, array('$slug' => $slug)); }
function rs2($s) { return str_replace(['a', 'b'], ['x', 'y'], $s . '!'); }
function rs3($a) { return implode(',', array_merge($a, [9])); }
function rs4($a) { return max(array_map('intval', $a)); }
function rs7($s) { return sprintf('%s-%d', strtoupper($s), strlen($s)); }
#[AllowDynamicProperties]
class _Dep {
    public $handle; public $src; public $deps = array(); public $ver = false; public $args = null; public $extra = array();
    public function __construct(...$args) {
        list($this->handle, $this->src, $this->deps, $this->ver, $this->args) = $args;
        if (!is_array($this->deps)) { $this->deps = array(); }
    }
    public function add_data($name, $data) { if (!is_scalar($name)) return false; $this->extra[$name] = $data; return true; }
}
class Reg {
    public $registered = [];
    public function add($handle, $src, $deps = array(), $ver = false, $args = null) {
        if (isset($this->registered[$handle])) return false;
        $this->registered[$handle] = new _Dep($handle, $src, $deps, $ver, $args);
        return true;
    }
    public function add_data($handle, $key, $value) {
        if (!isset($this->registered[$handle])) return false;
        return $this->registered[$handle]->add_data($key, $value);
    }
}
function reg($r, $i) { $r->add("h$i", "/x/$i.js", ['a'], '1.0'); $r->add_data("h$i", 'group', $i); $r->add("j$i", false, null); }
$k = new K;
$reg = new Reg;
for ($i = 0; $i < 40; $i++) {
    $r = [go('a'), go('a', 'b'), none(), extra(1), extra(1, 2), $k->m(1), $k->m(1, 2), K::s(), K::s(5), unused(3), outer(1),
          rs('a$slug-b', "x$i"), rs2('abc'), rs3([1, 2]), rs4(['1', '7', '3']), rs7('ab')];
    reg($reg, $i);
}
echo json_encode($r), "\n";
echo count($reg->registered), " ", json_encode($reg->registered['h39']), " ", json_encode($reg->registered['j39']), "\n";
--EXPECT--
[[false,1],[true,2],0,1,2,1,2,0,1,3,[[false,1],[true,2],0,3],"ax39-b","xyc!","1,2,9",7,"AB-2"]
80 {"handle":"h39","src":"\/x\/39.js","deps":["a"],"ver":"1.0","args":null,"extra":{"group":39}} {"handle":"j39","src":false,"deps":[],"ver":false,"args":null,"extra":[]}
