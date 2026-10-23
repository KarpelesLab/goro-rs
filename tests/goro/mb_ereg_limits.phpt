--TEST--
mbstring.regex_retry_limit / regex_stack_limit semantics
--EXTENSIONS--
mbstring
--FILE--
<?php
set_error_handler(function($n,$s){ echo "  ERR: $s\n"; return true; });
$cases = [['(a*)*b', str_repeat('a', 12)], ['((a)|b)*', str_repeat('ab', 3000)], ['(a|b\g<1>c)', str_repeat('b', 100).'a'.str_repeat('c', 100)]];
foreach ([[null, null], ['1000', null], ['100', null], [null, '50'], [null, '1'], ['0', '0'], ['-1', '-1'], ['4294967295', '4294967296']] as [$r, $st]) {
  echo "== retry=$r stack=$st\n";
  if ($r !== null) ini_set('mbstring.regex_retry_limit', $r);
  if ($st !== null) ini_set('mbstring.regex_stack_limit', $st);
  foreach ($cases as [$p, $s]) {
    var_dump(mb_ereg($p, $s, $m));
    var_dump(mb_ereg_match($p, $s));
    var_dump(strlen((string)mb_ereg_replace($p, 'x', $s)));
    mb_ereg_search_init($s, $p);
    var_dump(mb_ereg_search());
  }
  ini_restore('mbstring.regex_retry_limit');
  ini_restore('mbstring.regex_stack_limit');
}
--EXPECT--
== retry= stack=
bool(false)
bool(false)
int(12)
bool(false)
bool(true)
bool(true)
int(2)
bool(true)
bool(true)
bool(false)
int(163)
bool(true)
== retry=1000 stack=
bool(false)
bool(false)
int(12)
bool(false)
bool(false)
bool(false)
  ERR: mb_ereg_replace(): mbregex search failure in php_mbereg_replace_exec(): retry-limit-in-match over
int(0)
  ERR: mb_ereg_search(): mbregex search failure in mbregex_search(): retry-limit-in-match over
bool(false)
bool(true)
bool(false)
int(163)
bool(true)
== retry=100 stack=
bool(false)
bool(false)
int(12)
bool(false)
bool(false)
bool(false)
  ERR: mb_ereg_replace(): mbregex search failure in php_mbereg_replace_exec(): retry-limit-in-match over
int(0)
  ERR: mb_ereg_search(): mbregex search failure in mbregex_search(): retry-limit-in-match over
bool(false)
bool(true)
bool(false)
int(163)
bool(true)
== retry= stack=50
bool(false)
bool(false)
int(12)
bool(false)
bool(false)
bool(false)
  ERR: mb_ereg_replace(): mbregex search failure in php_mbereg_replace_exec(): match-stack limit over
int(0)
  ERR: mb_ereg_search(): mbregex search failure in mbregex_search(): match-stack limit over
bool(false)
bool(true)
bool(false)
int(163)
bool(true)
== retry= stack=1
bool(false)
bool(false)
int(12)
bool(false)
bool(false)
bool(false)
  ERR: mb_ereg_replace(): mbregex search failure in php_mbereg_replace_exec(): match-stack limit over
int(0)
  ERR: mb_ereg_search(): mbregex search failure in mbregex_search(): match-stack limit over
bool(false)
bool(true)
bool(false)
int(163)
bool(true)
== retry=0 stack=0
bool(false)
bool(false)
int(12)
bool(false)
bool(true)
bool(true)
int(2)
bool(true)
bool(true)
bool(false)
int(163)
bool(true)
== retry=-1 stack=-1
bool(false)
bool(false)
int(12)
bool(false)
bool(true)
bool(true)
int(2)
bool(true)
bool(true)
bool(false)
int(163)
bool(true)
== retry=4294967295 stack=4294967296
bool(false)
bool(false)
int(12)
bool(false)
bool(true)
bool(true)
int(2)
bool(true)
bool(true)
bool(false)
int(163)
bool(true)
