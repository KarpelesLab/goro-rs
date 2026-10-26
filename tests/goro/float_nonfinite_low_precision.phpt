--TEST--
INF/NAN under a precision/serialize_precision below 4: zend_gcvt() truncates them to ndigit characters
--FILE--
<?php
foreach ([0, 1, 2, 3, -1] as $p) {
    ini_set('precision', $p);
    ini_set('serialize_precision', $p);
    foreach ([INF, -INF, NAN] as $f) {
        echo $p, ": [", $f, "] [", var_export($f, true), "] [", serialize($f), "] ";
        var_dump($f);
    }
}
?>
--EXPECTF--
0: [I] [I] [d:;] float(INF)
0: [-] [-] [d:;] float(-INF)
0: [
Warning: unexpected NAN value was coerced to string in %s on line %d
N] [N] [d:;] float(NAN)
1: [I] [I] [d:I;] float(INF)
1: [-] [-] [d:-;] float(-INF)
1: [
Warning: unexpected NAN value was coerced to string in %s on line %d
N] [N] [d:N;] float(NAN)
2: [IN] [IN] [d:IN;] float(INF)
2: [-I] [-I] [d:-I;] float(-INF)
2: [
Warning: unexpected NAN value was coerced to string in %s on line %d
NA] [NA] [d:NA;] float(NAN)
3: [INF] [INF] [d:INF;] float(INF)
3: [-IN] [-IN] [d:-IN;] float(-INF)
3: [
Warning: unexpected NAN value was coerced to string in %s on line %d
NAN] [NAN] [d:NAN;] float(NAN)
-1: [INF] [INF] [d:INF;] float(INF)
-1: [-INF] [-INF] [d:-INF;] float(-INF)
-1: [
Warning: unexpected NAN value was coerced to string in %s on line %d
NAN] [NAN] [d:NAN;] float(NAN)
