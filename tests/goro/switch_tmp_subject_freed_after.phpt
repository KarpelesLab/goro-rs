--TEST--
A temporary switch subject lives until the switch ends (its destructor runs after the matched branch)
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
switch (new D(1)) { case $x: echo "eq\n"; break; case new D(2): echo "eq2\n"; break; default: echo "default\n"; }
var_dump(new D(3) == $x);
echo "end\n";

?>
--EXPECT--
C
C
abc
dtor 2
default
dtor 1
dtor 3
bool(false)
end
dtor 0
