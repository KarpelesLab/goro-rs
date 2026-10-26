--TEST--
MessageFormatter: a spellout / ordinal argument's style names the rule set
--EXTENSIONS--
intl
--FILE--
<?php
// makeRBNF(): the trimmed style is the default rule set when it is a
// public one of the locale's rules, else the default stays
foreach (["{0,spellout,%spellout-ordinal}", "{0, spellout, %spellout-numbering-year}", "{0,spellout,%bogus}", "{0,spellout}", "{0,ordinal}"] as $p) {
    echo $p, " => ", MessageFormatter::formatMessage("en", $p, [1999]), "\n";
}
echo MessageFormatter::formatMessage("fr", "{0,spellout,%spellout-cardinal-feminine} / {0,ordinal,%digits-ordinal-feminine}", [1]), "\n";
?>
--EXPECT--
{0,spellout,%spellout-ordinal} => one thousand nine hundred ninety-ninth
{0, spellout, %spellout-numbering-year} => nineteen ninety-nine
{0,spellout,%bogus} => one thousand nine hundred ninety-nine
{0,spellout} => one thousand nine hundred ninety-nine
{0,ordinal} => 1,999th
une / 1re
