--TEST--
mb_regex_set_options() and per-call options: mode letters i x m s p l n e
--EXTENSIONS--
mbstring
--FILE--
<?php
set_error_handler(function($n,$s){ echo "  ERR: $s\n"; return true; });
function t($pat, $str, $opt = null, $i = false) {
    if ($opt !== null) mb_regex_set_options($opt);
    $m = null;
    try {
        $r = $i ? mb_eregi($pat, $str, $m) : mb_ereg($pat, $str, $m);
    } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; if ($opt !== null) mb_regex_set_options('pr'); return; }
    echo json_encode([$pat, $str, $opt, $i], JSON_UNESCAPED_UNICODE|JSON_INVALID_UTF8_SUBSTITUTE), " => ", var_export($r, true), " ", json_encode($m, JSON_UNESCAPED_UNICODE|JSON_INVALID_UTF8_SUBSTITUTE), "\n";
    if ($opt !== null) mb_regex_set_options('pr');
}
function ti($pat, $str) { t($pat, $str, null, true); }
foreach (['', 'i', 'x', 'm', 's', 'p', 'l', 'n', 'ms', 'mp', 'ix', 'r', 'z', 'j', 'u', 'g', 'c', 'b', 'd', 'imsxlnpr', 'zr', 'rz', 'e', 'q', 'iz', 'mz', 'sz', 'pz', 'mj', 'sj', 'md', 'sb', 'pb', 'xz', 'lr', 'nr'] as $o) {
  echo "== [$o]\n";
  try { echo mb_regex_set_options($o), " -> ", mb_regex_set_options(), "\n"; } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
  mb_regex_set_options($o === 'e' || $o === 'q' ? 'pr' : ($o === '' ? '' : $o));
  t('a.b', "a\nb", null);
  t('^b', "a\nb", null);
  t('a$', "a\nb", null);
  t('A', "a", null);
  t('a b', "ab", null);
  t('a|ab', "abc", null);
  t('x*', "xxx", null);
  t('x*', "yxx", null);
  t('(?m:.)', "\n", null);
  t('(?s:.)', "\n", null);
  t('(?i:A)', "a", null);
  t('(?x: a )', "a", null);
  t('(?-m:.)', "\n", null);
  t('(?m)^b', "a\nb", null);
  t('\Ab', "a\nb", null);
  t('a\Z', "a\n", null);
  t('a\z', "a\n", null);
  mb_regex_set_options('pr');
}
echo "== per-call\n";
foreach (['', 'i', 'x', 'm', 's', 'p', 'l', 'n', 'r', 'z', 'j', 'u', 'g', 'c', 'b', 'd', 'mz', 'sz', 'iz', 'zl'] as $o) {
  var_dump(mb_ereg_replace('a.b', 'X', "a\nb aXb", $o));
  var_dump(mb_ereg_replace('^b', 'X', "a\nb", $o));
  var_dump(mb_ereg_replace('a|ab', 'X', "abab", $o));
  var_dump(mb_ereg_replace('x*', '-', "abc", $o));
  var_dump(mb_ereg_match('a.b', "a\nb", $o));
  var_dump(mb_ereg_match('A', "a", $o));
  var_dump(mb_split('\s*', "a b"));
  mb_ereg_search_init("a\nb ab", 'a.b', $o);
  var_dump(mb_ereg_search_regs());
  var_dump(mb_ereg_search_getpos());
}
--EXPECT--
== []
pr -> r
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [i]
pr -> ir
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => true ["a"]
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [x]
pr -> xr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => true ["ab"]
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [m]
pr -> mr
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [s]
pr -> sr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [p]
pr -> pr
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [l]
pr -> lr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["ab"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true ["xx"]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [n]
pr -> nr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true ["xx"]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [ms]
pr -> pr
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [mp]
pr -> pr
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [ix]
pr -> ixr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => true ["a"]
["a b","ab",null,false] => true ["ab"]
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [r]
pr -> r
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [z]
pr -> z
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [j]
pr -> j
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [u]
pr -> u
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?s:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?i:A)","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?x: a )","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?-m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [g]
pr -> g
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => false []
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => false []
["(?x: a )","a",null,false] => false []
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [c]
pr -> c
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => false []
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => false []
["(?x: a )","a",null,false] => false []
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [b]
pr -> b
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => false []
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => false []
["(?x: a )","a",null,false] => false []
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [d]
pr -> d
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?s:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?i:A)","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?x: a )","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?-m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [imsxlnpr]
pr -> ixplnr
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => true ["a"]
["a b","ab",null,false] => true ["ab"]
["a|ab","abc",null,false] => true ["ab"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true ["xx"]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [zr]
pr -> r
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [rz]
pr -> z
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [e]
ValueError: Option "e" is not supported
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [q]
ValueError: Option "q" is not supported
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [iz]
pr -> iz
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => true ["a"]
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [mz]
pr -> mz
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => true ["\n"]
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [sz]
pr -> sz
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [pz]
pr -> pz
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => true ["\n"]
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [mj]
pr -> mj
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => true ["\n"]
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => true ["\n"]
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [sj]
pr -> sj
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [md]
pr -> md
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?s:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?i:A)","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?x: a )","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?-m:.)","\n",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [sb]
pr -> sb
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => false []
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => false []
["(?x: a )","a",null,false] => false []
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [pb]
pr -> pb
["a.b","a\nb",null,false] => true ["a\nb"]
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => false []
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => false []
["(?x: a )","a",null,false] => false []
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => false []
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => false []
["a\\z","a\n",null,false] => false []
== [xz]
pr -> xz
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => false []
["a$","a\nb",null,false] => false []
["A","a",null,false] => false []
["a b","ab",null,false] => true ["ab"]
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true [false]
["(?m:.)","\n",null,false] => false []
["(?s:.)","\n",null,false] => true ["\n"]
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [lr]
pr -> lr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["ab"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true ["xx"]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== [nr]
pr -> nr
["a.b","a\nb",null,false] => false []
["^b","a\nb",null,false] => true ["b"]
["a$","a\nb",null,false] => true ["a"]
["A","a",null,false] => false []
["a b","ab",null,false] => false []
["a|ab","abc",null,false] => true ["a"]
["x*","xxx",null,false] => true ["xxx"]
["x*","yxx",null,false] => true ["xx"]
["(?m:.)","\n",null,false] => true ["\n"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s:.)","\n",null,false] => false []
["(?i:A)","a",null,false] => true ["a"]
["(?x: a )","a",null,false] => true ["a"]
["(?-m:.)","\n",null,false] => false []
["(?m)^b","a\nb",null,false] => true ["b"]
["\\Ab","a\nb",null,false] => false []
["a\\Z","a\n",null,false] => true ["a"]
["a\\z","a\n",null,false] => false []
== per-call
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(true)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(3) "X X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(true)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(1) {
  [0]=>
  string(3) "a
b"
}
int(3)
string(5) "a
b X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(3) "X X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(true)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(1) {
  [0]=>
  string(3) "a
b"
}
int(3)
string(5) "a
b X"
string(3) "a
X"
string(2) "XX"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(3) "abc"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "abab"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
X"
string(4) "abab"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(3) "X X"
string(3) "a
b"
string(4) "abab"
string(7) "-a-b-c-"
bool(true)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(1) {
  [0]=>
  string(3) "a
b"
}
int(3)
string(3) "X X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(true)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(1) {
  [0]=>
  string(3) "a
b"
}
int(3)
string(3) "X X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(true)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
array(1) {
  [0]=>
  string(3) "a
b"
}
int(3)
string(5) "a
b X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
b"
string(4) "XbXb"
string(7) "-a-b-c-"
bool(false)
bool(true)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
string(5) "a
b X"
string(3) "a
b"
string(2) "XX"
string(7) "-a-b-c-"
bool(false)
bool(false)
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
bool(false)
int(6)
