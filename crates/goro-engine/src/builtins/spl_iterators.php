<?php
/* SPL iterators written in PHP, from the PHP manual's class synopses and
 * descriptions (SPL Iterators chapter); behaviour checked against the
 * ext/spl .phpt expectations and php85 output.
 *
 * Conventions of internal classes written in PHP: properties named `__*`
 * are hidden internal state; methods named `__h_*` are helpers hidden from
 * backtraces and method listings. */

class IteratorIterator implements OuterIterator {
    protected $__it = null;
    protected $__valid = false;
    protected $__key = null;
    protected $__cur = null;
    public function __construct(Traversable $iterator, ?string $class = null) {
        while ($iterator instanceof IteratorAggregate) {
            $agg = $iterator;
            $iterator = $iterator->getIterator();
            if (!$iterator instanceof Traversable) {
                // observed on PHP 8.5
                throw new LogicException(get_class($agg) . '::getIterator() must return an object that implements Traversable');
            }
        }
        $this->__it = $iterator;
    }
    /* Unknown methods are the inner iterator's (observed on PHP 8.5). */
    protected function __h_call($name, $args) {
        if ($this->__it === null || !method_exists($this->__it, $name)) {
            throw new Error('Call to undefined method ' . static::class . '::' . $name . '()');
        }
        return $this->__it->$name(...$args);
    }
    protected function __h_chk() {
        if ($this->__it === null) {
            throw new LogicException("The object is in an invalid state as the parent constructor was not called");
        }
    }
    protected function __h_fetch() {
        $this->__cur = null;
        $this->__key = null;
        if ($this->__valid = $this->__it->valid()) {
            $this->__cur = $this->__it->current();
            $this->__key = $this->__it->key();
        }
        return $this->__valid;
    }
    public function getInnerIterator(): ?Iterator { $this->__h_chk(); return $this->__it; }
    public function rewind(): void { $this->__h_chk(); $this->__it->rewind(); $this->__h_fetch(); }
    public function valid(): bool { $this->__h_chk(); return $this->__valid; }
    public function key(): mixed { $this->__h_chk(); return $this->__key; }
    public function current(): mixed { $this->__h_chk(); return $this->__cur; }
    public function next(): void { $this->__h_chk(); $this->__it->next(); $this->__h_fetch(); }
}

abstract class FilterIterator extends IteratorIterator {
    abstract public function accept(): bool;
    public function __construct(Iterator $iterator) { parent::__construct($iterator); }
    protected function __h_skip() {
        while ($this->__h_fetch() && !$this->accept()) {
            $this->__it->next();
        }
    }
    public function rewind(): void { $this->__h_chk(); $this->__it->rewind(); $this->__h_skip(); }
    public function next(): void { $this->__h_chk(); $this->__it->next(); $this->__h_skip(); }
}

class CallbackFilterIterator extends FilterIterator {
    protected $__cb = null;
    public function __construct(Iterator $iterator, callable $callback) {
        parent::__construct($iterator);
        $this->__cb = $callback;
    }
    public function accept(): bool {
        $this->__h_chk();
        return (bool) ($this->__cb)($this->__cur, $this->__key, $this->__it);
    }
}

abstract class RecursiveFilterIterator extends FilterIterator implements RecursiveIterator {
    public function __construct(RecursiveIterator $iterator) { parent::__construct($iterator); }
    public function hasChildren(): bool { $this->__h_chk(); return $this->__it->hasChildren(); }
    public function getChildren(): ?RecursiveFilterIterator {
        $this->__h_chk();
        return new static($this->__it->getChildren());
    }
}

class ParentIterator extends RecursiveFilterIterator {
    public function accept(): bool { $this->__h_chk(); return $this->__it->hasChildren(); }
}

class RecursiveCallbackFilterIterator extends CallbackFilterIterator implements RecursiveIterator {
    public function __construct(RecursiveIterator $iterator, callable $callback) {
        parent::__construct($iterator, $callback);
    }
    public function hasChildren(): bool { $this->__h_chk(); return $this->__it->hasChildren(); }
    public function getChildren(): RecursiveCallbackFilterIterator {
        $this->__h_chk();
        return new static($this->__it->getChildren(), $this->__cb);
    }
}

class LimitIterator extends IteratorIterator {
    protected $__off = 0;
    protected $__lim = -1;
    protected $__pos = 0;
    public function __construct(Iterator $iterator, int $offset = 0, int $limit = -1) {
        if ($offset < 0) {
            throw new ValueError('LimitIterator::__construct(): Argument #2 ($offset) must be greater than or equal to 0');
        }
        if ($limit < -1) {
            throw new ValueError('LimitIterator::__construct(): Argument #3 ($limit) must be greater than or equal to -1');
        }
        parent::__construct($iterator);
        $this->__off = $offset;
        $this->__lim = $limit;
    }
    protected function __h_seek(int $offset) {
        if ($offset < $this->__off) {
            throw new OutOfBoundsException("Cannot seek to $offset which is below the offset {$this->__off}");
        }
        if ($this->__lim != -1 && $offset >= $this->__off + $this->__lim) {
            throw new OutOfBoundsException("Cannot seek to $offset which is behind offset {$this->__off} plus count {$this->__lim}");
        }
        if ($offset != $this->__pos && $this->__it instanceof SeekableIterator) {
            $this->__it->seek($offset);
            $this->__pos = $offset;
            $this->__h_fetch();
        } else {
            if ($offset < $this->__pos) {
                $this->__it->rewind();
                $this->__pos = 0;
            }
            while ($this->__pos < $offset && $this->__it->valid()) {
                $this->__it->next();
                $this->__pos++;
            }
            $this->__h_fetch();
        }
        return $this->__pos;
    }
    public function rewind(): void {
        $this->__h_chk();
        $this->__it->rewind();
        $this->__pos = 0;
        $this->__h_seek($this->__off);
    }
    public function seek(int $offset): int { $this->__h_chk(); return $this->__h_seek($offset); }
    public function valid(): bool {
        $this->__h_chk();
        return ($this->__lim == -1 || $this->__pos < $this->__off + $this->__lim) && $this->__valid;
    }
    public function next(): void {
        $this->__h_chk();
        $this->__it->next();
        $this->__pos++;
        if ($this->__lim == -1 || $this->__pos < $this->__off + $this->__lim) {
            $this->__h_fetch();
        }
    }
    public function getPosition(): int { $this->__h_chk(); return $this->__pos; }
}

class CachingIterator extends IteratorIterator implements ArrayAccess, Countable, Stringable {
    const CALL_TOSTRING = 1;
    const CATCH_GET_CHILD = 16;
    const TOSTRING_USE_KEY = 2;
    const TOSTRING_USE_CURRENT = 4;
    const TOSTRING_USE_INNER = 8;
    const FULL_CACHE = 256;
    protected $__flags = 1;
    protected $__str = null;
    protected $__cache = [];
    protected $__children = null;
    public function __construct(Iterator $iterator, int $flags = CachingIterator::CALL_TOSTRING) {
        if ((($flags & 1) ? 1 : 0) + (($flags & 2) ? 1 : 0) + (($flags & 4) ? 1 : 0) + (($flags & 8) ? 1 : 0) > 1) {
            throw new ValueError(static::class . '::__construct(): Argument #2 ($flags) must contain only one of CachingIterator::CALL_TOSTRING, CachingIterator::TOSTRING_USE_KEY, CachingIterator::TOSTRING_USE_CURRENT, or CachingIterator::TOSTRING_USE_INNER');
        }
        parent::__construct($iterator);
        $this->__flags = $flags;
    }
    protected function __h_next() {
        if ($this->__h_fetch()) {
            if ($this instanceof RecursiveCachingIterator) {
                $this->__children = null;
                if ($this->__flags & 16) {
                    try {
                        if ($this->__it->hasChildren()) {
                            $this->__children = new RecursiveCachingIterator($this->__it->getChildren(), $this->__flags);
                        }
                    } catch (Exception $e) {
                        $this->__children = null;
                    }
                } elseif ($this->__it->hasChildren()) {
                    $this->__children = new RecursiveCachingIterator($this->__it->getChildren(), $this->__flags);
                }
            }
            if ($this->__flags & 256) {
                $this->__cache[$this->__key] = $this->__cur;
            }
            if ($this->__flags & 1) {
                $this->__str = (string) $this->__cur;
            }
            $this->__it->next();
        } else {
            $this->__str = null;
        }
    }
    public function rewind(): void {
        $this->__h_chk();
        $this->__it->rewind();
        $this->__cache = [];
        $this->__h_next();
    }
    public function next(): void { $this->__h_chk(); $this->__h_next(); }
    public function hasNext(): bool { $this->__h_chk(); return $this->__it->valid(); }
    public function __toString(): string {
        $this->__h_chk();
        $f = $this->__flags;
        if (!($f & 15)) {
            throw new BadMethodCallException(static::class . ' does not fetch string value (see CachingIterator::__construct)');
        }
        if ($f & 2) {
            return (string) $this->__key;
        }
        if ($f & 4) {
            return (string) $this->__cur;
        }
        if ($f & 8) {
            return (string) $this->__it;
        }
        return (string) $this->__str;
    }
    public function getFlags(): int { $this->__h_chk(); return $this->__flags; }
    public function setFlags(int $flags): void {
        $this->__h_chk();
        if ((($flags & 1) ? 1 : 0) + (($flags & 2) ? 1 : 0) + (($flags & 4) ? 1 : 0) + (($flags & 8) ? 1 : 0) > 1) {
            throw new ValueError(static::class . '::setFlags(): Argument #1 ($flags) must contain only one of CachingIterator::CALL_TOSTRING, CachingIterator::TOSTRING_USE_KEY, CachingIterator::TOSTRING_USE_CURRENT, or CachingIterator::TOSTRING_USE_INNER');
        }
        if (($this->__flags & 1) && !($flags & 1)) {
            throw new InvalidArgumentException('Unsetting flag CALL_TO_STRING is not possible');
        }
        if (($this->__flags & 8) && !($flags & 8)) {
            throw new InvalidArgumentException('Unsetting flag TOSTRING_USE_INNER is not possible');
        }
        if (($flags & 256) && !($this->__flags & 256)) {
            $this->__cache = [];
        }
        $this->__flags = $flags;
    }
    protected function __h_full() {
        if (!($this->__flags & 256)) {
            throw new BadMethodCallException(static::class . ' does not use a full cache (see CachingIterator::__construct)');
        }
    }
    public function offsetGet($key): mixed {
        $this->__h_chk();
        $this->__h_full();
        return $this->__cache[$key];
    }
    public function offsetSet($key, mixed $value): void {
        $this->__h_chk();
        $this->__h_full();
        $this->__cache[$key] = $value;
    }
    public function offsetUnset($key): void {
        $this->__h_chk();
        $this->__h_full();
        unset($this->__cache[$key]);
    }
    public function offsetExists($key): bool {
        $this->__h_chk();
        $this->__h_full();
        return isset($this->__cache[$key]);
    }
    public function getCache(): array {
        $this->__h_chk();
        $this->__h_full();
        return $this->__cache;
    }
    public function count(): int {
        $this->__h_chk();
        $this->__h_full();
        return count($this->__cache);
    }
}

class RecursiveCachingIterator extends CachingIterator implements RecursiveIterator {
    public function __construct(Iterator $iterator, int $flags = RecursiveCachingIterator::CALL_TOSTRING) {
        if (!$iterator instanceof RecursiveIterator) {
            throw new TypeError('RecursiveCachingIterator::__construct(): Argument #1 ($iterator) must be of type RecursiveIterator, ' . get_debug_type($iterator) . ' given');
        }
        parent::__construct($iterator, $flags);
    }
    public function hasChildren(): bool { $this->__h_chk(); return $this->__children !== null; }
    public function getChildren(): ?RecursiveCachingIterator { $this->__h_chk(); return $this->__children; }
}

class NoRewindIterator extends IteratorIterator {
    public function __construct(Iterator $iterator) { parent::__construct($iterator); }
    public function rewind(): void { $this->__h_chk(); }
    public function valid(): bool { $this->__h_chk(); return $this->__it->valid(); }
    public function key(): mixed { $this->__h_chk(); return $this->__it->key(); }
    public function current(): mixed { $this->__h_chk(); return $this->__it->current(); }
    public function next(): void { $this->__h_chk(); $this->__it->next(); }
}

class InfiniteIterator extends IteratorIterator {
    public function __construct(Iterator $iterator) { parent::__construct($iterator); }
    public function next(): void {
        $this->__h_chk();
        $this->__it->next();
        if (!$this->__h_fetch()) {
            $this->__it->rewind();
            $this->__h_fetch();
        }
    }
}

class AppendIterator extends IteratorIterator {
    protected $__its = null;
    public function __construct() { $this->__its = new ArrayIterator(); }
    protected function __h_chk() {
        if ($this->__its === null) {
            throw new LogicException("The object is in an invalid state as the parent constructor was not called");
        }
    }
    protected function __h_nextit() {
        $this->__it = null;
        $this->__valid = false;
        $this->__cur = null;
        $this->__key = null;
        if ($this->__its->valid()) {
            $this->__it = $this->__its->current();
            $this->__it->rewind();
            return true;
        }
        return false;
    }
    protected function __h_afetch() {
        while ($this->__it === null || !$this->__it->valid()) {
            $this->__its->next();
            if (!$this->__h_nextit()) {
                return;
            }
        }
        $this->__h_fetch();
    }
    public function append(Iterator $iterator): void {
        $this->__h_chk();
        if ($this->__its->valid() && !($this->__it !== null && $this->__it->valid())) {
            $this->__its->append($iterator);
            $this->__its->next();
        } else {
            $this->__its->append($iterator);
        }
        if ($this->__it === null || !$this->__it->valid()) {
            if (!$this->__its->valid()) {
                $this->__its->rewind();
            }
            $n = $this->__its->count();
            do {
                $this->__h_nextit();
            } while ($this->__it !== $iterator && --$n > 0);
            $this->__h_afetch();
        }
    }
    public function rewind(): void {
        $this->__h_chk();
        $this->__its->rewind();
        if ($this->__h_nextit()) {
            $this->__h_afetch();
        }
    }
    public function valid(): bool { $this->__h_chk(); return $this->__valid; }
    public function current(): mixed { $this->__h_chk(); return $this->__cur; }
    public function next(): void {
        $this->__h_chk();
        if ($this->__it !== null) {
            $this->__it->next();
        }
        $this->__h_afetch();
    }
    public function getIteratorIndex(): ?int { $this->__h_chk(); return $this->__its->key(); }
    public function getArrayIterator(): ArrayIterator { $this->__h_chk(); return $this->__its; }
    public function getInnerIterator(): ?Iterator { $this->__h_chk(); return $this->__it; }
}

class RecursiveIteratorIterator implements OuterIterator {
    const LEAVES_ONLY = 0;
    const SELF_FIRST = 1;
    const CHILD_FIRST = 2;
    const CATCH_GET_CHILD = 16;
    protected $__its = null;
    protected $__st = [];
    protected $__mode = 0;
    protected $__flags = 0;
    protected $__max = -1;
    protected $__initer = false;
    public function __construct(Traversable $iterator, int $mode = RecursiveIteratorIterator::LEAVES_ONLY, int $flags = 0) {
        if ($iterator instanceof IteratorAggregate) {
            $agg = $iterator;
            $iterator = $iterator->getIterator();
            if (!$iterator instanceof Traversable) {
                // test RecursiveIteratorIterator_invalid_aggregate
                throw new LogicException(get_class($agg) . '::getIterator() must return an object that implements Traversable');
            }
        }
        if (!$iterator instanceof RecursiveIterator) {
            throw new InvalidArgumentException("An instance of RecursiveIterator or IteratorAggregate creating it is required");
        }
        $this->__its = [$iterator];
        $this->__st = [4];
        $this->__mode = $mode;
        $this->__flags = $flags;
    }
    protected function __h_chk() {
        if ($this->__its === null) {
            throw new LogicException("The object is in an invalid state as the parent constructor was not called");
        }
    }
    protected function __h_fwd() {
        while (true) {
            $l = count($this->__its) - 1;
            $it = $this->__its[$l];
            $st = $this->__st[$l];
            $up = false;
            if ($st == 0 || $st == 4) {
                if ($st == 0) {
                    $it->next();
                }
                if ($it->valid()) {
                    $st = 1;
                } else {
                    $up = true;
                }
            }
            if (!$up) {
                if ($st == 1) {
                    if ($this->__flags & 16) {
                        try {
                            $has = $this->callHasChildren();
                        } catch (Exception $e) {
                            $has = false;
                        }
                    } else {
                        try {
                            $has = $this->callHasChildren();
                        } catch (Throwable $e) {
                            $this->__st[$l] = 0;
                            throw $e;
                        }
                    }
                    if ($has && ($this->__max == -1 || $this->__max > $l)) {
                        $this->__st[$l] = $this->__mode == 1 ? 2 : 3;
                        continue;
                    }
                    $this->__st[$l] = 0;
                    $this->nextElement();
                    return;
                }
                if ($st == 2) {
                    if ($this->__mode == 1 || $this->__mode == 2) {
                        $this->nextElement();
                    }
                    $this->__st[$l] = $this->__mode == 1 ? 3 : 0;
                    return;
                }
                if ($st == 3) {
                    if ($this->__flags & 16) {
                        try {
                            $c = $this->callGetChildren();
                        } catch (Exception $e) {
                            $this->__st[$l] = 0;
                            continue;
                        }
                    } else {
                        $c = $this->callGetChildren();
                    }
                    if (!($c instanceof RecursiveIterator)) {
                        throw new UnexpectedValueException("Objects returned by RecursiveIterator::getChildren() must implement RecursiveIterator");
                    }
                    $this->__st[$l] = $this->__mode == 2 ? 2 : 0;
                    $this->__its[] = $c;
                    $this->__st[] = 4;
                    $c->rewind();
                    $this->beginChildren();
                    continue;
                }
            }
            if ($l > 0) {
                $this->endChildren();
                array_pop($this->__its);
                array_pop($this->__st);
            } else {
                return;
            }
        }
    }
    public function rewind(): void {
        $this->__h_chk();
        while (count($this->__its) > 1) {
            array_pop($this->__its);
            array_pop($this->__st);
            $this->endChildren();
        }
        $this->__st = [4];
        $this->__its[0]->rewind();
        if (!$this->__initer) {
            $this->beginIteration();
        }
        $this->__initer = true;
        $this->__h_fwd();
    }
    public function valid(): bool {
        $this->__h_chk();
        for ($l = count($this->__its) - 1; $l >= 0; $l--) {
            if ($this->__its[$l]->valid()) {
                return true;
            }
        }
        if ($this->__initer) {
            $this->__initer = false;
            $this->endIteration();
        }
        return false;
    }
    public function key(): mixed { $this->__h_chk(); return $this->__its[count($this->__its) - 1]->key(); }
    public function current(): mixed { $this->__h_chk(); return $this->__its[count($this->__its) - 1]->current(); }
    public function next(): void { $this->__h_chk(); $this->__h_fwd(); }
    public function getDepth(): int { $this->__h_chk(); return count($this->__its) - 1; }
    public function getSubIterator(?int $level = null): ?RecursiveIterator {
        $this->__h_chk();
        if ($level === null) {
            $level = count($this->__its) - 1;
        }
        if ($level < 0 || $level >= count($this->__its)) {
            return null;
        }
        return $this->__its[$level];
    }
    public function getInnerIterator(): RecursiveIterator { $this->__h_chk(); return $this->__its[count($this->__its) - 1]; }
    /* Unknown methods are those of the current sub-iterator (observed on
     * PHP 8.5; test RecursiveDirectoryIterator_getSubPath_basic). */
    protected function __h_call($name, $args) {
        $it = $this->__its === null ? null : $this->__its[count($this->__its) - 1];
        if ($it === null || !method_exists($it, $name)) {
            throw new Error('Call to undefined method ' . static::class . '::' . $name . '()');
        }
        return $it->$name(...$args);
    }
    public function beginIteration(): void {}
    public function endIteration(): void {}
    public function callHasChildren(): bool {
        $this->__h_chk();
        return $this->__its[count($this->__its) - 1]->hasChildren();
    }
    public function callGetChildren(): ?RecursiveIterator {
        $this->__h_chk();
        return $this->__its[count($this->__its) - 1]->getChildren();
    }
    public function beginChildren(): void {}
    public function endChildren(): void {}
    public function nextElement(): void {}
    public function setMaxDepth(int $maxDepth = -1): void {
        if ($maxDepth < -1) {
            throw new ValueError('RecursiveIteratorIterator::setMaxDepth(): Argument #1 ($maxDepth) must be greater than or equal to -1');
        }
        $this->__max = $maxDepth;
    }
    public function getMaxDepth(): int|false { return $this->__max == -1 ? false : $this->__max; }
}

class RecursiveTreeIterator extends RecursiveIteratorIterator {
    const BYPASS_CURRENT = 4;
    const BYPASS_KEY = 8;
    const PREFIX_LEFT = 0;
    const PREFIX_MID_HAS_NEXT = 1;
    const PREFIX_MID_LAST = 2;
    const PREFIX_END_HAS_NEXT = 3;
    const PREFIX_END_LAST = 4;
    const PREFIX_RIGHT = 5;
    protected $__rflags = 8;
    protected $__pre = ['', '| ', '  ', '|-', '\-', ''];
    protected $__post = '';
    public function __construct($iterator, int $flags = RecursiveTreeIterator::BYPASS_KEY, int $cachingIteratorFlags = CachingIterator::CATCH_GET_CHILD, int $mode = RecursiveTreeIterator::SELF_FIRST) {
        if ($iterator instanceof IteratorAggregate) {
            $iterator = $iterator->getIterator();
        }
        parent::__construct(new RecursiveCachingIterator($iterator, $cachingIteratorFlags), $mode, $flags);
        $this->__rflags = $flags;
    }
    public function getPrefix(): string {
        $this->__h_chk();
        $s = $this->__pre[0];
        $n = count($this->__its) - 1;
        for ($l = 0; $l < $n; $l++) {
            $s .= $this->__its[$l]->hasNext() ? $this->__pre[1] : $this->__pre[2];
        }
        $s .= $this->__its[$n]->hasNext() ? $this->__pre[3] : $this->__pre[4];
        return $s . $this->__pre[5];
    }
    public function setPrefixPart(int $part, string $value): void {
        if ($part < 0 || $part > 5) {
            throw new ValueError('RecursiveTreeIterator::setPrefixPart(): Argument #1 ($part) must be a RecursiveTreeIterator::PREFIX_* constant');
        }
        $this->__pre[$part] = $value;
    }
    public function getEntry(): ?string {
        $this->__h_chk();
        $c = $this->__its[count($this->__its) - 1]->current();
        if (is_array($c)) {
            return "Array";
        }
        return (string) $c;
    }
    public function setPostfix(string $postfix): void { $this->__post = $postfix; }
    public function getPostfix(): string { $this->__h_chk(); return $this->__post; }
    public function current(): mixed {
        $this->__h_chk();
        if ($this->__rflags & 4) {
            return $this->__its[count($this->__its) - 1]->current();
        }
        $e = $this->getEntry();
        return $this->getPrefix() . $e . $this->getPostfix();
    }
    public function key(): mixed {
        $this->__h_chk();
        $k = $this->__its[count($this->__its) - 1]->key();
        if ($this->__rflags & 8) {
            return $k;
        }
        return $this->getPrefix() . $k . $this->getPostfix();
    }
}

class EmptyIterator implements Iterator {
    public function current(): never { throw new BadMethodCallException("Accessing the value of an EmptyIterator"); }
    public function next(): void {}
    public function key(): never { throw new BadMethodCallException("Accessing the key of an EmptyIterator"); }
    public function valid(): false { return false; }
    public function rewind(): void {}
}
