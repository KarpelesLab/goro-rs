--TEST--
A class with __toString() implicitly implements Stringable and is not early-bound (nikic/php-parser's TokenPolyfill)
--FILE--
<?php
namespace PhpParser\Internal;

var_dump(class_exists(TokenPolyfill::class, false));
if (\PHP_VERSION_ID >= 80000) {
    class TokenPolyfill extends \ArrayObject {
    }
    var_dump(get_parent_class(TokenPolyfill::class));
    return;
}

class TokenPolyfill {
    public function __toString(): string {
        return '';
    }
}
?>
--EXPECT--
bool(false)
string(11) "ArrayObject"
