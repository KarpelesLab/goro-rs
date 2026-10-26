--TEST--
An [$obj, 'method'] autoloader has one frame in backtraces; a closure callback is not taken for a class to autoload
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
class L {
    function load($c) { throw new Exception("load $c"); }
}
function t($e) {
    foreach ($e->getTrace() as $i => $f) {
        echo $i, ' ', $f['class'] ?? '', $f['type'] ?? '', $f['function'], ':', implode(',', array_map('gettype', $f['args'] ?? [])), "\n";
    }
}
spl_autoload_register([new L, 'load']);
try { class_exists('A1'); } catch (Exception $e) { t($e); }
echo "--\n";
try { new A2; } catch (Exception $e) { t($e); }
echo "--\n";
function f() { return A3::X; }
try { f(); } catch (Exception $e) { t($e); }
echo "--\n";
// Laravel's Application::boot(): array_walk() with a closure bound to $this
// must not autoload "__closure_N" (an autoloader that throws broke it).
class App {
    private $providers = ['a', 'b'];
    public function boot() {
        array_walk($this->providers, function ($p) { echo "boot $p\n"; });
        usort($this->providers, fn($x, $y) => strcmp($y, $x));
        echo implode(',', array_map(function ($p) { return strtoupper($p); }, $this->providers)), "\n";
    }
}
(new App)->boot();
?>
--EXPECT--
0 L->load:string
1 class_exists:string
--
0 L->load:string
--
0 L->load:string
1 f:
--
boot a
boot b
B,A
