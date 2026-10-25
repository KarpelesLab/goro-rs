--TEST--
return $undef with a throwing error handler: return types, finally, generators, include, eval
--FILE--
<?php
set_error_handler(function($n, $s) { echo "handler: $s\n"; throw new Exception($s); });
function a(): mixed { try { return $u; } catch (Exception $e) { echo "caught a\n"; return 1; } }
function b(): ?int { try { return $u; } catch (Exception $e) { echo "caught b\n"; return 2; } }
function c() { try { throw new Exception("x"); } catch (Exception $e) { try { return $u; } catch (Exception $e2) { echo "caught c\n"; return 3; } } }
function d() { try { try { } finally { try { return $u; } catch (Exception $e) { echo "caught d\n"; return 4; } } } catch (Exception $e) { echo "caught d outer\n"; return 44; } }
function e() { try { return $u; } catch (Exception $e) { echo "caught e\n"; return 5; } finally { echo "fin e\n"; } }
function f() { try { if (true) { return $u; } } catch (Exception $e) { echo "caught f\n"; return 6; } }
function g() { $x = 1; try { return $x; } catch (Exception $e) { echo "caught g\n"; return 7; } }
function h() { static $s; try { return $s; } catch (Exception $e) { echo "caught h\n"; return 8; } }
function i($p) { unset($p); try { return $p; } catch (Exception $e) { echo "caught i\n"; return 9; } }
function j() { global $gg; try { return $gg; } catch (Exception $e) { echo "caught j\n"; return 10; } }
function k() { try { return $GLOBALS['nope']; } catch (Exception $e) { echo "caught k\n"; return 11; } }
function l() { try { return yield; } catch (Exception $e) { echo "caught l\n"; } }
function m() { try { yield 1; return $u; } catch (Exception $e) { echo "caught m\n"; } }
function n() { try { yield 1; return $u; } catch (Exception $e) { echo "caught n\n"; } finally { echo "fin n\n"; } }
foreach (range('a', 'k') as $fn) {
    try { var_dump($fn(1)); } catch (Exception $e) { echo "outer $fn: ", $e->getMessage(), "\n"; }
}
foreach (['m', 'n'] as $fn) {
    try { $gen = $fn(); foreach ($gen as $v) { echo "y $v\n"; } var_dump($gen->valid()); var_dump($gen->getReturn()); } catch (Exception $e) { echo "outer $fn: ", $e->getMessage(), "\n"; }
    try { var_dump($gen->valid()); var_dump($gen->getReturn()); } catch (Throwable $e) { echo "after $fn: ", $e->getMessage(), "\n"; }
}
file_put_contents(sys_get_temp_dir() . '/goro_inc_ret.inc', '<?php try { return $u; } catch (Exception $e) { echo "caught inc\n"; return 1; }');
try { var_dump(include sys_get_temp_dir() . '/goro_inc_ret.inc'); } catch (Exception $e) { echo "outer include\n"; }
unlink(sys_get_temp_dir() . '/goro_inc_ret.inc');
try { var_dump(eval('try { return $u; } catch (Exception $e) { echo "caught eval\n"; return 1; }')); } catch (Exception $e) { echo "outer eval\n"; }
echo "end\n";
--EXPECT--
handler: Undefined variable $u
outer a: Undefined variable $u
handler: Undefined variable $u
caught b
int(2)
handler: Undefined variable $u
outer c: Undefined variable $u
handler: Undefined variable $u
outer d: Undefined variable $u
handler: Undefined variable $u
caught e
fin e
int(5)
handler: Undefined variable $u
outer f: Undefined variable $u
int(1)
NULL
handler: Undefined variable $p
outer i: Undefined variable $p
NULL
handler: Undefined global variable $nope
caught k
int(11)
y 1
handler: Undefined variable $u
outer m: Undefined variable $u
bool(false)
NULL
y 1
handler: Undefined variable $u
caught n
fin n
bool(false)
NULL
bool(false)
NULL
handler: Undefined variable $u
outer include
handler: Undefined variable $u
outer eval
end
