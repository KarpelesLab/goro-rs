--TEST--
DateInterval microseconds survive diff()/add()/sub(); raw bytes and %g in date error messages; idate() -1
--FILE--
<?php
$a = new DateTime('2020-05-01 00:00:00.496249'); $i = $a->diff(new DateTime('2020-05-01 00:00:01'));
var_dump($i->f); echo $i->format('%F'), ' ', (clone $a)->add($i)->format('H:i:s.u'), "\n";
foreach (['.123456', '.000001', '.999999', '.5', '.496249', '.333333', '.700001'] as $us) {
    $x = new DateTimeImmutable("2021-01-01 10:00:00$us"); $y = new DateTimeImmutable('2021-01-02 11:30:15.250000');
    $d = $x->diff($y); echo $d->format('%a %h %i %s %F %f'), ' ', $x->add($d)->format('Y-m-d H:i:s.u'), ' ', $y->sub($d)->format('Y-m-d H:i:s.u'), "\n";
}
$i = new DateInterval('P1D'); $i->f = 0.1234567; echo $i->format('%F'), "\n";
var_dump(idate('U', -1), idate('Y', 0));
try { new DateTime("a\xff"); } catch (Exception $e) { echo bin2hex($e->getMessage()), "\n"; }
try { new DateInterval("P\xff"); } catch (Exception $e) { echo bin2hex($e->getMessage()), "\n"; }
$d = new DateTime("2020-01-01");
try { $d->modify("\xfe"); } catch (Throwable $e) { echo get_class($e), ' ', bin2hex($e->getMessage()), "\n"; }
var_dump(date_parse("\xff")['errors']);
try { DateTimeImmutable::createFromTimestamp(1e20); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { DateTimeImmutable::createFromTimestamp(-1e20); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { DateTime::createFromTimestamp(9.3e18); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
float(0.503751)
503751 00:00:01.000000
1 1 30 15 126544 126544 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.123456
1 1 30 15 249999 249999 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.000001
1 1 30 14 250001 250001 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.999999
1 1 30 14 750000 750000 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.500000
1 1 30 14 753751 753751 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.496249
1 1 30 14 916667 916667 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.333333
1 1 30 14 549999 549999 2021-01-02 11:30:15.250000 2021-01-01 10:00:00.700001
123456

Warning: idate(): Unrecognized date format token in %s on line 9
bool(false)
int(1970)
4661696c656420746f2070617273652074696d6520737472696e67202861ff2920617420706f736974696f6e20312028ff293a20556e657870656374656420636861726163746572
556e6b6e6f776e206f722062616420666f726d6174202850ff29
DateMalformedStringException 4461746554696d653a3a6d6f6469667928293a204661696c656420746f2070617273652074696d6520737472696e672028fe2920617420706f736974696f6e20302028fe293a20556e657870656374656420636861726163746572
array(1) {
  [0]=>
  string(20) "Unexpected character"
}
DateTimeImmutable::createFromTimestamp(): Argument #1 ($timestamp) must be a finite number between -9223372036854775808 and 9223372036854775807.999999, 1.0e+20 given
DateTimeImmutable::createFromTimestamp(): Argument #1 ($timestamp) must be a finite number between -9223372036854775808 and 9223372036854775807.999999, -1.0e+20 given
DateTime::createFromTimestamp(): Argument #1 ($timestamp) must be a finite number between -9223372036854775808 and 9223372036854775807.999999, 9.3e+18 given
