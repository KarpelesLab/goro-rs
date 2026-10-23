--TEST--
mb_eregi(): negated classes, multi-char case folds (ß/ss, ﬀ, İ/ı, ſ, K)
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
ti('[^a]', 'A');
ti('[^a]', 'b');
ti('[^A-Z]+', 'abcDEF123');
ti('[^k]', "\u{212A}");
ti('[^\x{212A}]', "k");
ti('[^\x{212A}]', "K");
ti('ß', 'ss');
ti('ss', 'ß');
ti('ß', 'SS');
ti('SS', 'ß');
ti('^ß$', 'ss');
ti('^ss$', 'ß');
ti('[ß]', 'ss');
ti('^[ß]$', 'ss');
ti('ﬀ', 'ff');
ti('ff', 'ﬀ');
ti('^ﬀ$', 'FF');
ti('i', 'İ');
ti('İ', 'i');
ti('İ', "i\u{307}");
ti("i\u{307}", 'İ');
ti('ı', 'I');
ti('I', 'ı');
ti('ı', 'i');
ti('[ı]', 'I');
ti('s', 'ſ');
ti('ſ', 'S');
ti('k', 'K');
ti('K', 'k');
ti('[k]', 'K');
ti('[^ß]', 's');
ti('[^ß]+', 'ssx');
ti('(ss)', 'ß');
ti('a(ß)b', 'assb');
ti('xßy', 'XSSY');
ti('ŉ', "\u{2BC}n");
ti('ΐ', "\u{3B9}\u{308}\u{301}");
ti('[[:upper:]]', 'a');
ti('[[:lower:]]', 'A');
ti('[^[:upper:]]', 'A');
ti('\p{Lu}', 'a');
ti('[a-z&&[^c]]+', 'ABCD');
ti('[^a-c]', 'B');
ti('\W', 'a');
ti('[\W]', 'A');
ti('(a)\1', 'aA');
ti('(ß)\1', 'ßss');
ti('(ss)\1', 'ssß');
ti('st', 'ﬆ');
ti('ﬆ', 'st');
ti('ﬅ', 'st');
ti('[ﬀ-ﬆ]', 'st');
ti('ǰ', "j\u{30C}");
ti('ss', 'ẞ');
ti('ẞ', 'ss');
ti('a*ss', 'aaß');
ti('ß+', 'ssss');
ti('[sß]+', 'ssß');
ti('\x{df}', 'SS');
ti('Σ', 'ς');
ti('σ', 'Σ');
ti('ς', 'σ');
ti('[^Σ]', 'ς');
ti('[^σ]', 'ς');
ti('Ω', 'Ω');
ti('ω', 'Ω');
ti('[^Ω]', 'ω');
ti('.ss.', 'aßb');
ti('a.c', 'aßc');
ti('fi', 'ﬁ');
ti('ﬃ', 'ffi');
ti('ﬃ', 'fﬁ');
ti('ffi', 'ﬃ');
ti('(?i:ß)', 'SS');
t('(?i)ss', 'ß');
t('(?i:[^a])', 'A');
ti('[^\p{Lu}]', 'a');
ti('[^\p{Ll}]', 'A');
ti('\P{Lu}', 'A');
ti('[^[:lower:]]+', 'abcXYZ');
ti('[^\w]', 'a');
ti('[^[:alpha:]]', 'a');
ti('[^a-z]', 'K');
ti('[^a-z]', "\u{212A}");
ti('[^a-zA-Z]', 'é');
ti('[^é]', 'É');
ti('[^\x{e9}]', 'É');
ti('[^ǅ]', 'ǆ');
ti('[^ǅ]', 'Ǆ');
ti('ǅ', 'ǆ');
ti('ǆ', 'Ǆ');
ti('[ǅ]', 'Ǆ');
ti('ss', 'ßx');
ti('s', 'ß');
ti('[s]', 'ß');
ti('[s]s', 'ß');
ti('^.$', 'ß');
ti('x[^ss]y', 'xßy');
ti('ab|ss', 'ß');
ti('(?:s)s', 'ß');
ti('s{2}', 'ß');
ti('s+', 'ß');
ti('[a-z]+', 'ßſK');
ti('[s-t]', 'ſ');
ti('[^s-t]', 'ſ');
--EXPECT--
["[^a]","A",null,true] => false []
["[^a]","b",null,true] => true ["b"]
["[^A-Z]+","abcDEF123",null,true] => true ["123"]
["[^k]","K",null,true] => false []
["[^\\x{212A}]","k",null,true] => false []
["[^\\x{212A}]","K",null,true] => false []
["ß","ss",null,true] => true ["ss"]
["ss","ß",null,true] => true ["ß"]
["ß","SS",null,true] => true ["SS"]
["SS","ß",null,true] => true ["ß"]
["^ß$","ss",null,true] => true ["ss"]
["^ss$","ß",null,true] => true ["ß"]
["[ß]","ss",null,true] => true ["ss"]
["^[ß]$","ss",null,true] => true ["ss"]
["ﬀ","ff",null,true] => true ["ff"]
["ff","ﬀ",null,true] => true ["ﬀ"]
["^ﬀ$","FF",null,true] => true ["FF"]
["i","İ",null,true] => false []
["İ","i",null,true] => false []
["İ","i̇",null,true] => true ["i̇"]
["i̇","İ",null,true] => true ["İ"]
["ı","I",null,true] => false []
["I","ı",null,true] => false []
["ı","i",null,true] => false []
["[ı]","I",null,true] => false []
["s","ſ",null,true] => true ["ſ"]
["ſ","S",null,true] => true ["S"]
["k","K",null,true] => true ["K"]
["K","k",null,true] => true ["k"]
["[k]","K",null,true] => true ["K"]
["[^ß]","s",null,true] => true ["s"]
["[^ß]+","ssx",null,true] => true ["ssx"]
["(ss)","ß",null,true] => true ["ß","ß"]
["a(ß)b","assb",null,true] => true ["assb","ss"]
["xßy","XSSY",null,true] => true ["XSSY"]
["ŉ","ʼn",null,true] => true ["ʼn"]
["ΐ","ΐ",null,true] => true ["ΐ"]
["[[:upper:]]","a",null,true] => true ["a"]
["[[:lower:]]","A",null,true] => true ["A"]
["[^[:upper:]]","A",null,true] => false []
["\\p{Lu}","a",null,true] => false []
["[a-z&&[^c]]+","ABCD",null,true] => true ["AB"]
["[^a-c]","B",null,true] => false []
["\\W","a",null,true] => false []
["[\\W]","A",null,true] => false []
["(a)\\1","aA",null,true] => true ["aA","a"]
["(ß)\\1","ßss",null,true] => false []
["(ss)\\1","ssß",null,true] => false []
["st","ﬆ",null,true] => true ["ﬆ"]
["ﬆ","st",null,true] => true ["st"]
["ﬅ","st",null,true] => true ["st"]
["[ﬀ-ﬆ]","st",null,true] => true ["st"]
["ǰ","ǰ",null,true] => true ["ǰ"]
["ss","ẞ",null,true] => true ["ẞ"]
["ẞ","ss",null,true] => true ["ss"]
["a*ss","aaß",null,true] => true ["aaß"]
["ß+","ssss",null,true] => true ["ssss"]
["[sß]+","ssß",null,true] => true ["ssß"]
["\\x{df}","SS",null,true] => true ["SS"]
["Σ","ς",null,true] => true ["ς"]
["σ","Σ",null,true] => true ["Σ"]
["ς","σ",null,true] => true ["σ"]
["[^Σ]","ς",null,true] => false []
["[^σ]","ς",null,true] => false []
["Ω","Ω",null,true] => true ["Ω"]
["ω","Ω",null,true] => true ["Ω"]
["[^Ω]","ω",null,true] => false []
[".ss.","aßb",null,true] => true ["aßb"]
["a.c","aßc",null,true] => true ["aßc"]
["fi","ﬁ",null,true] => true ["ﬁ"]
["ﬃ","ffi",null,true] => true ["ffi"]
["ﬃ","fﬁ",null,true] => false []
["ffi","ﬃ",null,true] => true ["ﬃ"]
["(?i:ß)","SS",null,true] => true ["SS"]
["(?i)ss","ß",null,false] => true ["ß"]
["(?i:[^a])","A",null,false] => false []
["[^\\p{Lu}]","a",null,true] => false []
["[^\\p{Ll}]","A",null,true] => false []
["\\P{Lu}","A",null,true] => false []
["[^[:lower:]]+","abcXYZ",null,true] => false []
["[^\\w]","a",null,true] => false []
["[^[:alpha:]]","a",null,true] => false []
["[^a-z]","K",null,true] => false []
["[^a-z]","K",null,true] => false []
["[^a-zA-Z]","é",null,true] => true ["é"]
["[^é]","É",null,true] => false []
["[^\\x{e9}]","É",null,true] => false []
["[^ǅ]","ǆ",null,true] => false []
["[^ǅ]","Ǆ",null,true] => false []
["ǅ","ǆ",null,true] => true ["ǆ"]
["ǆ","Ǆ",null,true] => true ["Ǆ"]
["[ǅ]","Ǆ",null,true] => true ["Ǆ"]
["ss","ßx",null,true] => true ["ß"]
["s","ß",null,true] => false []
["[s]","ß",null,true] => false []
["[s]s","ß",null,true] => false []
["^.$","ß",null,true] => true ["ß"]
["x[^ss]y","xßy",null,true] => true ["xßy"]
["ab|ss","ß",null,true] => true ["ß"]
["(?:s)s","ß",null,true] => true ["ß"]
["s{2}","ß",null,true] => false []
["s+","ß",null,true] => false []
["[a-z]+","ßſK",null,true] => true ["ſK"]
["[s-t]","ſ",null,true] => true ["ſ"]
["[^s-t]","ſ",null,true] => false []
