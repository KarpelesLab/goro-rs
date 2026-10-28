<?php
$c = [
 ['/abc/', 'abc'], ['/abc/', 'xxabc'], ['/a/', 'a'], ['/(a)/', 'a'], ['/((a))/', 'a'], ['/(((a)))/', 'a'], ['/(?:a)/', 'a'], ['/(?:(?:a))/', 'a'],
 ['/a|b/', 'b'], ['/a|b|c/', 'c'], ['/(a|b)/', 'b'], ['/(?>a)/', 'a'], ['/(?=a)a/', 'a'], ['/(?!b)a/', 'a'], ['/(?<=x)a/', 'xa'],
 ['/.*\p{N}/', '0123456789'], ['/\p{Nd}/', '0123456789'], ['/.*a/', 'aaaaaaaaab'], ['/.*?a/', 'bbbbbbbbba'], ['/a*b/', 'aaaab'], ['/a*?b/', 'aaaab'],
 ['/[a-z]*1/', 'abcdefgh1'], ['/[a-z]+?1/', 'abcdefgh1'], ['/\d+5/', '123456'], ['/\d+?5/', '123456'], ['/(a)*/', 'aaaa'], ['/(a)+/', 'aaaa'], ['/(?:a)*/', 'aaaa'],
 ['/(?:ab)*c/', 'ababc'], ['/(ab)*c/', 'ababc'], ['/(?:a|b)*c/', 'ababc'], ['/(a|b)*c/', 'ababc'], ['/(a|b)*?c/', 'ababc'], ['/(?:a|b)+c/', 'ababc'],
 ['/^(a\1?){4}$/', 'aaaaaaaaaa'], ['/(a+)+b/', 'aaaab'], ['/(a+)+b/', 'aaaa'], ['/(?:a+)+b/', 'aaaaaaaaaaaab'], ['/\p{Ll}(\p{L}((\p{Ll}\p{Ll})))/', 'aeiou'],
 ['/\p{Ll}\p{L}\p{Ll}\p{Ll}/', 'aeiou'], ['/(\d*)/', 'ab2c3u'], ['/^[a-z]+/', 'abc'], ['/(?:\D+|<\d+>)*[!?]/', 'foobar foobar foobar'],
 ['/x(?R)?y/', 'xxyy'], ['/(a(?1)?b)/', 'aabb'], ['/a(?1)?(b)/', 'abb'], ['/(?(1)a|b)/', 'b'], ['/(a)?(?(1)a|b)/', 'b'], ['/a(*COMMIT)b/', 'ab'],
 ['/a(*MARK:x)b/', 'ab'], ['/a\Kb/', 'ab'], ['/\b\w+\b/', 'hello world'], ['/^.*$/', 'hello'], ['/^(\w+)\s(\w+)$/', 'hello world'],
 ['/(?:(a)|b)*/', 'abab'], ['/(a?)*b/', 'aab'], ['/(a*)*b/', 'aab'], ['/a{3,5}/', 'aaaaaa'], ['/a{3,5}?b/', 'aaaaab'], ['/(?:ab){2,3}/', 'abababab'],
 ['/(ab){2,3}/', 'abababab'], ['/(ab){0,3}?c/', 'ababc'], ['/a*+b/', 'aaab'], ['/(?>a*)b/', 'aaab'], ['/a++b/', 'aaab'], ['/[ab]+c/', 'ababc'],
 ['/\s+$/', "abc   "], ['/\s+$/', "abc   x"], ['/^\s+/', "   abc"], ['/(\s+)$/', "abc   "], ['/.+@.+/', 'me@ex.com'], ['/<[^>]+>/', 'a <b> c'],
 ['/\[(right)\](((?R)|[^[]+?|\[)*)\[\/\1\]/siU', '[CODE]&lt;td align=&quot;$stylevar[right]&quot;&gt;[/CODE]'],
];
foreach ($c as [$p, $s]) echo bin2hex($p), "\t", bin2hex($s), "\n";
