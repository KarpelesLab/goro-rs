--TEST--
An error handler's exception from an internal method's warning, in a try with catch and finally (the fast opcode path): caught once, by the catch
--FILE--
<?php
set_error_handler(function ($no, $str, $file, $line) { throw new Exception("$str $line"); });
$ao = new ArrayObject([]);
try { $l = true ? $ao->offsetGet('x') : 1; } catch (Throwable $e) { echo "c1 ", $e->getMessage(), "\n"; } finally { echo "f1\n"; }
try { $l = true ? $ao->offsetGet('x') : 1; } catch (Throwable $e) { echo "c2\n"; }
try { try { $l = true ? $ao->offsetGet('x') : 1; } finally { echo "f3 inner\n"; } } catch (Throwable $e) { echo "c3\n"; } finally { echo "f3\n"; }
function f($ao) { try { return true ? $ao->offsetGet('x') : 1; } catch (Exception $e) { return "c4"; } finally { echo "f4\n"; } }
var_dump(f($ao));
try { $l = ($ao === null) ? false : $ao->offsetGet('y'); } catch (LogicException $e) { echo "wrong\n"; } catch (Exception $e) { echo "c5\n"; } finally { echo "f5\n"; }
$dom = new DOMDocument();
try { $loaded = true ? $dom->loadHTML('<meta charset="unknown"><p>x</p>') : false; } catch (Throwable $e) { echo "c6 ", $e->getMessage(), "\n"; $loaded = false; } finally { echo "f6\n"; }
var_dump($loaded);
echo "end\n";

?>
--EXPECTF--
c1 Undefined array key "x" 4
f1
c2
f3 inner
c3
f3
f4
string(2) "c4"
c5
f5
c6 DOMDocument::loadHTML(): Unsupported encoding: unknown in Entity, line: 1 11
f6
bool(false)
end
