--TEST--
Call sites keep their targets while other functions are declared, and see functions declared later
--FILE--
<?php
namespace N {
    function run($round) {
        $o = [];
        // (an unqualified call: N\strlen once declared before the first call, else the global one)
        $o[] = strlen("abc");
        try { $o[] = later(); } catch (\Error $e) { $o[] = $e->getMessage(); }
        $o[] = \strlen("abcde");
        return implode(", ", $o);
    }
}
namespace {
    function f() { return "f"; }
    function call_all($round) {
        $o = [];
        $o[] = f();
        $o[] = strlen("ab");
        try { $o[] = g(); } catch (Error $e) { $o[] = $e->getMessage(); }
        $name = $round % 2 ? 'f' : 'h';
        try { $o[] = $name(); } catch (Error $e) { $o[] = $e->getMessage(); }
        $c = function () use ($round) { return "closure $round"; };
        $o[] = $c();
        $fcc = f(...);
        $o[] = $fcc();
        return implode(", ", $o);
    }
    for ($i = 1; $i <= 4; $i++) {
        echo call_all($i), "\n";
        echo N\run($i), "\n";
        // functions declared between the rounds
        if ($i == 1) {
            eval('function g() { return "g"; } function unrelated1() {}');
        }
        if ($i == 2) {
            eval('function h() { return "h"; }');
            eval('namespace N; function later() { return "N\\\\later"; }');
        }
        if ($i == 3) {
            eval('namespace N; function strlen($s) { return "N\\\\strlen"; }');
        }
    }
}
?>
--EXPECT--
f, 2, Call to undefined function g(), f, closure 1, f
3, Call to undefined function N\later(), 5
f, 2, g, Call to undefined function h(), closure 2, f
3, Call to undefined function N\later(), 5
f, 2, g, f, closure 3, f
3, N\later, 5
f, 2, g, h, closure 4, f
3, N\later, 5
