--TEST--
#[\Override] of methods of an internal interface's parent interfaces (SeekableIterator's Iterator)
--FILE--
<?php
class A implements SeekableIterator {
    public function seek(int $offset): void {}
    #[\Override] public function current(): mixed { return 1; }
    public function key(): mixed { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
echo "A ok\n";
final readonly class B implements SeekableIterator {
    public function seek(int $offset): void {}
    #[\Override] public function current(): mixed { return 1; }
    public function key(): mixed { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
echo "B ok\n";
class C implements SeekableIterator {
    public function __construct(private array $a) {}
    public function seek(int $offset): void {}
    #[\Override] public function current(): mixed { return 1; }
    public function key(): mixed { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
echo "C ok\n";

?>
--EXPECT--
A ok
B ok
C ok
