--TEST--
Taking a reference to a hooked property: a throwing get hook and write-only virtual properties
--FILE--
<?php
class A {
    public int $h { get => $this->h * 2; set(int $v) { $this->h = $v + 1; } }
    public $v { set { echo "set\n"; } }
}
$a = new A;
try { $r = &$a->h; var_dump($r); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $r = &$a->v; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$a->h = 5;
try { $r = &$a->h; var_dump($r); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo "done\n";
?>
--EXPECT--
Typed property A::$h must not be accessed before initialization
Property A::$v is write-only
Indirect modification of A::$h is not allowed
done
