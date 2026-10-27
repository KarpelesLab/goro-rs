--TEST--
A fiber suspended inside a call made from a lean call: its running calls do not count in the resumer's call depth
--FILE--
<?php
function mk() {
    $f = new Fiber(function () { Fiber::suspend(); });
    $f->start();
    return $f;
}
function runOne($i) {
    $t = mk();
    echo "run $i\n";
}
runOne(1);
echo "between\n";
runOne(2);
echo "A\n";

?>
--EXPECTF--
run 1
between
run 2
A
