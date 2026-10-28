<?php
// Driver for float_edges.txt (one PHP 8.5 run): getFloat() on tiny and
// subnormal intervals and with k above 2^53, from a user engine returning
// chosen 64-bit values. Floats are written as (signed) bit patterns.
use Random\{Randomizer, IntervalBoundary as B};
final class Script implements Random\Engine {
    public function __construct(private array $v) {}
    public function generate(): string { return pack('P', array_shift($this->v) ?? 0); }
}
function u($n) { return unpack('E', pack('J', $n))[1]; }  // float with bits $n
function bits($f) { return unpack('J', pack('E', $f))[1]; }
$cases = [
    [u(0), u(3)], [u(0), u(1)], [u(1), u(2)], [u(5), u(9)], [u(0), u(0x10000000000000)], [u(0), u(0x10000000000001)],
    [u(0x0fffffffffffff), u(0x10000000000002)], [u(0), u(0x20000000000000)], [-u(3), u(3)], [-u(2), u(5)], [-u(5), u(2)],
    [u(0x10000000000000), u(0x10000000000003)], [-u(5), -u(2)], [u(2), u(0x30000000000000)],
];
foreach ($cases as [$a, $b]) {
    foreach (['CC' => B::ClosedClosed, 'CO' => B::ClosedOpen, 'OC' => B::OpenClosed, 'OO' => B::OpenOpen] as $bn => $bv) {
        $o = [];
        foreach ([0, 1, 2, 3, 4, 5] as $k) {
            $r = new Randomizer(new Script([$k]));
            try { $f = $r->getFloat($a, $b, $bv); $o[] = (bits($f) >= 0 ? '' : '-') . (bits($f) & PHP_INT_MAX); } catch (Throwable $t) { $o[] = 'E'; }
        }
        printf("%s%d %s%d %s: %s\n", $a < 0 ? '-' : '', bits(abs($a)), $b < 0 ? '-' : '', bits(abs($b)), $bn, implode(' ', $o));
    }
}
// large-k exactness on [-1, 1]: k odd above 2^53, and k near hi
foreach ([2 ** 53 + 1, 2 ** 53 + 3, 2 ** 54 - 1, 2 ** 54 - 3, 2 ** 53 - 1] as $k) {
    foreach (['CC' => B::ClosedClosed, 'CO' => B::ClosedOpen] as $bn => $bv) {
        $r = new Randomizer(new Script([$k]));
        printf("k=%d %s: %.17g %016x\n", $k, $bn, $f = $r->getFloat(-1.0, 1.0, $bv), bits($f));
    }
}
foreach ([2 ** 53 + 1, 2 ** 53 + 3, 2 ** 54 - 3] as $k) {
    $r = new Randomizer(new Script([$k]));
    printf("k=%d [-2,1] CC: %016x\n", $k, bits($r->getFloat(-2.0, 1.0, B::ClosedClosed)));
    $r = new Randomizer(new Script([$k]));
    printf("k=%d [-1,2] CC: %016x\n", $k, bits($r->getFloat(-1.0, 2.0, B::ClosedClosed)));
}
