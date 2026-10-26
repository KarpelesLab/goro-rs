--TEST--
self::privateStatic(...) in a trait method makes a closure of the using class's method
--FILE--
<?php
trait Creator {
    private static function era(array $e): array { return [self::class, $e]; }
    public static function eras2() { $f = self::era(...); echo "made\n"; return $f([3]); }
    public static function eras() { return array_map(self::era(...), [[1]]); }
}
class C { use Creator; }
class E { use Creator; }
echo json_encode(C::eras2()), "\n";
echo json_encode(E::eras()), "\n";

?>
--EXPECT--
made
["C",[3]]
[["E",[1]]]
