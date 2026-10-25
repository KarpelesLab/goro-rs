--TEST--
catch (Throwable) in a namespace names that namespace's class unless imported
--FILE--
<?php
namespace Foo {
    try { throw new \Exception("a"); } catch (\Throwable $e) { echo "caught \\Throwable\n"; }
    try {
        try { throw new \Exception("b"); } catch (Throwable $e) { echo "wrong\n"; }
    } catch (\Exception $e) { echo "Foo\\Throwable did not catch ", $e->getMessage(), "\n"; }
}
namespace Bar {
    use Throwable;
    try { throw new \Error("c"); } catch (Throwable $e) { echo "caught imported Throwable\n"; }
}
namespace {
    try { throw new Exception("d"); } catch (Throwable $e) { echo "caught global Throwable\n"; }
}
?>
--EXPECT--
caught \Throwable
Foo\Throwable did not catch b
caught imported Throwable
caught global Throwable
