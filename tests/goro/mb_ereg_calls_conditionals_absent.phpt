--TEST--
mb_ereg(): \g<> calls/recursion, (?(cond)..), absent operator, \X \y \Y, Unicode properties
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
// subexpression calls
t('(?<p>\((?:[^()]|\g<p>)*\))', 'x(a(b)c)y');
t('\A(?<a>|.|(?:(?<b>.)\g<a>\k<b+0>))\z', 'abcba');
t('\A(?<a>|.|(?:(?<b>.)\g<a>\k<b+0>))\z', 'abcda');
t('(a|b\g<1>c)', 'bbacc');
t('(?<n>a)\g<n>\g<n>', 'aaa');
t('\g<1>(a)', 'aa');
t('\g<0>', 'a');
t('(?:a|\g<0>b)', 'ab');
t('a\g<0>?b', 'aabb');
t('(a)\g<-1>', 'aa');
t('(a)\g<+1>(b)', 'abb');
t('\g<2>(a)', 'a');
t('(?<n>a)(?<n>b)\g<n>', 'aba');
t('(?<x>.)\g<y>', 'ab');
t('(?<x>.)\g<x', 'ab');
t('(.)\g<1>{0}', 'a');
t('(?<a>\g<a>)', 'a');
t('(?<a>a\g<a>?)', 'aaa');
t('(?<a>a|\g<a>)', 'a');
t('(?<n>[a-z]+)=\g<n>', 'ab=cd');
t('\A(?<paren>\(\g<paren>*\))*\z', '(())()');
t('(?<a>x)(?<b>\g<a>)', 'xx');
// conditionals
t('(a)?(?(1)b|c)', 'ab');
t('(a)?(?(1)b|c)', 'c');
t('(a)?(?(1)b|c)', 'b');
t('(?<n>a)?(?(<n>)b|c)', 'ab');
t('(?<n>a)?(?(\'n\')b|c)', 'c');
t('(a)?(?(1)b)', 'b');
t('(a)?(?(1))', 'a');
t('(?(1)a)', 'a');
t('(a)(?(2)b|c)', 'ac');
t('(?(?=a)ab|cd)', 'cd');
t('(?(?=a)ab|cd)', 'ab');
t('(?(a)b|c)', 'ab');
t('(?(a)b|c|d)', 'd');
t('(a)(?(1)b|c|d)', 'ad');
t('(?(x)', 'x');
t('(a)?(?(1)|c)', 'c');
t('(?(1-0)a)', 'a');
t('(a)(?(-1)b)', 'ab');
t('(a)(?(+1)b)(c)', 'abc');
// absent
t('(?~abc)', 'abxabc');
t('(?~)', 'abc');
t('(?~a)', 'a');
t('/\*(?~\*/)\*/', 'x /* comment */ y */');
t('(?~|abc|.*)', 'xxabcyy');
t('(?~|abc)', 'xxabcyy');
t('(?~|)', 'abc');
t('a(?~|b)c', 'ac');
t('(?~|a|b+)', 'bbb');
t('(?~|ab|\w+)', 'xyabz');
t('(?~a|b)', 'xyz');
t('(?~(a)b)', 'xab');
t('(?~\d+)', '12a');
t('(?~a*)', 'aaa');
t('(?~|a)*', 'b');
t('x(?~)y', 'xy');
// text segments
t('\X', "e\u{301}x");
t('\X+', "a\r\nb");
t('^\X$', "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
t('^\X\X$', "\u{1F1EF}\u{1F1F5}\u{1F1FA}\u{1F1F8}");
t('\X', "\u{AC00}\u{11A8}");
t('a\yb', "ab");
t('\y', "");
t('\Y', "");
t('a\Yb', "ab");
t("e\\Y\u{301}", "e\u{301}");
t('(?y{g})\y.', "ab");
t('(?y{w})\yab\y', "ab cd");
t('\X{2}', "\u{0915}\u{094D}\u{0937}a");
t('\O', "\n");
t('\N', "\n");
t('\R', "\n");
t('\R', "\u{2028}");
t('\R{2}', "\r\n\n");
t('a\Kb', 'ab');
t('(?<=a\K)b', 'ab');
// properties
foreach (['Alpha','Lu','L','Greek','Han','Hiragana','Katakana','Latin','Cyrillic','Arabic','Hebrew','Thai','Hangul','Common','Inherited','Emoji','Emoji_Presentation','Extended_Pictographic','In_Basic_Latin','In_CJK_Unified_Ideographs','Any','Assigned','ASCII','Word','Punct','Sc','Sm','Nd','Mn','Zs','Cc','Cf','Co','Cn','Cs','Lt','Lm','Lo','Me','Mc','Nl','No','Pc','Pd','Ps','Pe','Pi','Pf','Po','Sk','So','Zl','Zp','White_Space','Dash','Hex_Digit','Ideographic','Math','Lowercase','Uppercase','Cased','Alnum','Graph','Print','XDigit','Newline','Blank','Space','Cntrl','Digit','Upper','Lower','Devanagari','Bengali','Tamil','Ethiopic','Armenian','Georgian','Mongolian','Tibetan','Braille','Gothic','Ogham','Runic','Cherokee','Khmer','Lao','Myanmar','Sinhala','Syriac','Thaana','Yi','Bopomofo','Coptic','Unknown','Zzzz','Latn','Grek','Cyrl','Age=6.0','In_No_Block','Bidi_Control','Join_Control','Variation_Selector','Regional_Indicator','Grapheme_Base','Grapheme_Extend','ID_Start','ID_Continue','XID_Start','Default_Ignorable_Code_Point','Soft_Dotted','Diacritic','Extender','Radical','Unified_Ideograph','Noncharacter_Code_Point','Deprecated','Pattern_Syntax','Pattern_White_Space','Changes_When_Lowercased','Emoji_Modifier','Emoji_Component','Kana','Hira','Hani','Arab','Hebr','Foo','L&','LC','Combining_Mark','Letter','Other','Separator','Symbol','Number','Mark'] as $prop) {
  t('\p{'.$prop.'}+', "abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t\u{1F600}\u{0301}\u{E000}");
}
t('\p{^Alpha}+', 'ab12');
t('\P{^Alpha}+', 'ab12');
t('[\p{Greek}\d]+', 'αβ12x');
t('\p{alpha}', 'a');
t('\p{ ALPHA }', 'a');
t('\p{Al_pha}', 'a');
t('\p{al-pha}', 'a');
t('\p{Alpha', 'a');
t('\p{}', 'a');
t('\pL', 'a');
--EXPECT--
["(?<p>\\((?:[^()]|\\g<p>)*\\))","x(a(b)c)y",null,false] => true {"0":"(a(b)c)","1":"(a(b)c)","p":"(a(b)c)"}
["\\A(?<a>|.|(?:(?<b>.)\\g<a>\\k<b+0>))\\z","abcba",null,false] => true {"0":"abcba","1":"abcba","2":"b","a":"abcba","b":"b"}
["\\A(?<a>|.|(?:(?<b>.)\\g<a>\\k<b+0>))\\z","abcda",null,false] => false []
["(a|b\\g<1>c)","bbacc",null,false] => true ["bbacc","bbacc"]
["(?<n>a)\\g<n>\\g<n>","aaa",null,false] => true {"0":"aaa","1":"a","n":"a"}
["\\g<1>(a)","aa",null,false] => true ["aa","a"]
  ERR: mb_ereg(): mbregex compile err: never ending recursion
["\\g<0>","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: never ending recursion
["(?:a|\\g<0>b)","ab",null,false] => false []
["a\\g<0>?b","aabb",null,false] => true ["aabb"]
["(a)\\g<-1>","aa",null,false] => true ["aa","a"]
["(a)\\g<+1>(b)","abb",null,false] => true ["abb","a","b"]
  ERR: mb_ereg(): mbregex compile err: undefined group <2> reference
["\\g<2>(a)","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: multiplex definition name <n> call
["(?<n>a)(?<n>b)\\g<n>","aba",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined name <y> reference
["(?<x>.)\\g<y>","ab",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <x>
["(?<x>.)\\g<x","ab",null,false] => false []
["(.)\\g<1>{0}","a",null,false] => true ["a","a"]
  ERR: mb_ereg(): mbregex compile err: never ending recursion
["(?<a>\\g<a>)","a",null,false] => false []
["(?<a>a\\g<a>?)","aaa",null,false] => true {"0":"aaa","1":"aaa","a":"aaa"}
  ERR: mb_ereg(): mbregex compile err: never ending recursion
["(?<a>a|\\g<a>)","a",null,false] => false []
["(?<n>[a-z]+)=\\g<n>","ab=cd",null,false] => true {"0":"ab=cd","1":"cd","n":"cd"}
["\\A(?<paren>\\(\\g<paren>*\\))*\\z","(())()",null,false] => true {"0":"(())()","1":"()","paren":"()"}
["(?<a>x)(?<b>\\g<a>)","xx",null,false] => true {"0":"xx","1":"x","2":"x","a":"x","b":"x"}
["(a)?(?(1)b|c)","ab",null,false] => true ["ab","a"]
["(a)?(?(1)b|c)","c",null,false] => true ["c",false]
["(a)?(?(1)b|c)","b",null,false] => false []
["(?<n>a)?(?(<n>)b|c)","ab",null,false] => true {"0":"ab","1":"a","n":"a"}
["(?<n>a)?(?('n')b|c)","c",null,false] => true {"0":"c","1":false,"n":false}
["(a)?(?(1)b)","b",null,false] => true [false,false]
["(a)?(?(1))","a",null,false] => true ["a","a"]
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(?(1)a)","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(a)(?(2)b|c)","ac",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?(?=a)ab|cd)","cd",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: target of repeat operator is not specified
["(?(?=a)ab|cd)","ab",null,false] => false []
["(?(a)b|c)","ab",null,false] => true ["ab"]
["(?(a)b|c|d)","d",null,false] => true ["d"]
["(a)(?(1)b|c|d)","ad",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: end pattern in group
["(?(x)","x",null,false] => false []
["(a)?(?(1)|c)","c",null,false] => true ["c",false]
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(?(1-0)a)","a",null,false] => false []
["(a)(?(-1)b)","ab",null,false] => true ["ab","a"]
["(a)(?(+1)b)(c)","abc",null,false] => false []
["(?~abc)","abxabc",null,false] => true ["abx"]
["(?~)","abc",null,false] => true [false]
["(?~a)","a",null,false] => true [false]
["\/\\*(?~\\*\/)\\*\/","x \/* comment *\/ y *\/",null,false] => true ["\/* comment *\/"]
["(?~|abc|.*)","xxabcyy",null,false] => true ["xx"]
["(?~|abc)","xxabcyy",null,false] => true [false]
["(?~|)","abc",null,false] => true [false]
["a(?~|b)c","ac",null,false] => true ["ac"]
["(?~|a|b+)","bbb",null,false] => true ["bbb"]
["(?~|ab|\\w+)","xyabz",null,false] => true ["xy"]
["(?~a|b)","xyz",null,false] => true ["xyz"]
["(?~(a)b)","xab",null,false] => true ["x",false]
["(?~\\d+)","12a",null,false] => true [false]
["(?~a*)","aaa",null,false] => true [false]
["(?~|a)*","b",null,false] => true [false]
["x(?~)y","xy",null,false] => true ["xy"]
["\\X","éx",null,false] => true ["é"]
["\\X+","a\r\nb",null,false] => true ["a\r\nb"]
["^\\X$","👨‍👩‍👧",null,false] => true ["👨‍👩‍👧"]
["^\\X\\X$","🇯🇵🇺🇸",null,false] => true ["🇯🇵🇺🇸"]
["\\X","각",null,false] => true ["각"]
["a\\yb","ab",null,false] => true ["ab"]
["\\y","",null,false] => true [false]
["\\Y","",null,false] => false []
["a\\Yb","ab",null,false] => false []
["e\\Ý","é",null,false] => true ["é"]
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?y{g})\\y.","ab",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined group option
["(?y{w})\\yab\\y","ab cd",null,false] => false []
["\\X{2}","क्षa",null,false] => true ["क्ष"]
["\\O","\n",null,false] => false []
["\\N","\n",null,false] => false []
["\\R","\n",null,false] => true ["\n"]
["\\R","\u2028",null,false] => true ["\u2028"]
["\\R{2}","\r\n\n",null,false] => true ["\r\n\n"]
["a\\Kb","ab",null,false] => true ["b"]
["(?<=a\\K)b","ab",null,false] => true ["b"]
["\\p{Alpha}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย"]
["\\p{Lu}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖΩ"]
["\\p{L}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย"]
["\\p{Greek}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["Ω"]
["\\p{Han}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本"]
["\\p{Hiragana}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ひら"]
["\\p{Katakana}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["カタ"]
["\\p{Latin}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Cyrillic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖ"]
["\\p{Arabic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["١٢٣"]
["\\p{Hebrew}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ש"]
["\\p{Thai}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ไทย"]
["\\p{Hangul}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["한글"]
["\\p{Common}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["€+ \t😀"]
["\\p{Inherited}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{Emoji}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["😀"]
["\\p{Emoji_Presentation}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["😀"]
["\\p{Extended_Pictographic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["😀"]
["\\p{In_Basic_Latin}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{In_CJK_Unified_Ideographs}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本"]
["\\p{Any}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́"]
["\\p{Assigned}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́"]
["\\p{ASCII}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Word}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש"]
["\\p{Punct}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Sc}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["€"]
["\\p{Sm}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["+"]
["\\p{Nd}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["١٢٣"]
["\\p{Mn}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{Zs}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" "]
["\\p{Cc}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["\t"]
["\\p{Cf}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Co}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [""]
["\\p{Cn}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Cs}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Lt}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Lm}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Lo}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本ひらカタ한글ไทย"]
["\\p{Me}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Mc}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Nl}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{No}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pc}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pd}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Ps}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pe}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pi}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pf}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Po}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Sk}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{So}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["😀"]
["\\p{Zl}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Zp}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{White_Space}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" \t"]
["\\p{Dash}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Hex_Digit}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Ideographic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本"]
["\\p{Math}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["+"]
["\\p{Lowercase}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Uppercase}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖΩ"]
["\\p{Cased}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ"]
["\\p{Alnum}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש"]
["\\p{Graph}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+"]
["\\p{Print}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ "]
["\\p{XDigit}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Newline}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Blank}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" \t"]
["\\p{Space}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" \t"]
["\\p{Cntrl}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["\t"]
["\\p{Digit}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["١٢٣"]
["\\p{Upper}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖΩ"]
["\\p{Lower}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Devanagari}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Bengali}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Tamil}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Ethiopic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Armenian}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Georgian}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Mongolian}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Tibetan}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Braille}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Gothic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Ogham}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Runic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Cherokee}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Khmer}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Lao}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Myanmar}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Sinhala}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Syriac}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Thaana}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Yi}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Bopomofo}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Coptic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Unknown}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [""]
["\\p{Zzzz}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [""]
["\\p{Latn}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abc"]
["\\p{Grek}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["Ω"]
["\\p{Cyrl}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖ"]
  ERR: mb_ereg(): mbregex compile err: invalid character property name {Age=6.0}
["\\p{Age=6.0}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{In_No_Block}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Bidi_Control}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Join_Control}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Variation_Selector}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Regional_Indicator}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Grapheme_Base}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ "]
["\\p{Grapheme_Extend}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{ID_Start}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย"]
["\\p{ID_Continue}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย١٢٣ש"]
["\\p{XID_Start}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย"]
["\\p{Default_Ignorable_Code_Point}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Soft_Dotted}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Diacritic}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{Extender}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Radical}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Unified_Ideograph}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本"]
["\\p{Noncharacter_Code_Point}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Deprecated}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Pattern_Syntax}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["+"]
["\\p{Pattern_White_Space}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" \t"]
["\\p{Changes_When_Lowercased}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ДЖΩ"]
["\\p{Emoji_Modifier}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Emoji_Component}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{Kana}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["カタ"]
["\\p{Hira}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ひら"]
["\\p{Hani}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["日本"]
["\\p{Arab}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["١٢٣"]
["\\p{Hebr}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["ש"]
  ERR: mb_ereg(): mbregex compile err: invalid character property name {Foo}
["\\p{Foo}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid character property name {L&}
["\\p{L&}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => false []
["\\p{LC}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ"]
["\\p{Combining_Mark}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{Letter}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["abcДЖΩ日本ひらカタ한글ไทย"]
["\\p{Other}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["\t"]
["\\p{Separator}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true [" "]
["\\p{Symbol}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["€+"]
["\\p{Number}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["١٢٣"]
["\\p{Mark}+","abcДЖΩ日本ひらカタ한글ไทย١٢٣ש€+ \t😀́",null,false] => true ["́"]
["\\p{^Alpha}+","ab12",null,false] => true ["12"]
["\\P{^Alpha}+","ab12",null,false] => true ["ab"]
["[\\p{Greek}\\d]+","αβ12x",null,false] => true ["αβ12"]
["\\p{alpha}","a",null,false] => true ["a"]
["\\p{ ALPHA }","a",null,false] => true ["a"]
["\\p{Al_pha}","a",null,false] => true ["a"]
["\\p{al-pha}","a",null,false] => true ["a"]
  ERR: mb_ereg(): mbregex compile err: end pattern with unmatched parenthesis
["\\p{Alpha","a",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid character property name {}
["\\p{}","a",null,false] => false []
["\\pL","a",null,false] => false []
