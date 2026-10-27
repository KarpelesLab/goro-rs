--TEST--
ClosedGeneratorException: at the delegating generator's yield from
--FILE--
<?php
function from() {
    yield 1;
    throw new Exception();
}
function gen($gen) {
    try {
        var_dump(
            yield from $gen
        );
    } catch (Exception $e) {
        echo get_class($e), " @ ", $e->getLine(), "\n";
    }
}
$gen = from();
$gens[] = gen($gen);
$gens[] = gen($gen);
foreach ($gens as $g) {
    $g->current();
}
do {
    foreach ($gens as $g) {
        $g->next();
    }
} while ($gens[0]->valid());
--EXPECTF--
Exception @ 4
ClosedGeneratorException @ 9
