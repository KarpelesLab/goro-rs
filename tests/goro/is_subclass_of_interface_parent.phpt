--TEST--
is_subclass_of() / is_a() through the parent of an implemented interface
--FILE--
<?php
namespace B { interface FI extends \Serializable {} }
namespace R {
use B\FI as BaseFI;
interface FI extends BaseFI {}
final class F implements FI {
    public function serialize() {} public function unserialize($d) {}
    public function __serialize(): array { return []; } public function __unserialize(array $d): void {}
}
var_dump(is_subclass_of('R\F', 'B\FI'), is_subclass_of('R\F', 'Serializable'), is_a('R\F', 'B\FI', true));
var_dump(is_subclass_of(new F, 'B\FI'), is_subclass_of('R\FI', 'B\FI'), is_subclass_of('R\FI', 'Serializable'));
var_dump(new F instanceof BaseFI, in_array('B\FI', class_implements('R\F')));
}

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
