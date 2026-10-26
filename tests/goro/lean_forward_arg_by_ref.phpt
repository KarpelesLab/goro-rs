--TEST--
Forwarding calls (self::, static::, parent::) with arguments that may be sent by reference, in runs of fast opcodes and lean callees
--FILE--
<?php
class A {
    static function byval($x) { return is_array($x) ? count($x) : $x; }
    static function byref(&$x) { $x = ($x ?? 0) + 1; return $x; }
    function ibyref(&$x) { $x[] = 'i'; return count($x); }
    static function walk($tree, $depth) {
        foreach ($tree as $k => $v) {
            if (is_array($v)) { $tree[$k] = self::walk($v, $depth + 1); }
            else { $tree[$k] = self::byval($tree[$k]) . "@$depth"; }
        }
        return $tree;
    }
    function run() {
        $a = ['k' => [1, 2, 3], 'n' => 5];
        $out = [];
        for ($i = 0; $i < 3; $i++) {
            $out[] = self::byval($a['k']);
            $out[] = static::byval($a['n']);
            $out[] = self::byref($a['c']);
            $out[] = static::byref($a['k'][0]);
            $out[] = $this->ibyref($a['list']);
            $out[] = self::ibyref($a['list2']);
        }
        echo json_encode($out), "\n", json_encode($a), "\n";
        echo json_encode(self::walk(['x' => 1, 'y' => ['z' => 2, 'w' => [3]]], 0)), "\n";
    }
}
class B extends A {
    static function byval($x) { return 'B' . parent::byval($x); }
    static function byref(&$x) { $x = ($x ?? 0) + 10; return parent::byref($x); }
}
(new A)->run();
(new B)->run();
--EXPECT--
[3,5,1,2,1,1,3,5,2,3,2,2,3,5,3,4,3,3]
{"k":[4,2,3],"n":5,"c":3,"list":["i","i","i"],"list2":["i","i","i"]}
{"x":"1@0","y":{"z":"2@1","w":["3@2"]}}
[3,"B5",1,12,1,1,3,"B5",2,23,2,2,3,"B5",3,34,3,3]
{"k":[34,2,3],"n":5,"c":3,"list":["i","i","i"],"list2":["i","i","i"]}
{"x":"1@0","y":{"z":"2@1","w":["3@2"]}}
