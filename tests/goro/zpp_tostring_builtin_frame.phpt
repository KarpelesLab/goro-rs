--TEST--
__toString() run by a builtin's argument parsing runs in the builtin's frame, called from [internal function]
--FILE--
<?php
class S { function __toString(): string { throw new Exception; } }
function t($f) { try { $f(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; } }
t(fn() => strtoupper(new S));
t(fn() => str_repeat(new S, 2));
t(fn() => trim(new S));
t(fn() => md5(new S));
t(fn() => explode(",", new S));
t(fn() => htmlspecialchars(new S));
t(fn() => mb_strlen(new S));
t(fn() => implode(",", [new S]));
t(fn() => strcmp(new S, "a"));
t(fn() => in_array("a", [new S]));
t(fn() => str_replace("a", "b", new S));
t(function () { $x = new S; settype($x, "string"); });
t(fn() => strlen(new S));
t(fn() => sprintf("%s", new S));
t(fn() => (string) new S);
t(fn() => array_map('strtoupper', [new S]));
t(fn() => call_user_func('strtoupper', new S));
t(fn() => "a" . new S);
t(function () { echo new S; });
t(fn() => (new ArrayObject([]))->offsetGet(new S));
t(fn() => str_contains("abc", new S));
t(fn() => strtoupper(...)(new S));
$f = 'strtoupper';
t(fn() => $f(new S));
t(fn() => sprintf("%d %s", 1, new S));
class T { function __toString(): string { return debug_print_backtrace() ?? "x"; } }
echo strtoupper(new T), "\n";
echo "a" . new T, "\n";
--EXPECTF--
#0 [internal function]: S->__toString()
#1 %s(4): strtoupper(Object(S))
#2 %s(3): {closure:%s:4}()
#3 %s(4): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(5): str_repeat(Object(S), 2)
#2 %s(3): {closure:%s:5}()
#3 %s(5): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(6): trim(Object(S))
#2 %s(3): {closure:%s:6}()
#3 %s(6): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(7): md5(Object(S))
#2 %s(3): {closure:%s:7}()
#3 %s(7): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(8): explode(',', Object(S))
#2 %s(3): {closure:%s:8}()
#3 %s(8): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(9): htmlspecialchars(Object(S))
#2 %s(3): {closure:%s:9}()
#3 %s(9): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(10): mb_strlen(Object(S))
#2 %s(3): {closure:%s:10}()
#3 %s(10): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(11): implode(',', Array)
#2 %s(3): {closure:%s:11}()
#3 %s(11): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(12): strcmp(Object(S), 'a')
#2 %s(3): {closure:%s:12}()
#3 %s(12): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(13): in_array('a', Array)
#2 %s(3): {closure:%s:13}()
#3 %s(13): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(14): str_replace('a', 'b', Object(S))
#2 %s(3): {closure:%s:14}()
#3 %s(14): t(Object(Closure))
#4 {main}
#0 [internal function]: S->__toString()
#1 %s(15): settype(Object(S), 'string')
#2 %s(3): {closure:%s:15}()
#3 %s(15): t(Object(Closure))
#4 {main}
#0 %s(16): S->__toString()
#1 %s(3): {closure:%s:16}()
#2 %s(16): t(Object(Closure))
#3 {main}
#0 %s(17): S->__toString()
#1 %s(3): {closure:%s:17}()
#2 %s(17): t(Object(Closure))
#3 {main}
#0 %s(18): S->__toString()
#1 %s(3): {closure:%s:18}()
#2 %s(18): t(Object(Closure))
#3 {main}
#0 [internal function]: S->__toString()
#1 [internal function]: strtoupper(Object(S))
#2 %s(19): array_map('strtoupper', Array)
#3 %s(3): {closure:%s:19}()
#4 %s(19): t(Object(Closure))
#5 {main}
#0 [internal function]: S->__toString()
#1 %s(20): strtoupper(Object(S))
#2 %s(3): {closure:%s:20}()
#3 %s(20): t(Object(Closure))
#4 {main}
#0 %s(21): S->__toString()
#1 %s(3): {closure:%s:21}()
#2 %s(21): t(Object(Closure))
#3 {main}
#0 %s(22): S->__toString()
#1 %s(3): {closure:%s:22}()
#2 %s(22): t(Object(Closure))
#3 {main}

Fatal error: Uncaught TypeError: Cannot access offset of type S on ArrayObject in %s:23
Stack trace:
#0 %s(23): ArrayObject->offsetGet(Object(S))
#1 %s(3): {closure:%s:23}()
#2 %s(23): t(Object(Closure))
#3 {main}
  thrown in %s on line 23
