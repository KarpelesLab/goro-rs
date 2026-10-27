--TEST--
#[\Override] on a hook without a parent is reported on the hook's line
--FILE--
<?php
class A {
    public string $p {
        #[Override]
        #[DelayedTargetValidation]

        set => strtoupper($value);
    }
}
--EXPECTF--

Fatal error: A::$p::set() has #[\Override] attribute, but no matching parent method exists in %s on line 7
