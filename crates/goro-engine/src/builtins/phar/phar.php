<?php
/* Phar and PharData (PHP manual: Phar, PharData): directory iterators over
 * the archive's phar:// root, with the archive methods native
 * (classes.rs). `__h_*` methods are hidden helpers. */
class Phar extends RecursiveDirectoryIterator implements Countable, ArrayAccess {
    const BZ2 = 8192;
    const GZ = 4096;
    const NONE = 0;
    const PHAR = 1;
    const TAR = 2;
    const ZIP = 3;
    const COMPRESSED = 61440;
    const PHP = 0;
    const PHPS = 1;
    const MD5 = 1;
    const OPENSSL = 16;
    const OPENSSL_SHA256 = 17;
    const OPENSSL_SHA512 = 18;
    const SHA1 = 2;
    const SHA256 = 3;
    const SHA512 = 4;
    public function __construct(string $filename, int $flags = FilesystemIterator::SKIP_DOTS | FilesystemIterator::UNIX_PATHS, ?string $alias = null) {
        $root = $this->__h_pharInit($filename, $alias, false, 1);
        $this->__h_open($root, $flags, static::class);
        $this->setInfoClass('PharFileInfo');
    }
    public function __destruct() {}
    protected function __h_pharInit($filename, $alias, $data, $format) {}
    public function addEmptyDir(string $directory): void {}
    public function addFile(string $filename, ?string $localName = null): void {}
    public function addFromString(string $localName, string $contents): void {}
    public function compressFiles(int $compression): void {}
    public function decompressFiles(): bool {}
    public function compress(int $compression, ?string $extension = null): ?Phar {}
    public function decompress(?string $extension = null): ?Phar {}
    public function convertToExecutable(?int $format = null, ?int $compression = null, ?string $extension = null): ?Phar {}
    public function convertToData(?int $format = null, ?int $compression = null, ?string $extension = null): ?PharData {}
    public function copy(string $from, string $to): bool {}
    public function count(int $mode = COUNT_NORMAL): int {}
    public function delete(string $localName): bool {}
    public function delMetadata(): bool {}
    public function extractTo(string $directory, array|string|null $files = null, bool $overwrite = false): bool {}
    public function getAlias(): ?string {}
    public function getPath(): string {}
    public function getMetadata(array $unserializeOptions = []): mixed {}
    public function getModified(): bool {}
    public function getSignature(): array|false {}
    public function getStub(): string {}
    public function getVersion(): string {}
    public function hasMetadata(): bool {}
    public function isBuffering(): bool {}
    public function isCompressed(): int|false {}
    public function isFileFormat(int $format): bool {}
    public function isWritable(): bool {}
    public function offsetExists($localName): bool {}
    public function offsetGet($localName): SplFileInfo {}
    public function offsetSet($localName, $value): void {}
    public function offsetUnset($localName): void {}
    public function setAlias(string $alias): bool {}
    public function setDefaultStub(?string $index = null, ?string $webIndex = null): bool {}
    public function setMetadata(mixed $metadata): void {}
    public function setSignatureAlgorithm(int $algo, ?string $privateKey = null): void {}
    public function setStub($stub, int $length = -1): bool {}
    public function startBuffering(): void {}
    public function stopBuffering(): void {}
    final public static function apiVersion(): string {}
    final public static function canCompress(int $compression = 0): bool {}
    final public static function canWrite(): bool {}
    final public static function createDefaultStub(?string $index = null, ?string $webIndex = null): string {}
    final public static function getSupportedCompression(): array {}
    final public static function getSupportedSignatures(): array {}
    final public static function interceptFileFuncs(): void {}
    final public static function isValidPharFilename(string $filename, bool $executable = true): bool {}
    final public static function loadPhar(string $filename, ?string $alias = null): bool {}
    final public static function mapPhar(?string $alias = null, int $offset = 0): bool {}
    final public static function running(bool $returnPhar = true): string {}
    final public static function mount(string $pharPath, string $externalPath): void {}
    final public static function mungServer(array $variables): void {}
    final public static function unlinkArchive(string $filename): bool {}
    final public static function webPhar(?string $alias = null, ?string $index = null, ?string $fileNotFoundScript = null, array $mimeTypes = [], ?callable $rewrite = null): void {}
