--TEST--
mb_regex_set_options(): syntax letters r z j u g c b d
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
foreach (['r','z','j','u','g','c','b','d'] as $s) {
  echo "=== $s\n";
  t('a(*FAIL)|b', 'ab', $s);
  t('(?{foo})a', 'a', $s);
  t('a+', 'aaa', $s);
  t('a\+', 'a+aa', $s);
  t('a?', 'a', $s);
  t('a\?', 'a?', $s);
  t('a{2}', 'aaa', $s);
  t('a\{2\}', 'aaa', $s);
  t('(a)', 'a', $s);
  t('\(a\)', 'a', $s);
  t('\(a\)', '(a)', $s);
  t('a|b', 'b', $s);
  t('a\|b', 'b', $s);
  t('a*?', 'aa', $s);
  t('a++', 'aa', $s);
  t('(?<n>a)\k<n>', 'aa', $s);
  t('(?P<n>a)', 'a', $s);
  t('(?i)A', 'a', $s);
  t('(?s).', "\n", $s);
  t('(?m).', "\n", $s);
  t('(?m)^b', "a\nb", $s);
  t('\Qa.b\E', "a.b", $s);
  t('\w+', "ab c", $s);
  t('\<a', "ba a", $s);
  t('\`a', "a", $s);
  t("\\'", "a", $s);
  t('[[:alpha:]]+', "ab1", $s);
  t('\h+', "ab1g", $s);
  t('\x41', "A", $s);
  t('\x{41}', "A", $s);
  t('A', "A", $s);
  t('\101', "A", $s);
  t('\o{101}', "A", $s);
  t('\cA', "\x01", $s);
  t('\C-a', "\x01", $s);
  t('\M-a', "\xe1", $s);
  t('\v', "\x0b", $s);
  t('\e', "\x1b", $s);
  t('\a', "\x07", $s);
  t('\p{Alpha}', "a", $s);
  t('\p{^Alpha}', "1", $s);
  t('\R', "\r\n", $s);
  t('\K', "a", $s);
  t('a\Kb', "ab", $s);
  t('\N', "a", $s);
  t('\O', "\n", $s);
  t('\X', "a", $s);
  t('\y', "a", $s);
  t('(a)(?(1)b|c)', "ab", $s);
  t('(?~a)', "bba", $s);
  t('(a)\g<1>', "aa", $s);
  t('(?<x>a)\g<x>', "aa", $s);
  t('(?R)?a', "a", $s);
  t('(?1)(a)', "aa", $s);
  t('(?&n)(?<n>a)', "aa", $s);
  t('[a&&b]', "&", $s);
  t('[a-z&&[^b]]+', "abc", $s);
  t('*a', "a", $s);
  t('a**', "aa", $s);
  t('a{,2}', "aaa", $s);
  t('a{,2}', "a{,2}", $s);
  t('a{2', "a{2", $s);
  t('a)', "a)", $s);
  t('^*', "*", $s);
  t('(*)', "*", $s);
  t('a|*', "*", $s);
  t('x^', "x^", $s);
  t('$x', '$x', $s);
  t('(^a)', 'a', $s);
  t('(a$)', 'a', $s);
  t('a\(^b\)', 'a^b', $s);
  t('[]a]', ']', $s);
  t('[a-]', '-', $s);
  t('[a-b-c]', '-', $s);
  t('[\w-a]', '-', $s);
  t('[z-a]', 'a', $s);
  t('[\]]', ']', $s);
  t('[\d]', '5', $s);
  t('[\d]', 'd', $s);
  t('[^a]', "\n", $s);
  t('a{1,2}?', "aa", $s);
  t('a{2}?', "aa", $s);
  t('a{2}+', "aaaa", $s);
  t('(?#comment)a', "a", $s);
  t('(?:a)', "a", $s);
  t('(?=a)a', "a", $s);
  t('(?<=a)b', "ab", $s);
  t('(?<!a)b', "cb", $s);
  t('(?<=a|bc)d', "bcd", $s);
  t('(?<=a+)b', "aab", $s);
  t('(?>a+)a', "aaa", $s);
  t('a{1}{2}', "aa", $s);
  t('(a)\1', "aa", $s);
  t('\b', "a", $s);
  t('\Aa\z', "a", $s);
  t('a\Z', "a\n", $s);
  t('\Ga', "a", $s);
  t('a\s', "a ", $s);
  t('\d\D', "1a", $s);
  t('\S\W', "a ", $s);
  t('a.b', "a\nb", $s);
  t('^b', "a\nb", $s);
  t('a$', "a\nb", $s);
}
--EXPECT--
=== r
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a(*FAIL)|b","ab","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?{foo})a","a","r",false] => false []
["a+","aaa","r",false] => true ["aaa"]
["a\\+","a+aa","r",false] => true ["a+"]
["a?","a","r",false] => true ["a"]
["a\\?","a?","r",false] => true ["a?"]
["a{2}","aaa","r",false] => true ["aa"]
["a\\{2\\}","aaa","r",false] => false []
["(a)","a","r",false] => true ["a","a"]
["\\(a\\)","a","r",false] => false []
["\\(a\\)","(a)","r",false] => true ["(a)"]
["a|b","b","r",false] => true ["b"]
["a\\|b","b","r",false] => false []
["a*?","aa","r",false] => true [false]
["a++","aa","r",false] => true ["aa"]
["(?<n>a)\\k<n>","aa","r",false] => true {"0":"aa","1":"a","n":"a"}
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?P<n>a)","a","r",false] => false []
["(?i)A","a","r",false] => true ["a"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?s).","\n","r",false] => false []
["(?m).","\n","r",false] => true ["\n"]
["(?m)^b","a\nb","r",false] => true ["b"]
["\\Qa.b\\E","a.b","r",false] => false []
["\\w+","ab c","r",false] => true ["ab"]
["\\<a","ba a","r",false] => false []
["\\`a","a","r",false] => false []
["\\'","a","r",false] => false []
["[[:alpha:]]+","ab1","r",false] => true ["ab"]
["\\h+","ab1g","r",false] => true ["ab1"]
["\\x41","A","r",false] => true ["A"]
["\\x{41}","A","r",false] => true ["A"]
["A","A","r",false] => true ["A"]
["\\101","A","r",false] => true ["A"]
["\\o{101}","A","r",false] => true ["A"]
["\\cA","\u0001","r",false] => true ["\u0001"]
["\\C-a","\u0001","r",false] => true ["\u0001"]
["\\M-a","�","r",false] => false []
["\\v","\u000b","r",false] => true ["\u000b"]
["\\e","\u001b","r",false] => true ["\u001b"]
["\\a","\u0007","r",false] => true ["\u0007"]
["\\p{Alpha}","a","r",false] => true ["a"]
["\\p{^Alpha}","1","r",false] => true ["1"]
["\\R","\r\n","r",false] => true ["\r\n"]
["\\K","a","r",false] => true [false]
["a\\Kb","ab","r",false] => true ["b"]
["\\N","a","r",false] => false []
["\\O","\n","r",false] => false []
["\\X","a","r",false] => true ["a"]
["\\y","a","r",false] => true [false]
["(a)(?(1)b|c)","ab","r",false] => true ["ab","a"]
["(?~a)","bba","r",false] => true ["bb"]
["(a)\\g<1>","aa","r",false] => true ["aa","a"]
["(?<x>a)\\g<x>","aa","r",false] => true {"0":"aa","1":"a","x":"a"}
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?R)?a","a","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?1)(a)","aa","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?&n)(?<n>a)","aa","r",false] => false []
["[a&&b]","&","r",false] => false []
["[a-z&&[^b]]+","abc","r",false] => true ["a"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["*a","a","r",false] => false []
["a**","aa","r",false] => true ["aa"]
["a{,2}","aaa","r",false] => true ["aa"]
["a{,2}","a{,2}","r",false] => true ["a"]
["a{2","a{2","r",false] => true ["a{2"]
  ERR: mb_ereg(): mbregex compile err: unmatched close parenthesis
["a)","a)","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is invalid
["^*","*","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(*)","*","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a|*","*","r",false] => false []
["x^","x^","r",false] => false []
["$x","$x","r",false] => false []
["(^a)","a","r",false] => true ["a","a"]
["(a$)","a","r",false] => true ["a","a"]
["a\\(^b\\)","a^b","r",false] => false []
["[]a]","]","r",false] => true ["]"]
["[a-]","-","r",false] => true ["-"]
["[a-b-c]","-","r",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: unmatched range specifier in char-class
["[\\w-a]","-","r",false] => false []
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","r",false] => false []
["[\\]]","]","r",false] => true ["]"]
["[\\d]","5","r",false] => true ["5"]
["[\\d]","d","r",false] => false []
["[^a]","\n","r",false] => true ["\n"]
["a{1,2}?","aa","r",false] => true ["a"]
["a{2}?","aa","r",false] => true ["aa"]
["a{2}+","aaaa","r",false] => true ["aaaa"]
["(?#comment)a","a","r",false] => true ["a"]
["(?:a)","a","r",false] => true ["a"]
["(?=a)a","a","r",false] => true ["a"]
["(?<=a)b","ab","r",false] => true ["b"]
["(?<!a)b","cb","r",false] => true ["b"]
["(?<=a|bc)d","bcd","r",false] => true ["d"]
["(?<=a+)b","aab","r",false] => true ["b"]
["(?>a+)a","aaa","r",false] => false []
["a{1}{2}","aa","r",false] => true ["aa"]
["(a)\\1","aa","r",false] => true ["aa","a"]
["\\b","a","r",false] => true [false]
["\\Aa\\z","a","r",false] => true ["a"]
["a\\Z","a\n","r",false] => true ["a"]
["\\Ga","a","r",false] => true ["a"]
["a\\s","a ","r",false] => true ["a "]
["\\d\\D","1a","r",false] => true ["1a"]
["\\S\\W","a ","r",false] => true ["a "]
["a.b","a\nb","r",false] => false []
["^b","a\nb","r",false] => true ["b"]
["a$","a\nb","r",false] => true ["a"]
=== z
  ERR: mb_ereg(): mbregex compile err: undefined callout name
["a(*FAIL)|b","ab","z",false] => false []
["(?{foo})a","a","z",false] => true ["a"]
["a+","aaa","z",false] => true ["aaa"]
["a\\+","a+aa","z",false] => true ["a+"]
["a?","a","z",false] => true ["a"]
["a\\?","a?","z",false] => true ["a?"]
["a{2}","aaa","z",false] => true ["aa"]
["a\\{2\\}","aaa","z",false] => false []
["(a)","a","z",false] => true ["a","a"]
["\\(a\\)","a","z",false] => false []
["\\(a\\)","(a)","z",false] => true ["(a)"]
["a|b","b","z",false] => true ["b"]
["a\\|b","b","z",false] => false []
["a*?","aa","z",false] => true [false]
["a++","aa","z",false] => true ["aa"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?<n>a)\\k<n>","aa","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?P<n>a)","a","z",false] => false []
["(?i)A","a","z",false] => true ["a"]
["(?s).","\n","z",false] => true ["\n"]
["(?m).","\n","z",false] => false []
["(?m)^b","a\nb","z",false] => true ["b"]
["\\Qa.b\\E","a.b","z",false] => true ["a.b"]
["\\w+","ab c","z",false] => true ["ab"]
["\\<a","ba a","z",false] => false []
["\\`a","a","z",false] => false []
["\\'","a","z",false] => false []
["[[:alpha:]]+","ab1","z",false] => true ["ab"]
["\\h+","ab1g","z",false] => false []
["\\x41","A","z",false] => true ["A"]
["\\x{41}","A","z",false] => true ["A"]
["A","A","z",false] => true ["A"]
["\\101","A","z",false] => true ["A"]
["\\o{101}","A","z",false] => true ["A"]
["\\cA","\u0001","z",false] => true ["\u0001"]
["\\C-a","\u0001","z",false] => false []
["\\M-a","�","z",false] => false []
["\\v","\u000b","z",false] => false []
["\\e","\u001b","z",false] => true ["\u001b"]
["\\a","\u0007","z",false] => true ["\u0007"]
["\\p{Alpha}","a","z",false] => true ["a"]
["\\p{^Alpha}","1","z",false] => true ["1"]
["\\R","\r\n","z",false] => true ["\r\n"]
["\\K","a","z",false] => true [false]
["a\\Kb","ab","z",false] => true ["b"]
["\\N","a","z",false] => true ["a"]
["\\O","\n","z",false] => true ["\n"]
["\\X","a","z",false] => true ["a"]
["\\y","a","z",false] => true [false]
["(a)(?(1)b|c)","ab","z",false] => true ["ab","a"]
["(?~a)","bba","z",false] => true ["bb"]
["(a)\\g<1>","aa","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?<x>a)\\g<x>","aa","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?R)?a","a","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?1)(a)","aa","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?&n)(?<n>a)","aa","z",false] => false []
["[a&&b]","&","z",false] => true ["&"]
["[a-z&&[^b]]+","abc","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["*a","a","z",false] => false []
["a**","aa","z",false] => true ["aa"]
["a{,2}","aaa","z",false] => false []
["a{,2}","a{,2}","z",false] => true ["a{,2}"]
["a{2","a{2","z",false] => true ["a{2"]
  ERR: mb_ereg(): mbregex compile err: unmatched close parenthesis
["a)","a)","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is invalid
["^*","*","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid callout name
["(*)","*","z",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a|*","*","z",false] => false []
["x^","x^","z",false] => false []
["$x","$x","z",false] => false []
["(^a)","a","z",false] => true ["a","a"]
["(a$)","a","z",false] => true ["a","a"]
["a\\(^b\\)","a^b","z",false] => false []
["[]a]","]","z",false] => true ["]"]
["[a-]","-","z",false] => true ["-"]
["[a-b-c]","-","z",false] => true ["-"]
["[\\w-a]","-","z",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","z",false] => false []
["[\\]]","]","z",false] => true ["]"]
["[\\d]","5","z",false] => true ["5"]
["[\\d]","d","z",false] => false []
["[^a]","\n","z",false] => true ["\n"]
["a{1,2}?","aa","z",false] => true ["a"]
["a{2}?","aa","z",false] => true ["aa"]
["a{2}+","aaaa","z",false] => true ["aa"]
["(?#comment)a","a","z",false] => true ["a"]
["(?:a)","a","z",false] => true ["a"]
["(?=a)a","a","z",false] => true ["a"]
["(?<=a)b","ab","z",false] => true ["b"]
["(?<!a)b","cb","z",false] => true ["b"]
  ERR: mb_ereg(): mbregex compile err: invalid pattern in look-behind
["(?<=a|bc)d","bcd","z",false] => false []
["(?<=a+)b","aab","z",false] => true ["b"]
["(?>a+)a","aaa","z",false] => false []
["a{1}{2}","aa","z",false] => true ["aa"]
["(a)\\1","aa","z",false] => true ["aa","a"]
["\\b","a","z",false] => true [false]
["\\Aa\\z","a","z",false] => true ["a"]
["a\\Z","a\n","z",false] => true ["a"]
["\\Ga","a","z",false] => true ["a"]
["a\\s","a ","z",false] => true ["a "]
["\\d\\D","1a","z",false] => true ["1a"]
["\\S\\W","a ","z",false] => true ["a "]
["a.b","a\nb","z",false] => false []
["^b","a\nb","z",false] => false []
["a$","a\nb","z",false] => false []
=== j
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a(*FAIL)|b","ab","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?{foo})a","a","j",false] => false []
["a+","aaa","j",false] => true ["aaa"]
["a\\+","a+aa","j",false] => true ["a+"]
["a?","a","j",false] => true ["a"]
["a\\?","a?","j",false] => true ["a?"]
["a{2}","aaa","j",false] => true ["aa"]
["a\\{2\\}","aaa","j",false] => false []
["(a)","a","j",false] => true ["a","a"]
["\\(a\\)","a","j",false] => false []
["\\(a\\)","(a)","j",false] => true ["(a)"]
["a|b","b","j",false] => true ["b"]
["a\\|b","b","j",false] => false []
["a*?","aa","j",false] => true [false]
["a++","aa","j",false] => true ["aa"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?<n>a)\\k<n>","aa","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?P<n>a)","a","j",false] => false []
["(?i)A","a","j",false] => true ["a"]
["(?s).","\n","j",false] => true ["\n"]
["(?m).","\n","j",false] => false []
["(?m)^b","a\nb","j",false] => true ["b"]
["\\Qa.b\\E","a.b","j",false] => true ["a.b"]
["\\w+","ab c","j",false] => true ["ab"]
["\\<a","ba a","j",false] => false []
["\\`a","a","j",false] => false []
["\\'","a","j",false] => false []
["[[:alpha:]]+","ab1","j",false] => true ["a"]
["\\h+","ab1g","j",false] => false []
["\\x41","A","j",false] => true ["A"]
["\\x{41}","A","j",false] => false []
["A","A","j",false] => true ["A"]
["\\101","A","j",false] => true ["A"]
["\\o{101}","A","j",false] => false []
["\\cA","\u0001","j",false] => true ["\u0001"]
["\\C-a","\u0001","j",false] => false []
["\\M-a","�","j",false] => false []
["\\v","\u000b","j",false] => true ["\u000b"]
["\\e","\u001b","j",false] => true ["\u001b"]
["\\a","\u0007","j",false] => true ["\u0007"]
["\\p{Alpha}","a","j",false] => true ["a"]
  ERR: mb_ereg(): mbregex compile err: invalid character property name {^Alpha}
["\\p{^Alpha}","1","j",false] => false []
["\\R","\r\n","j",false] => false []
["\\K","a","j",false] => false []
["a\\Kb","ab","j",false] => false []
["\\N","a","j",false] => false []
["\\O","\n","j",false] => false []
["\\X","a","j",false] => false []
["\\y","a","j",false] => true [false]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(a)(?(1)b|c)","ab","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?~a)","bba","j",false] => false []
["(a)\\g<1>","aa","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?<x>a)\\g<x>","aa","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?R)?a","a","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?1)(a)","aa","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?&n)(?<n>a)","aa","j",false] => false []
["[a&&b]","&","j",false] => false []
["[a-z&&[^b]]+","abc","j",false] => true ["a"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["*a","a","j",false] => false []
["a**","aa","j",false] => true ["aa"]
["a{,2}","aaa","j",false] => false []
["a{,2}","a{,2}","j",false] => true ["a{,2}"]
["a{2","a{2","j",false] => true ["a{2"]
  ERR: mb_ereg(): mbregex compile err: unmatched close parenthesis
["a)","a)","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is invalid
["^*","*","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(*)","*","j",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a|*","*","j",false] => false []
["x^","x^","j",false] => false []
["$x","$x","j",false] => false []
["(^a)","a","j",false] => true ["a","a"]
["(a$)","a","j",false] => true ["a","a"]
["a\\(^b\\)","a^b","j",false] => false []
["[]a]","]","j",false] => true ["]"]
["[a-]","-","j",false] => true ["-"]
["[a-b-c]","-","j",false] => true ["-"]
["[\\w-a]","-","j",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","j",false] => false []
["[\\]]","]","j",false] => true ["]"]
["[\\d]","5","j",false] => true ["5"]
["[\\d]","d","j",false] => false []
["[^a]","\n","j",false] => true ["\n"]
["a{1,2}?","aa","j",false] => true ["a"]
["a{2}?","aa","j",false] => true ["aa"]
["a{2}+","aaaa","j",false] => true ["aa"]
["(?#comment)a","a","j",false] => true ["a"]
["(?:a)","a","j",false] => true ["a"]
["(?=a)a","a","j",false] => true ["a"]
["(?<=a)b","ab","j",false] => true ["b"]
["(?<!a)b","cb","j",false] => true ["b"]
["(?<=a|bc)d","bcd","j",false] => true ["d"]
["(?<=a+)b","aab","j",false] => true ["b"]
["(?>a+)a","aaa","j",false] => false []
["a{1}{2}","aa","j",false] => true ["aa"]
["(a)\\1","aa","j",false] => true ["aa","a"]
["\\b","a","j",false] => true [false]
["\\Aa\\z","a","j",false] => true ["a"]
["a\\Z","a\n","j",false] => true ["a"]
["\\Ga","a","j",false] => true ["a"]
["a\\s","a ","j",false] => true ["a "]
["\\d\\D","1a","j",false] => true ["1a"]
["\\S\\W","a ","j",false] => true ["a "]
["a.b","a\nb","j",false] => false []
["^b","a\nb","j",false] => false []
["a$","a\nb","j",false] => false []
=== u
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a(*FAIL)|b","ab","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?{foo})a","a","u",false] => false []
["a+","aaa","u",false] => true ["aaa"]
["a\\+","a+aa","u",false] => true ["a+"]
["a?","a","u",false] => true ["a"]
["a\\?","a?","u",false] => true ["a?"]
["a{2}","aaa","u",false] => true ["aa"]
["a\\{2\\}","aaa","u",false] => false []
["(a)","a","u",false] => true ["a","a"]
["\\(a\\)","a","u",false] => false []
["\\(a\\)","(a)","u",false] => true ["(a)"]
["a|b","b","u",false] => true ["b"]
["a\\|b","b","u",false] => false []
["a*?","aa","u",false] => true ["aa"]
["a++","aa","u",false] => true ["aa"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<n>a)\\k<n>","aa","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?P<n>a)","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?i)A","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?s).","\n","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m).","\n","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m)^b","a\nb","u",false] => false []
["\\Qa.b\\E","a.b","u",false] => false []
["\\w+","ab c","u",false] => true ["ab"]
["\\<a","ba a","u",false] => true ["a"]
["\\`a","a","u",false] => false []
["\\'","a","u",false] => false []
["[[:alpha:]]+","ab1","u",false] => true ["ab"]
["\\h+","ab1g","u",false] => false []
["\\x41","A","u",false] => false []
["\\x{41}","A","u",false] => false []
["A","A","u",false] => true ["A"]
["\\101","A","u",false] => false []
["\\o{101}","A","u",false] => false []
["\\cA","\u0001","u",false] => false []
["\\C-a","\u0001","u",false] => false []
["\\M-a","�","u",false] => false []
["\\v","\u000b","u",false] => false []
["\\e","\u001b","u",false] => false []
["\\a","\u0007","u",false] => false []
["\\p{Alpha}","a","u",false] => false []
["\\p{^Alpha}","1","u",false] => false []
["\\R","\r\n","u",false] => false []
["\\K","a","u",false] => false []
["a\\Kb","ab","u",false] => false []
["\\N","a","u",false] => false []
["\\O","\n","u",false] => false []
["\\X","a","u",false] => false []
["\\y","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(a)(?(1)b|c)","ab","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?~a)","bba","u",false] => false []
["(a)\\g<1>","aa","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<x>a)\\g<x>","aa","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?R)?a","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?1)(a)","aa","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?&n)(?<n>a)","aa","u",false] => false []
["[a&&b]","&","u",false] => true ["&"]
["[a-z&&[^b]]+","abc","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["*a","a","u",false] => false []
["a**","aa","u",false] => true ["aa"]
["a{,2}","aaa","u",false] => false []
["a{,2}","a{,2}","u",false] => true ["a{,2}"]
["a{2","a{2","u",false] => true ["a{2"]
  ERR: mb_ereg(): mbregex compile err: unmatched close parenthesis
["a)","a)","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is invalid
["^*","*","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(*)","*","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a|*","*","u",false] => false []
["x^","x^","u",false] => false []
["$x","$x","u",false] => false []
["(^a)","a","u",false] => true ["a","a"]
["(a$)","a","u",false] => true ["a","a"]
["a\\(^b\\)","a^b","u",false] => false []
["[]a]","]","u",false] => true ["]"]
["[a-]","-","u",false] => true ["-"]
["[a-b-c]","-","u",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: unmatched range specifier in char-class
["[\\w-a]","-","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","u",false] => false []
["[\\]]","]","u",false] => true ["]"]
["[\\d]","5","u",false] => true ["5"]
["[\\d]","d","u",false] => false []
["[^a]","\n","u",false] => true ["\n"]
["a{1,2}?","aa","u",false] => true ["aa"]
["a{2}?","aa","u",false] => true ["aa"]
["a{2}+","aaaa","u",false] => true ["aaaa"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?#comment)a","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?:a)","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?=a)a","a","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a)b","ab","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<!a)b","cb","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a|bc)d","bcd","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a+)b","aab","u",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?>a+)a","aaa","u",false] => false []
["a{1}{2}","aa","u",false] => true ["aa"]
["(a)\\1","aa","u",false] => true ["aa","a"]
["\\b","a","u",false] => true [false]
["\\Aa\\z","a","u",false] => true ["a"]
["a\\Z","a\n","u",false] => true ["a"]
["\\Ga","a","u",false] => true ["a"]
["a\\s","a ","u",false] => true ["a "]
["\\d\\D","1a","u",false] => true ["1a"]
["\\S\\W","a ","u",false] => true ["a "]
["a.b","a\nb","u",false] => false []
["^b","a\nb","u",false] => true ["b"]
["a$","a\nb","u",false] => true ["a"]
=== g
["a(*FAIL)|b","ab","g",false] => false []
["(?{foo})a","a","g",false] => false []
["a+","aaa","g",false] => false []
["a\\+","a+aa","g",false] => true ["a"]
["a?","a","g",false] => false []
["a\\?","a?","g",false] => true ["a"]
["a{2}","aaa","g",false] => false []
["a\\{2\\}","aaa","g",false] => true ["aa"]
["(a)","a","g",false] => false []
["\\(a\\)","a","g",false] => true ["a","a"]
["\\(a\\)","(a)","g",false] => true ["a","a"]
["a|b","b","g",false] => false []
["a\\|b","b","g",false] => true ["b"]
["a*?","aa","g",false] => false []
["a++","aa","g",false] => false []
["(?<n>a)\\k<n>","aa","g",false] => false []
["(?P<n>a)","a","g",false] => false []
["(?i)A","a","g",false] => false []
["(?s).","\n","g",false] => false []
["(?m).","\n","g",false] => false []
["(?m)^b","a\nb","g",false] => false []
["\\Qa.b\\E","a.b","g",false] => false []
["\\w+","ab c","g",false] => false []
["\\<a","ba a","g",false] => true ["a"]
["\\`a","a","g",false] => false []
["\\'","a","g",false] => false []
["[[:alpha:]]+","ab1","g",false] => false []
["\\h+","ab1g","g",false] => false []
["\\x41","A","g",false] => false []
["\\x{41}","A","g",false] => false []
["A","A","g",false] => true ["A"]
["\\101","A","g",false] => false []
["\\o{101}","A","g",false] => false []
["\\cA","\u0001","g",false] => false []
["\\C-a","\u0001","g",false] => false []
["\\M-a","�","g",false] => false []
["\\v","\u000b","g",false] => false []
["\\e","\u001b","g",false] => false []
["\\a","\u0007","g",false] => false []
["\\p{Alpha}","a","g",false] => false []
["\\p{^Alpha}","1","g",false] => false []
["\\R","\r\n","g",false] => false []
["\\K","a","g",false] => false []
["a\\Kb","ab","g",false] => false []
["\\N","a","g",false] => false []
["\\O","\n","g",false] => false []
["\\X","a","g",false] => false []
["\\y","a","g",false] => false []
["(a)(?(1)b|c)","ab","g",false] => false []
["(?~a)","bba","g",false] => false []
["(a)\\g<1>","aa","g",false] => false []
["(?<x>a)\\g<x>","aa","g",false] => false []
["(?R)?a","a","g",false] => false []
["(?1)(a)","aa","g",false] => false []
["(?&n)(?<n>a)","aa","g",false] => false []
["[a&&b]","&","g",false] => true ["&"]
["[a-z&&[^b]]+","abc","g",false] => false []
["*a","a","g",false] => false []
["a**","aa","g",false] => true ["aa"]
["a{,2}","aaa","g",false] => false []
["a{,2}","a{,2}","g",false] => true ["a{,2}"]
["a{2","a{2","g",false] => true ["a{2"]
["a)","a)","g",false] => true ["a)"]
["^*","*","g",false] => true ["*"]
["(*)","*","g",false] => false []
["a|*","*","g",false] => false []
["x^","x^","g",false] => true ["x^"]
["$x","$x","g",false] => true ["$x"]
["(^a)","a","g",false] => false []
["(a$)","a","g",false] => false []
["a\\(^b\\)","a^b","g",false] => false []
["[]a]","]","g",false] => true ["]"]
["[a-]","-","g",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: unmatched range specifier in char-class
["[a-b-c]","-","g",false] => false []
["[\\w-a]","-","g",false] => false []
["[z-a]","a","g",false] => false []
["[\\]]","]","g",false] => false []
["[\\d]","5","g",false] => false []
["[\\d]","d","g",false] => true ["d"]
["[^a]","\n","g",false] => false []
["a{1,2}?","aa","g",false] => false []
["a{2}?","aa","g",false] => false []
["a{2}+","aaaa","g",false] => false []
["(?#comment)a","a","g",false] => false []
["(?:a)","a","g",false] => false []
["(?=a)a","a","g",false] => false []
["(?<=a)b","ab","g",false] => false []
["(?<!a)b","cb","g",false] => false []
["(?<=a|bc)d","bcd","g",false] => false []
["(?<=a+)b","aab","g",false] => false []
["(?>a+)a","aaa","g",false] => false []
["a{1}{2}","aa","g",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(a)\\1","aa","g",false] => false []
["\\b","a","g",false] => true [false]
["\\Aa\\z","a","g",false] => false []
["a\\Z","a\n","g",false] => false []
["\\Ga","a","g",false] => false []
["a\\s","a ","g",false] => false []
["\\d\\D","1a","g",false] => false []
["\\S\\W","a ","g",false] => false []
["a.b","a\nb","g",false] => false []
["^b","a\nb","g",false] => true ["b"]
["a$","a\nb","g",false] => true ["a"]
=== c
["a(*FAIL)|b","ab","c",false] => false []
["(?{foo})a","a","c",false] => false []
["a+","aaa","c",false] => true ["aaa"]
["a\\+","a+aa","c",false] => true ["a+"]
["a?","a","c",false] => true ["a"]
["a\\?","a?","c",false] => true ["a?"]
["a{2}","aaa","c",false] => false []
["a\\{2\\}","aaa","c",false] => true ["aa"]
["(a)","a","c",false] => false []
["\\(a\\)","a","c",false] => true ["a","a"]
["\\(a\\)","(a)","c",false] => true ["a","a"]
["a|b","b","c",false] => false []
["a\\|b","b","c",false] => true ["b"]
["a*?","aa","c",false] => true ["aa"]
["a++","aa","c",false] => true ["aa"]
["(?<n>a)\\k<n>","aa","c",false] => false []
["(?P<n>a)","a","c",false] => false []
["(?i)A","a","c",false] => false []
["(?s).","\n","c",false] => false []
["(?m).","\n","c",false] => false []
["(?m)^b","a\nb","c",false] => false []
["\\Qa.b\\E","a.b","c",false] => false []
["\\w+","ab c","c",false] => false []
["\\<a","ba a","c",false] => false []
["\\`a","a","c",false] => true ["a"]
["\\'","a","c",false] => true [false]
["[[:alpha:]]+","ab1","c",false] => false []
["\\h+","ab1g","c",false] => false []
["\\x41","A","c",false] => false []
["\\x{41}","A","c",false] => false []
["A","A","c",false] => true ["A"]
["\\101","A","c",false] => false []
["\\o{101}","A","c",false] => false []
["\\cA","\u0001","c",false] => false []
["\\C-a","\u0001","c",false] => false []
["\\M-a","�","c",false] => false []
["\\v","\u000b","c",false] => false []
["\\e","\u001b","c",false] => true ["\u001b"]
["\\a","\u0007","c",false] => true ["\u0007"]
["\\p{Alpha}","a","c",false] => false []
["\\p{^Alpha}","1","c",false] => false []
["\\R","\r\n","c",false] => false []
["\\K","a","c",false] => false []
["a\\Kb","ab","c",false] => false []
["\\N","a","c",false] => false []
["\\O","\n","c",false] => false []
["\\X","a","c",false] => false []
["\\y","a","c",false] => true [false]
["(a)(?(1)b|c)","ab","c",false] => false []
["(?~a)","bba","c",false] => false []
["(a)\\g<1>","aa","c",false] => false []
["(?<x>a)\\g<x>","aa","c",false] => false []
["(?R)?a","a","c",false] => false []
["(?1)(a)","aa","c",false] => false []
["(?&n)(?<n>a)","aa","c",false] => false []
["[a&&b]","&","c",false] => true ["&"]
["[a-z&&[^b]]+","abc","c",false] => false []
["*a","a","c",false] => false []
["a**","aa","c",false] => true ["aa"]
["a{,2}","aaa","c",false] => false []
["a{,2}","a{,2}","c",false] => true ["a{,2}"]
["a{2","a{2","c",false] => true ["a{2"]
["a)","a)","c",false] => true ["a)"]
["^*","*","c",false] => true ["*"]
["(*)","*","c",false] => false []
["a|*","*","c",false] => false []
["x^","x^","c",false] => false []
["$x","$x","c",false] => false []
["(^a)","a","c",false] => false []
["(a$)","a","c",false] => false []
["a\\(^b\\)","a^b","c",false] => false []
["[]a]","]","c",false] => true ["]"]
["[a-]","-","c",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: unmatched range specifier in char-class
["[a-b-c]","-","c",false] => false []
["[\\w-a]","-","c",false] => false []
["[z-a]","a","c",false] => false []
["[\\]]","]","c",false] => false []
["[\\d]","5","c",false] => false []
["[\\d]","d","c",false] => true ["d"]
["[^a]","\n","c",false] => true ["\n"]
["a{1,2}?","aa","c",false] => false []
["a{2}?","aa","c",false] => false []
["a{2}+","aaaa","c",false] => false []
["(?#comment)a","a","c",false] => true ["a"]
["(?:a)","a","c",false] => false []
["(?=a)a","a","c",false] => false []
["(?<=a)b","ab","c",false] => false []
["(?<!a)b","cb","c",false] => false []
["(?<=a|bc)d","bcd","c",false] => false []
["(?<=a+)b","aab","c",false] => false []
["(?>a+)a","aaa","c",false] => false []
["a{1}{2}","aa","c",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(a)\\1","aa","c",false] => false []
["\\b","a","c",false] => false []
["\\Aa\\z","a","c",false] => false []
["a\\Z","a\n","c",false] => false []
["\\Ga","a","c",false] => false []
["a\\s","a ","c",false] => false []
["\\d\\D","1a","c",false] => false []
["\\S\\W","a ","c",false] => false []
["a.b","a\nb","c",false] => false []
["^b","a\nb","c",false] => true ["b"]
["a$","a\nb","c",false] => true ["a"]
=== b
["a(*FAIL)|b","ab","b",false] => false []
["(?{foo})a","a","b",false] => false []
["a+","aaa","b",false] => false []
["a\\+","a+aa","b",false] => true ["a+"]
["a?","a","b",false] => false []
["a\\?","a?","b",false] => true ["a?"]
["a{2}","aaa","b",false] => false []
["a\\{2\\}","aaa","b",false] => true ["aa"]
["(a)","a","b",false] => false []
["\\(a\\)","a","b",false] => true ["a","a"]
["\\(a\\)","(a)","b",false] => true ["a","a"]
["a|b","b","b",false] => false []
["a\\|b","b","b",false] => false []
["a*?","aa","b",false] => false []
["a++","aa","b",false] => false []
["(?<n>a)\\k<n>","aa","b",false] => false []
["(?P<n>a)","a","b",false] => false []
["(?i)A","a","b",false] => false []
["(?s).","\n","b",false] => false []
["(?m).","\n","b",false] => false []
["(?m)^b","a\nb","b",false] => false []
["\\Qa.b\\E","a.b","b",false] => false []
["\\w+","ab c","b",false] => false []
["\\<a","ba a","b",false] => false []
["\\`a","a","b",false] => false []
["\\'","a","b",false] => false []
["[[:alpha:]]+","ab1","b",false] => false []
["\\h+","ab1g","b",false] => false []
["\\x41","A","b",false] => false []
["\\x{41}","A","b",false] => false []
["A","A","b",false] => true ["A"]
["\\101","A","b",false] => false []
["\\o{101}","A","b",false] => false []
["\\cA","\u0001","b",false] => false []
["\\C-a","\u0001","b",false] => false []
["\\M-a","�","b",false] => false []
["\\v","\u000b","b",false] => false []
["\\e","\u001b","b",false] => true ["\u001b"]
["\\a","\u0007","b",false] => true ["\u0007"]
["\\p{Alpha}","a","b",false] => false []
["\\p{^Alpha}","1","b",false] => false []
["\\R","\r\n","b",false] => false []
["\\K","a","b",false] => false []
["a\\Kb","ab","b",false] => false []
["\\N","a","b",false] => false []
["\\O","\n","b",false] => false []
["\\X","a","b",false] => false []
["\\y","a","b",false] => true [false]
["(a)(?(1)b|c)","ab","b",false] => false []
["(?~a)","bba","b",false] => false []
["(a)\\g<1>","aa","b",false] => false []
["(?<x>a)\\g<x>","aa","b",false] => false []
["(?R)?a","a","b",false] => false []
["(?1)(a)","aa","b",false] => false []
["(?&n)(?<n>a)","aa","b",false] => false []
["[a&&b]","&","b",false] => true ["&"]
["[a-z&&[^b]]+","abc","b",false] => false []
["*a","a","b",false] => false []
["a**","aa","b",false] => true ["aa"]
["a{,2}","aaa","b",false] => false []
["a{,2}","a{,2}","b",false] => true ["a{,2}"]
["a{2","a{2","b",false] => true ["a{2"]
["a)","a)","b",false] => true ["a)"]
["^*","*","b",false] => true ["*"]
["(*)","*","b",false] => false []
["a|*","*","b",false] => false []
["x^","x^","b",false] => true ["x^"]
["$x","$x","b",false] => true ["$x"]
["(^a)","a","b",false] => false []
["(a$)","a","b",false] => false []
["a\\(^b\\)","a^b","b",false] => false []
["[]a]","]","b",false] => true ["]"]
["[a-]","-","b",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: unmatched range specifier in char-class
["[a-b-c]","-","b",false] => false []
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[\\w-a]","-","b",false] => false []
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","b",false] => false []
["[\\]]","]","b",false] => false []
["[\\d]","5","b",false] => false []
["[\\d]","d","b",false] => true ["d"]
["[^a]","\n","b",false] => true ["\n"]
["a{1,2}?","aa","b",false] => false []
["a{2}?","aa","b",false] => false []
["a{2}+","aaaa","b",false] => false []
["(?#comment)a","a","b",false] => false []
["(?:a)","a","b",false] => false []
["(?=a)a","a","b",false] => false []
["(?<=a)b","ab","b",false] => false []
["(?<!a)b","cb","b",false] => false []
["(?<=a|bc)d","bcd","b",false] => false []
["(?<=a+)b","aab","b",false] => false []
["(?>a+)a","aaa","b",false] => false []
["a{1}{2}","aa","b",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(a)\\1","aa","b",false] => false []
["\\b","a","b",false] => false []
["\\Aa\\z","a","b",false] => false []
["a\\Z","a\n","b",false] => false []
["\\Ga","a","b",false] => false []
["a\\s","a ","b",false] => false []
["\\d\\D","1a","b",false] => false []
["\\S\\W","a ","b",false] => false []
["a.b","a\nb","b",false] => true ["a\nb"]
["^b","a\nb","b",false] => false []
["a$","a\nb","b",false] => false []
=== d
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a(*FAIL)|b","ab","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?{foo})a","a","d",false] => false []
["a+","aaa","d",false] => true ["aaa"]
["a\\+","a+aa","d",false] => true ["a+"]
["a?","a","d",false] => true ["a"]
["a\\?","a?","d",false] => true ["a?"]
["a{2}","aaa","d",false] => true ["aa"]
["a\\{2\\}","aaa","d",false] => false []
["(a)","a","d",false] => true ["a","a"]
["\\(a\\)","a","d",false] => false []
["\\(a\\)","(a)","d",false] => true ["(a)"]
["a|b","b","d",false] => true ["b"]
["a\\|b","b","d",false] => false []
["a*?","aa","d",false] => true ["aa"]
["a++","aa","d",false] => true ["aa"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<n>a)\\k<n>","aa","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?P<n>a)","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?i)A","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?s).","\n","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m).","\n","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?m)^b","a\nb","d",false] => false []
["\\Qa.b\\E","a.b","d",false] => false []
["\\w+","ab c","d",false] => false []
["\\<a","ba a","d",false] => false []
["\\`a","a","d",false] => false []
["\\'","a","d",false] => false []
["[[:alpha:]]+","ab1","d",false] => true ["ab"]
["\\h+","ab1g","d",false] => false []
["\\x41","A","d",false] => false []
["\\x{41}","A","d",false] => false []
["A","A","d",false] => true ["A"]
["\\101","A","d",false] => false []
["\\o{101}","A","d",false] => false []
["\\cA","\u0001","d",false] => false []
["\\C-a","\u0001","d",false] => false []
["\\M-a","�","d",false] => false []
["\\v","\u000b","d",false] => false []
["\\e","\u001b","d",false] => true ["\u001b"]
["\\a","\u0007","d",false] => true ["\u0007"]
  ERR: mb_ereg(): mbregex compile err: invalid repeat range {lower,upper}
["\\p{Alpha}","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid repeat range {lower,upper}
["\\p{^Alpha}","1","d",false] => false []
["\\R","\r\n","d",false] => false []
["\\K","a","d",false] => false []
["a\\Kb","ab","d",false] => false []
["\\N","a","d",false] => false []
["\\O","\n","d",false] => false []
["\\X","a","d",false] => false []
["\\y","a","d",false] => true [false]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(a)(?(1)b|c)","ab","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?~a)","bba","d",false] => false []
["(a)\\g<1>","aa","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<x>a)\\g<x>","aa","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?R)?a","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?1)(a)","aa","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?&n)(?<n>a)","aa","d",false] => false []
["[a&&b]","&","d",false] => true ["&"]
["[a-z&&[^b]]+","abc","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["*a","a","d",false] => false []
["a**","aa","d",false] => true ["aa"]
  ERR: mb_ereg(): mbregex compile err: invalid repeat range {lower,upper}
["a{,2}","aaa","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid repeat range {lower,upper}
["a{,2}","a{,2}","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid repeat range {lower,upper}
["a{2","a{2","d",false] => false []
["a)","a)","d",false] => true ["a)"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is invalid
["^*","*","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(*)","*","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["a|*","*","d",false] => false []
["x^","x^","d",false] => false []
["$x","$x","d",false] => false []
["(^a)","a","d",false] => true ["a","a"]
["(a$)","a","d",false] => true ["a","a"]
["a\\(^b\\)","a^b","d",false] => false []
["[]a]","]","d",false] => true ["]"]
["[a-]","-","d",false] => true ["-"]
["[a-b-c]","-","d",false] => true ["-"]
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[\\w-a]","-","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: empty range in char class
["[z-a]","a","d",false] => false []
["[\\]]","]","d",false] => false []
["[\\d]","5","d",false] => false []
["[\\d]","d","d",false] => true ["d"]
["[^a]","\n","d",false] => true ["\n"]
["a{1,2}?","aa","d",false] => true ["aa"]
["a{2}?","aa","d",false] => true ["aa"]
["a{2}+","aaaa","d",false] => true ["aaaa"]
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?#comment)a","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?:a)","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?=a)a","a","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a)b","ab","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<!a)b","cb","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a|bc)d","bcd","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?<=a+)b","aab","d",false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?>a+)a","aaa","d",false] => false []
["a{1}{2}","aa","d",false] => true ["aa"]
["(a)\\1","aa","d",false] => true ["aa","a"]
["\\b","a","d",false] => false []
["\\Aa\\z","a","d",false] => false []
["a\\Z","a\n","d",false] => false []
["\\Ga","a","d",false] => false []
["a\\s","a ","d",false] => false []
["\\d\\D","1a","d",false] => false []
["\\S\\W","a ","d",false] => false []
["a.b","a\nb","d",false] => true ["a\nb"]
["^b","a\nb","d",false] => false []
["a$","a\nb","d",false] => false []
