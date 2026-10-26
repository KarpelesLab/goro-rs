--TEST--
Fast isset() of string offsets and null, .= on shared strings, &$a[k] and $a = &$tmp
--FILE--
<?php
function iss($text, $n = null) {
    $r = [isset($text[1]), isset($text[0]), isset($text[-1]), isset($text[-5]), isset($text[100]),
          isset($n['k']), isset($n[0]), empty($text[1]), empty($n['x'])];
    $r[] = isset($undef['k']);
    $r[] = $text[1] ?? 'dflt';
    $r[] = $n['k'] ?? 'nd';
    return $r;
}
function iss_ref(&$s) { return [isset($s[2]), isset($s[9])]; }
function cat($a, $b) {
    $s = '';
    $s .= $a;
    $s .= $b;
    $t = $s;
    $t .= 7;
    $u = $a;
    $u .= $u;
    $e = '';
    $e .= '';
    return [$s, $t, $u, $e, strlen($e)];
}
function cat_ref(&$s, $x) { $s .= $x; return $s; }
function aset(&$input_array, $path, $value = null) {
    $path_length = count($path);
    for ($i = 0; $i < $path_length - 1; ++$i) {
        $path_element = $path[$i];
        if (!array_key_exists($path_element, $input_array)) {
            $input_array[$path_element] = array();
        }
        $input_array = &$input_array[$path_element];
    }
    $input_array[$path[$i]] = $value;
}
function getref_shared() {
    $inner = [1, 2];
    $a = ['x' => $inner, 'n' => 5, 3 => 'i'];
    $b = $a;
    $r = &$a['x'];
    $r[] = 3;
    $q = &$a['n'];
    $q = 6;
    $m = &$a['missing'];
    $k = &$a['3'];
    $k .= 'j';
    unset($r, $q, $m, $k);
    return [$a, $b, $inner];
}
for ($i = 0; $i < 40; $i++) {
    $r = [];
    $r[] = iss("hello", null);
    $r[] = iss("h", ['k' => 1]);
    $r[] = iss("", null);
    $s = "abc";
    $r[] = iss_ref($s);
    $r[] = cat("x$i", "y");
    $r[] = cat("", 5);
    $str = "s";
    $r[] = cat_ref($str, "t");
    $r[] = $str;
    $arr = [];
    aset($arr, ['a', 'b', 'c'], $i);
    aset($arr, ['a', 'd'], 1);
    $copy = $arr;
    aset($arr, ['a', 'b', 'e'], 2);
    $r[] = [$arr, $copy];
    $r[] = getref_shared();
}
echo json_encode($r), "\n";
--EXPECT--
[[true,true,true,true,false,false,false,false,true,false,"e","nd"],[false,true,true,false,false,true,false,true,true,false,"dflt",1],[false,false,false,false,false,false,false,true,true,false,"dflt","nd"],[true,false],["x39y","x39y7","x39x39","",0],["5","57","","",0],"st","st",[{"a":{"b":{"c":39,"e":2},"d":1}},{"a":{"b":{"c":39},"d":1}}],[{"x":[1,2,3],"n":6,"3":"ij","missing":null},{"x":[1,2],"n":5,"3":"i"},[1,2]]]
