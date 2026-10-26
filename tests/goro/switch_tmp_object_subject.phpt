--TEST--
A switch on a temporary object (enum case) compares it with every case; == still frees a temporary object operand
--FILE--
<?php
enum E { case A; case B; case C; }
function id($x) { return $x; }
switch (id(E::C)) { case E::A: echo "A\n"; break; case E::B: echo "B\n"; break; case E::C: echo "C\n"; break; default: echo "none\n"; }
$o = new stdClass; $o->t = E::C;
switch ($o->t) { case E::A: echo "A\n"; break; case E::B: echo "B\n"; break; case E::C: echo "C\n"; break; default: echo "none\n"; }
class T {
    private readonly E $type;
    public function __construct(E $t) { $this->type = $t; }
    public function m(): string {
        switch ($this->type) {
            case E::A: return 'a';
            case E::B: return 'b';
            case E::C: return 'c';
        }
        return '-';
    }
}
echo (new T(E::A))->m(), (new T(E::B))->m(), (new T(E::C))->m(), "\n";
class D { public $v; function __construct($v) { $this->v = $v; } function __destruct() { echo "dtor {$this->v}\n"; } }
$x = new D(0);
var_dump(new D(3) == $x);
echo "end\n";

?>
--EXPECT--
C
C
abc
dtor 3
bool(false)
end
dtor 0
