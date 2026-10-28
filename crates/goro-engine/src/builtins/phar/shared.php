    /* buildFromIterator()/buildFromDirectory() (PHP manual): the iterator
     * is walked by hand, the value checked before the key is asked for
     * (tests phar_buildfromiterator*.phpt). */
    public function buildFromIterator(Traversable $iterator, ?string $baseDirectory = null): array {
        $this->__h_checkWrite();
        return $this->__h_build($iterator, $baseDirectory, get_class($iterator));
    }
    public function buildFromDirectory(string $directory, string $pattern = ""): array {
        $this->__h_checkWrite();
        $it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($directory, FilesystemIterator::SKIP_DOTS));
        if ($pattern !== '') {
            $it = new RegexIterator($it, $pattern);
        }
        return $this->__h_build($it, $directory, 'RecursiveIteratorIterator');
    }
    private function __h_build($iterator, $base, $cls) {
        while ($iterator instanceof IteratorAggregate) {
            $iterator = $iterator->getIterator();
        }
        $out = [];
        $realBase = null;
        if ($base !== null) {
            $realBase = realpath($base);
            if ($realBase === false) {
                $realBase = $base;
            }
            $realBase = rtrim($realBase, '/');
        }
        $iterator->rewind();
        while ($iterator->valid()) {
            $value = $iterator->current();
            if (is_string($value)) {
                $key = $iterator->key();
                if (!is_string($key)) {
                    throw new UnexpectedValueException("Iterator $cls returned an invalid key (must return a string)");
                }
                $name = $key;
                $path = $value;
            } elseif (is_resource($value)) {
                $key = $iterator->key();
                if (!is_string($key)) {
                    throw new UnexpectedValueException("Iterator $cls returned an invalid key (must return a string)");
                }
                if ($this->__h_addData($key, stream_get_contents($value))) {
                    $out[$key] = '[stream]';
                }
                $iterator->next();
                continue;
            } elseif ($value instanceof SplFileInfo) {
                if ($base === null) {
                    throw new BadMethodCallException("Iterator $cls returns an SplFileInfo object, so base directory must be specified");
                }
                $path = $value->getPathname();
                $fn = $value->getFilename();
                if ($fn === '.' || $fn === '..' || is_dir($path)) {
                    $iterator->next();
                    continue;
                }
                $real = realpath($path);
                if ($real === false) {
                    $real = $path;
                }
                if (!str_starts_with($real, $realBase . '/')) {
                    throw new UnexpectedValueException("Iterator $cls returned a path \"$real\" that is not in the base directory \"$realBase\"");
                }
                $name = substr($real, strlen($realBase) + 1);
            } else {
                throw new UnexpectedValueException("Iterator $cls returned an invalid value (must return a string, a stream, or an SplFileInfo object)");
            }
            $r = $this->__h_addFile($name, $path);
            if ($r === false) {
                throw new UnexpectedValueException("Iterator $cls returned a file that could not be opened \"$path\"");
            }
            if ($r !== null) {
                $out[$name] = $r;
            }
            $iterator->next();
        }
        if ($out) {
            $this->__h_save();
        }
        return $out;
    }
    private function __h_checkWrite() {}
    private function __h_addFile($name, $path) {}
    private function __h_addData($name, $data) {}
    private function __h_save() {}
}
