--TEST--
Transliterator: Latin-ASCII follows CLDR's rules; A(B) / A() / (B) IDs; '\' in rules
--EXTENSIONS--
intl
--FILE--
<?php
// Latin-ASCII only touches Latin, Common and Inherited characters (й, が and
// Devanagari keep their marks) and maps compatibility characters (½ ﬁ Ａ «)
$t = Transliterator::create("Latin-ASCII");
foreach (["½ ﬁ ＡＢＣ ſ ꜳ ⅷ Ǆ ŉ", "© « » × ÷ ‘ ’ “ ” – —", "й が हिन्दी", "Ærøskøbing Œuvre Straße", "x\u{0301} Å Ŋ ł"] as $s) {
    echo $t->transliterate($s), "\n";
}
// the usual slug chain: Lower() is Lower with an empty inverse
foreach (["Any-Latin; Latin-ASCII; Lower()", "Lower()", "(Lower)", "Upper(Lower)", "Latin-ASCII ()", "Any-Latin; Latin-ASCII; [:Nonspacing Mark:] Remove; Lower()"] as $id) {
    $tr = Transliterator::create($id);
    echo $id, " => ", $tr->id, " | ", $tr->transliterate("Ὀδυσσεύς Иванов Æsir “Straße”"), str_contains($id, ";") ? "" : " | inverse " . $tr->createInverse()?->id, "\n";
}
var_dump(transliterator_transliterate("Any-Latin; Latin-ASCII; Lower()", "Ἀθῆναι Москва Ærø"));
// a backslash in quotes is a quoted backslash
var_dump(Transliterator::createFromRules("a → '\\' ; b → c ;")->transliterate("abx"));
?>
--EXPECT--
 1/2 fi ABC s aa viii DZ 'n
(C) << >> * / ' ' " " - -
й が हिन्दी
AEroskobing OEuvre Strasse
x A N l
Any-Latin; Latin-ASCII; Lower() => Any-Latin;Latin-ASCII;Lower() | odysseus ivanov aesir "strasse"
Lower() => Lower() | ὀδυσσεύς иванов æsir “straße” | inverse (Lower)
(Lower) => (Lower) | Ὀδυσσεύς Иванов Æsir “Straße” | inverse Lower()
Upper(Lower) => Upper(Lower) | ὈΔΥΣΣΕΎΣ ИВАНОВ ÆSIR “STRASSE” | inverse Lower(Upper)
Latin-ASCII () => Latin-ASCII() | Ὀδυσσεύς Иванов AEsir "Strasse" | inverse (Latin-ASCII)
Any-Latin; Latin-ASCII; [:Nonspacing Mark:] Remove; Lower() => Any-Latin;Latin-ASCII;[:Nonspacing Mark:] Remove;Lower() | odysseus ivanov aesir "strasse"
string(19) "athenai moskva aero"
string(3) "\cx"
