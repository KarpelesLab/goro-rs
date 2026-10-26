--TEST--
SPL: offset*() frames of SplDoublyLinkedList / SplObjectStorage dimension accesses
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
function t($f) {
    try { $f(); } catch (Throwable $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
        $t = $e->getTrace()[0];
        echo "  ", $t['class'] ?? '', $t['type'] ?? '', $t['function'], "(", json_encode($t['args'] ?? null), ")\n";
    }
}
$s = new SplObjectStorage;
t(function () use ($s) { $s[new stdClass]; });
t(function () use ($s) { $s['x'] = 1; });
t(function () use ($s) { isset($s['x']); });
t(function () use ($s) { unset($s['x']); });
$q = new SplQueue;
t(function () use ($q) { $q[5]; });
t(function () use ($q) { $q['x'] = 1; });
t(function () use ($q) { unset($q[3]); });
$f = new SplFixedArray(1);
t(function () use ($f) { $f[5]; });
// The object offset of a failed lookup is not kept alive by a frame
$s2 = new SplObjectStorage;
try { $s2[new stdClass]; } catch (Exception $e) {}
var_dump(spl_object_id(new stdClass) < spl_object_id($e));
?>
--EXPECTF--
UnexpectedValueException: Object not found
  {closure:%s}([])
TypeError: SplObjectStorage::offsetSet(): Argument #1 ($object) must be of type object, string given
  SplObjectStorage->offsetSet(["x",1])
TypeError: SplObjectStorage::offsetExists(): Argument #1 ($object) must be of type object, string given
  SplObjectStorage->offsetExists(["x"])
TypeError: SplObjectStorage::offsetUnset(): Argument #1 ($object) must be of type object, string given
  SplObjectStorage->offsetUnset(["x"])
OutOfRangeException: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) is out of range
  SplDoublyLinkedList->offsetGet([5])
TypeError: SplDoublyLinkedList::offsetSet(): Argument #1 ($index) must be of type ?int, string given
  SplDoublyLinkedList->offsetSet(["x",1])
OutOfRangeException: SplDoublyLinkedList::offsetUnset(): Argument #1 ($index) is out of range
  SplDoublyLinkedList->offsetUnset([3])
OutOfBoundsException: Index invalid or out of range
  {closure:%s}([])
bool(true)
