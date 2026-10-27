--TEST--
#[\Override] on an interface method no internal parent interface declares
--FILE--
<?php
interface CI extends \IteratorAggregate {
    #[\Override]
    public function nope(): void;
}
echo "unreached\n";

?>
--EXPECTF--

Fatal error: CI::nope() has #[\Override] attribute, but no matching parent method exists in %s on line 4
