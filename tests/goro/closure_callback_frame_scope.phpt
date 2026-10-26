--TEST--
A closure called by an internal function has its scope as the trace frame's class
--FILE--
<?php
namespace NS;
class TIF {
    public function direct(): void {
        $f = function (): void { throw new \RuntimeException('x'); };
        $f();
    }
    public function viaClosure(): void {
        array_map(function (): void {
            throw new \RuntimeException('x');
        }, [1]);
    }
    public function viaCuf(): void {
        call_user_func(function (): void {
            throw new \RuntimeException('x');
        });
    }
    public static function viaStatic(): void {
        $a = [1, 2]; usort($a, static function (): void {
            throw new \RuntimeException('x');
        });
    }
}
foreach (['direct', 'viaClosure', 'viaCuf', 'viaStatic'] as $m) {
    try { $a = [1, 2]; $m === 'viaStatic' ? TIF::viaStatic() : (new TIF)->$m(); } catch (\Throwable $e) {
        $t = $e->getTrace()[0];
        echo $t['class'] ?? '-', ' ', $t['type'] ?? '-', ' ', $t['function'], "\n";
        echo basename(strstr($e->getTraceAsString(), "\n", true)), "\n";
    }
}

?>
--EXPECT--
NS\TIF -> {closure:NS\TIF::direct():5}
closure_callback_frame_scope.php(6): NS\TIF->{closure:NS\TIF::direct():5}()
NS\TIF -> {closure:NS\TIF::viaClosure():9}
#0 [internal function]: NS\TIF->{closure:NS\TIF::viaClosure():9}(1)
NS\TIF -> {closure:NS\TIF::viaCuf():14}
#0 [internal function]: NS\TIF->{closure:NS\TIF::viaCuf():14}()
NS\TIF :: {closure:NS\TIF::viaStatic():19}
#0 [internal function]: NS\TIF::{closure:NS\TIF::viaStatic():19}(1, 2)
