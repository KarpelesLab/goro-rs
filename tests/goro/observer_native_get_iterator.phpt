--TEST--
Observer: foreach over a native aggregate (ce->get_iterator) makes no calls; PDO driver method deprecations come first
--EXTENSIONS--
zend_test
pdo_sqlite
date
--INI--
zend_test.observer.enabled=1
zend_test.observer.show_output=1
zend_test.observer.observe_all=1
--FILE--
<?php
$db = new PDO('sqlite::memory:');
foreach ($db->query('SELECT 1') as $row) { echo "row\n"; }
$it = $db->query('SELECT 2')->getIterator();
foreach ($it as $row) { echo "row2\n"; }
foreach (new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), new DateTimeImmutable('2020-01-02')) as $d) { echo "d\n"; }
$db->sqliteCreateCollation('c', 'strcmp');
?>
--EXPECTF--
<!-- init '%s' -->
<file '%s'>
  <!-- init PDO::__construct() -->
  <PDO::__construct>
  </PDO::__construct>
  <!-- init PDO::query() -->
  <PDO::query>
  </PDO::query>
row
  <PDO::query>
  </PDO::query>
  <!-- init PDOStatement::getIterator() -->
  <PDOStatement::getIterator>
  </PDOStatement::getIterator>
  <!-- init InternalIterator::rewind() -->
  <InternalIterator::rewind>
  </InternalIterator::rewind>
  <!-- init InternalIterator::valid() -->
  <InternalIterator::valid>
  </InternalIterator::valid>
  <!-- init InternalIterator::current() -->
  <InternalIterator::current>
  </InternalIterator::current>
row2
  <!-- init InternalIterator::next() -->
  <InternalIterator::next>
  </InternalIterator::next>
  <InternalIterator::valid>
  </InternalIterator::valid>
  <!-- init DateTime::__construct() -->
  <DateTime::__construct>
  </DateTime::__construct>
  <!-- init DateInterval::__construct() -->
  <DateInterval::__construct>
  </DateInterval::__construct>
  <!-- init DateTimeImmutable::__construct() -->
  <DateTimeImmutable::__construct>
  </DateTimeImmutable::__construct>
  <!-- init DatePeriod::__construct() -->
  <DatePeriod::__construct>
  </DatePeriod::__construct>
d
  <!-- init Deprecated::__construct() -->
  <Deprecated::__construct>
  </Deprecated::__construct>

Deprecated: Method PDO::sqliteCreateCollation() is deprecated since 8.5, use Pdo\Sqlite::createCollation() instead in %s on line %d
  <!-- init PDO::sqliteCreateCollation() -->
  <PDO::sqliteCreateCollation>
  </PDO::sqliteCreateCollation>
</file '%s'>
