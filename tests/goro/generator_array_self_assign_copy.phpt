--TEST--
$a[k] = $a in a generator stores a copy of $a
--FILE--
<?php
function g2() {
    $d = ['y' => 0];
    $d['p'] = $d;
    $d['y'] = 10;
    echo $d['p']['y'], "\n";
    yield 1;
}
foreach (g2() as $v);
function g3() {
    $d = ['y' => 0];
    $d['p'] = $d;
    $d['y'] = 10;
    echo $d['p']['y'], "\n";
}
g3();
function g4() {
    $d = ['y' => 0, 'z' => 1];
    $d['p'] = $d;
    $d['y'] = 10;
    echo $d['p']['y'], "\n";
    yield 1;
}
foreach (g4() as $v);
function g5($d) {
    $d['p'] = $d;
    $d['y'] = 10;
    echo $d['p']['y'], "\n";
    yield 1;
}
foreach (g5(['y' => 0]) as $v);
function g6() {
    $d = [];
    $d['y'] = 0;
    $d['p'] = $d;
    $d['y'] = 10;
    echo $d['p']['y'], "\n";
    yield 1;
}
foreach (g6() as $v);
function f(array $context) {
    $context['_parent'] = $context;
    $context['_seq'] = $context['items'];
    foreach ($context['_seq'] as $context["_key"] => $context["item"]) {
        $context['_parent'] = $context;
        $context['_seq'] = $context['items'];
        foreach ($context['_seq'] as $context["_key"] => $context["item"]) {
            yield "[" . $context['item'] . "]";
        }
        $_parent = $context['_parent'];
        unset($context['_seq'], $context['_key'], $context['item'], $context['_parent']);
        $context = array_intersect_key($context, $_parent);
        $context += $_parent;
        yield "<" . $context['item'] . ">";
    }
}
foreach (f(['items' => ['a', 'b']]) as $v) echo $v;
echo "\n";
function g() {
    $d = ['y' => 0];
    foreach ([1, 2] as $d['y']) {
        $d['p'] = $d;
        foreach ([10] as $d['y']) { yield 1; }
        echo $d['p']['y'], "\n";
    }
}
foreach (g() as $v);

?>
--EXPECT--
0
0
0
0
0
[a][b]<a>[a][b]<b>
1
2
