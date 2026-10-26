--TEST--
Set hook parameter types: ?T, unions and supertypes are accepted
--FILE--
<?php
class P {}
class C extends P {}
class B {
    public string $a { set(?string $v) { $this->a = $v ?? ''; } }
    public ?string $b { set(string|null $v) { $this->b = $v; } }
    public int $c { set(int|string $v) { $this->c = (int) $v; } }
    public C $d { set(P $v) { } }
    public string $e { set(Stringable|string $v) { $this->e = (string) $v; } }
    public false $f { set(?bool $v) { } }
}
$b = new B;
$b->a = null;
$b->b = null;
$b->c = "12";
var_dump($b->a, $b->b, $b->c);
--EXPECT--
string(0) ""
NULL
int(12)
