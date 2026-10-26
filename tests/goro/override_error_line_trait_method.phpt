--TEST--
#[\Override] on a trait method without a parent method: reported at its line in the trait
--FILE--
<?php
trait T {

    #[\Override]
    function m() {}
}
class C {
    use T;
}
--EXPECTF--

Fatal error: C::m() has #[\Override] attribute, but no matching parent method exists in %s on line 5
