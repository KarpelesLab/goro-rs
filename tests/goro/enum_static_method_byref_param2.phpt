--TEST--
An enum's static recursion check (league/uri StringCoercionMode): the by-reference array parameter is the caller's variable
--FILE--
<?php
enum Mode {
    case A;
    private const M = "\0__MARK__\0";
    public function coerce(mixed $value): ?string {
        return match ($this) {
            self::A => match (true) {
                is_object($value) => '[o]',
                is_array($value) => match (true) {
                    self::hcr($value) => 'circ',
                    array_is_list($value) => implode(',', array_map($this->coerce(...), $value)),
                    default => '[object Object]',
                },
                is_scalar($value) => (string) $value,
                default => 'x',
            },
        };
    }
    private static function hcr(array &$arr): bool {
        if (isset($arr[self::M])) return true;
        try {
            $arr[self::M] = true;
            foreach ($arr as $key => &$value) {
                if (self::M !== $key && is_array($value) && self::hcr($value)) return true;
            }
            return false;
        } finally {
            unset($arr[self::M]);
        }
    }
}
var_dump(Mode::A->coerce([[1, [2]], 3]), Mode::A->coerce([1, 2]));
class K {
    private const M = "\0__MARK__\0";
    public static function t(array $v) { self::h($v); return [array_is_list($v), array_keys($v)]; }
    private static function h(array &$arr) { try { $arr[self::M] = true; return false; } finally { unset($arr[self::M]); } }
}
var_dump(K::t([1, 2]));

?>
--EXPECTF--
string(5) "1,2,3"
string(3) "1,2"
array(2) {
  [0]=>
  bool(true)
  [1]=>
  array(2) {
    [0]=>
    int(0)
    [1]=>
    int(1)
  }
}
