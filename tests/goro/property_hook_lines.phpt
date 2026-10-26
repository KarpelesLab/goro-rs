--TEST--
Property hooks' start and end lines (getStartLine()/getEndLine(), __toString())
--FILE--
<?php
class C {
	public string $h {
		#[Foo]
		get => 'x';

		set {
			$this->h = $value;
		}
	}
	#[Attr]
	public
	function m() {
	}
	public $p { get { return 1; } }
}
$r = new ReflectionProperty('C', 'h');
foreach ($r->getHooks() as $k => $hk) echo $k, ' ', $hk->getStartLine(), '-', $hk->getEndLine(), "\n";
$m = new ReflectionMethod('C', 'm'); echo $m->getStartLine(), '-', $m->getEndLine(), "\n";
$hk = (new ReflectionProperty('C', 'p'))->getHook(PropertyHookType::Get); echo $hk->getStartLine(), '-', $hk->getEndLine(), "\n";
$f = #[A]
  function() {
  };
$r = new ReflectionFunction($f); echo $r->getStartLine(), '-', $r->getEndLine(), "\n";
$g = fn()
  => 1;
$r = new ReflectionFunction($g); echo $r->getStartLine(), '-', $r->getEndLine(), "\n";
class P2 {
  public function __construct(
    public int $x {
      get => $this->x * 2;
      set(int $v) {
        $this->x = $v;
      }
    }
  ) {}
}
foreach ((new ReflectionProperty("P2", "x"))->getHooks() as $k => $h) echo $k, " ", $h->getStartLine(), "-", $h->getEndLine(), "\n";
class Q2 { public int $y { set { throw new Exception("e"); } } }
$q2 = new Q2;
try { $q2->y = 1; } catch (Exception $e) { echo $e->getLine(), "\n"; }
echo (new ReflectionProperty("Q2", "y"))->getHook(PropertyHookType::Set);
?>
--EXPECTF--
get 5-5
set 7-9
13-14
15-15
22-23
25-26
get 31-31
set 32-34
39
Method [ <user> public method $y::set ] {
  @@ %s 39 - 39

  - Parameters [1] {
    Parameter #0 [ <required> int $value ]
  }
  - Return [ void ]
}
