//! PHP declarations of the ext/dom classes. The API surface (names,
//! signatures, property types) is the one Reflection shows on PHP 8.5;
//! properties are hooked, their hooks bound to native handlers
//! (`"class::$"` entries of the module's methods).

pub const CLASSES: &[(&str, &str)] = &[
    (
        "dom\\adjacentposition",
        r#"<?php namespace Dom;
enum AdjacentPosition: string {
    case BeforeBegin = 'beforebegin';
    case AfterBegin = 'afterbegin';
    case BeforeEnd = 'beforeend';
    case AfterEnd = 'afterend';
}"#,
    ),
    (
        "domexception",
        r#"<?php
final class DOMException extends \Exception {
    public $code = 0;
}"#,
    ),
    (
        "domparentnode",
        r#"<?php
interface DOMParentNode {
    public function append(...$nodes): void;
    public function prepend(...$nodes): void;
    public function replaceChildren(...$nodes): void;
}"#,
    ),
    (
        "dom\\parentnode",
        r#"<?php namespace Dom;
interface ParentNode {
    public function append(\Dom\Node|string ...$nodes): void;
    public function prepend(\Dom\Node|string ...$nodes): void;
    public function replaceChildren(\Dom\Node|string ...$nodes): void;
    public function querySelector(string $selectors): ?\Dom\Element;
    public function querySelectorAll(string $selectors): \Dom\NodeList;
}"#,
    ),
    (
        "domchildnode",
        r#"<?php
interface DOMChildNode {
    public function remove(): void;
    public function before(...$nodes): void;
    public function after(...$nodes): void;
    public function replaceWith(...$nodes): void;
}"#,
    ),
    (
        "dom\\childnode",
        r#"<?php namespace Dom;
interface ChildNode {
    public function remove(): void;
    public function before(\Dom\Node|string ...$nodes): void;
    public function after(\Dom\Node|string ...$nodes): void;
    public function replaceWith(\Dom\Node|string ...$nodes): void;
}"#,
    ),
    (
        "domimplementation",
        r#"<?php
class DOMImplementation {
    public function hasFeature(string $feature, string $version): bool {}
    public function createDocumentType(string $qualifiedName, string $publicId = '', string $systemId = '') {}
    public function createDocument(?string $namespace = NULL, string $qualifiedName = '', ?\DOMDocumentType $doctype = NULL): \DOMDocument {}
}"#,
    ),
    (
        "dom\\implementation",
        r#"<?php namespace Dom;
class Implementation {
    public function createDocumentType(string $qualifiedName, string $publicId, string $systemId): \Dom\DocumentType {}
    public function createDocument(?string $namespace, string $qualifiedName, ?\Dom\DocumentType $doctype = NULL): \Dom\XMLDocument {}
    public function createHTMLDocument(?string $title = NULL): \Dom\HTMLDocument {}
}"#,
    ),
    (
        "domnode",
        r#"<?php
class DOMNode {
    public const int DOCUMENT_POSITION_DISCONNECTED = 1;
    public const int DOCUMENT_POSITION_PRECEDING = 2;
    public const int DOCUMENT_POSITION_FOLLOWING = 4;
    public const int DOCUMENT_POSITION_CONTAINS = 8;
    public const int DOCUMENT_POSITION_CONTAINED_BY = 16;
    public const int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC = 32;
    public string $nodeName { get {} set {} }
    public ?string $nodeValue { get {} set {} }
    public int $nodeType { get {} set {} }
    public ?\DOMNode $parentNode { get {} set {} }
    public ?\DOMElement $parentElement { get {} set {} }
    public \DOMNodeList $childNodes { get {} set {} }
    public ?\DOMNode $firstChild { get {} set {} }
    public ?\DOMNode $lastChild { get {} set {} }
    public ?\DOMNode $previousSibling { get {} set {} }
    public ?\DOMNode $nextSibling { get {} set {} }
    public ?\DOMNamedNodeMap $attributes { get {} set {} }
    public bool $isConnected { get {} set {} }
    public ?\DOMDocument $ownerDocument { get {} set {} }
    public ?string $namespaceURI { get {} set {} }
    public string $prefix { get {} set {} }
    public ?string $localName { get {} set {} }
    public ?string $baseURI { get {} set {} }
    public string $textContent { get {} set {} }
    public function appendChild(\DOMNode $node) {}
    public function C14N(bool $exclusive = false, bool $withComments = false, ?array $xpath = NULL, ?array $nsPrefixes = NULL): string|false {}
    public function C14NFile(string $uri, bool $exclusive = false, bool $withComments = false, ?array $xpath = NULL, ?array $nsPrefixes = NULL): int|false {}
    public function cloneNode(bool $deep = false) {}
    public function getLineNo(): int {}
    public function getNodePath(): ?string {}
    public function hasAttributes(): bool {}
    public function hasChildNodes(): bool {}
    public function insertBefore(\DOMNode $node, ?\DOMNode $child = NULL) {}
    public function isDefaultNamespace(string $namespace): bool {}
    public function isSameNode(\DOMNode $otherNode): bool {}
    public function isEqualNode(?\DOMNode $otherNode): bool {}
    public function isSupported(string $feature, string $version): bool {}
    public function lookupNamespaceURI(?string $prefix): ?string {}
    public function lookupPrefix(string $namespace): ?string {}
    public function normalize(): void {}
    public function removeChild(\DOMNode $child) {}
    public function replaceChild(\DOMNode $node, \DOMNode $child) {}
    public function contains(\DOMNode|\DOMNameSpaceNode|null $other): bool {}
    public function getRootNode(?array $options = NULL): \DOMNode {}
    public function compareDocumentPosition(\DOMNode $other): int {}
    public function __sleep(): array {}
    public function __wakeup(): void {}
}"#,
    ),
    (
        "dom\\node",
        r#"<?php namespace Dom;
class Node {
    public const int DOCUMENT_POSITION_DISCONNECTED = 1;
    public const int DOCUMENT_POSITION_PRECEDING = 2;
    public const int DOCUMENT_POSITION_FOLLOWING = 4;
    public const int DOCUMENT_POSITION_CONTAINS = 8;
    public const int DOCUMENT_POSITION_CONTAINED_BY = 16;
    public const int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC = 32;
    public int $nodeType { get {} set {} }
    public string $nodeName { get {} set {} }
    public string $baseURI { get {} set {} }
    public bool $isConnected { get {} set {} }
    public ?\Dom\Document $ownerDocument { get {} set {} }
    public ?\Dom\Node $parentNode { get {} set {} }
    public ?\Dom\Element $parentElement { get {} set {} }
    public \Dom\NodeList $childNodes { get {} set {} }
    public ?\Dom\Node $firstChild { get {} set {} }
    public ?\Dom\Node $lastChild { get {} set {} }
    public ?\Dom\Node $previousSibling { get {} set {} }
    public ?\Dom\Node $nextSibling { get {} set {} }
    public ?string $nodeValue { get {} set {} }
    public ?string $textContent { get {} set {} }
    final private function __construct() {}
    public function getRootNode(array $options = []): \Dom\Node {}
    public function hasChildNodes(): bool {}
    public function normalize(): void {}
    public function cloneNode(bool $deep = false): \Dom\Node {}
    public function isEqualNode(?\Dom\Node $otherNode): bool {}
    public function isSameNode(?\Dom\Node $otherNode): bool {}
    public function compareDocumentPosition(\Dom\Node $other): int {}
    public function contains(?\Dom\Node $other): bool {}
    public function lookupPrefix(?string $namespace): ?string {}
    public function lookupNamespaceURI(?string $prefix): ?string {}
    public function isDefaultNamespace(?string $namespace): bool {}
    public function insertBefore(\Dom\Node $node, ?\Dom\Node $child): \Dom\Node {}
    public function appendChild(\Dom\Node $node): \Dom\Node {}
    public function replaceChild(\Dom\Node $node, \Dom\Node $child): \Dom\Node {}
    public function removeChild(\Dom\Node $child): \Dom\Node {}
    public function getLineNo(): int {}
    public function getNodePath(): string {}
    public function C14N(bool $exclusive = false, bool $withComments = false, ?array $xpath = NULL, ?array $nsPrefixes = NULL): string|false {}
    public function C14NFile(string $uri, bool $exclusive = false, bool $withComments = false, ?array $xpath = NULL, ?array $nsPrefixes = NULL): int|false {}
    public function __sleep(): array {}
    public function __wakeup(): void {}
}"#,
    ),
    (
        "domnamespacenode",
        r#"<?php
class DOMNameSpaceNode {
    public string $nodeName { get {} set {} }
    public ?string $nodeValue { get {} set {} }
    public int $nodeType { get {} set {} }
    public string $prefix { get {} set {} }
    public ?string $localName { get {} set {} }
    public ?string $namespaceURI { get {} set {} }
    public bool $isConnected { get {} set {} }
    public ?\DOMDocument $ownerDocument { get {} set {} }
    public ?\DOMNode $parentNode { get {} set {} }
    public ?\DOMElement $parentElement { get {} set {} }
    public function __sleep(): array {}
    public function __wakeup(): void {}
}"#,
    ),
    (
        "dom\\namespaceinfo",
        r#"<?php namespace Dom;
final readonly class NamespaceInfo {
    public protected(set) readonly ?string $prefix;
    public protected(set) readonly ?string $namespaceURI;
    public protected(set) readonly \Dom\Element $element;
    private function __construct() {}
}"#,
    ),
    (
        "domdocumentfragment",
        r#"<?php
class DOMDocumentFragment extends \DOMNode implements \DOMParentNode {
    public ?\DOMElement $firstElementChild { get {} set {} }
    public ?\DOMElement $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public function __construct() {}
    public function appendXML(string $data): bool {}
    public function append(...$nodes): void {}
    public function prepend(...$nodes): void {}
    public function replaceChildren(...$nodes): void {}
}"#,
    ),
    (
        "dom\\documentfragment",
        r#"<?php namespace Dom;
class DocumentFragment extends \Dom\Node implements \Dom\ParentNode {
    public \Dom\HTMLCollection $children { get {} set {} }
    public ?\Dom\Element $firstElementChild { get {} set {} }
    public ?\Dom\Element $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public function appendXml(string $data): bool {}
    public function append(\Dom\Node|string ...$nodes): void {}
    public function prepend(\Dom\Node|string ...$nodes): void {}
    public function replaceChildren(\Dom\Node|string ...$nodes): void {}
    public function querySelector(string $selectors): ?\Dom\Element {}
    public function querySelectorAll(string $selectors): \Dom\NodeList {}
}"#,
    ),
    (
        "dom\\document",
        r#"<?php namespace Dom;
abstract class Document extends \Dom\Node implements \Dom\ParentNode {
    public \Dom\HTMLCollection $children { get {} set {} }
    public ?\Dom\Element $firstElementChild { get {} set {} }
    public ?\Dom\Element $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public \Dom\Implementation $implementation { get {} set {} }
    public string $URL { get {} set {} }
    public string $documentURI { get {} set {} }
    public string $characterSet { get {} set {} }
    public string $charset { get {} set {} }
    public string $inputEncoding { get {} set {} }
    public ?\Dom\DocumentType $doctype { get {} set {} }
    public ?\Dom\Element $documentElement { get {} set {} }
    public ?\Dom\HTMLElement $body { get {} set {} }
    public ?\Dom\HTMLElement $head { get {} set {} }
    public string $title { get {} set {} }
    public function getElementsByTagName(string $qualifiedName): \Dom\HTMLCollection {}
    public function getElementsByTagNameNS(?string $namespace, string $localName): \Dom\HTMLCollection {}
    public function getElementsByClassName(string $classNames): \Dom\HTMLCollection {}
    public function createElement(string $localName): \Dom\Element {}
    public function createElementNS(?string $namespace, string $qualifiedName): \Dom\Element {}
    public function createDocumentFragment(): \Dom\DocumentFragment {}
    public function createTextNode(string $data): \Dom\Text {}
    public function createCDATASection(string $data): \Dom\CDATASection {}
    public function createComment(string $data): \Dom\Comment {}
    public function createProcessingInstruction(string $target, string $data): \Dom\ProcessingInstruction {}
    public function importNode(?\Dom\Node $node, bool $deep = false): \Dom\Node {}
    public function adoptNode(\Dom\Node $node): \Dom\Node {}
    public function createAttribute(string $localName): \Dom\Attr {}
    public function createAttributeNS(?string $namespace, string $qualifiedName): \Dom\Attr {}
    public function getElementById(string $elementId): ?\Dom\Element {}
    public function registerNodeClass(string $baseClass, ?string $extendedClass): void {}
    public function schemaValidate(string $filename, int $flags = 0): bool {}
    public function schemaValidateSource(string $source, int $flags = 0): bool {}
    public function relaxNgValidate(string $filename): bool {}
    public function relaxNgValidateSource(string $source): bool {}
    public function append(\Dom\Node|string ...$nodes): void {}
    public function prepend(\Dom\Node|string ...$nodes): void {}
    public function replaceChildren(\Dom\Node|string ...$nodes): void {}
    public function importLegacyNode(\DOMNode $node, bool $deep = false): \Dom\Node {}
    public function querySelector(string $selectors): ?\Dom\Element {}
    public function querySelectorAll(string $selectors): \Dom\NodeList {}
}"#,
    ),
    (
        "domdocument",
        r#"<?php
class DOMDocument extends \DOMNode implements \DOMParentNode {
    public ?\DOMDocumentType $doctype { get {} set {} }
    public \DOMImplementation $implementation { get {} set {} }
    public ?\DOMElement $documentElement { get {} set {} }
    public ?string $actualEncoding { get {} set {} }
    public ?string $encoding { get {} set {} }
    public ?string $xmlEncoding { get {} set {} }
    public bool $standalone { get {} set {} }
    public bool $xmlStandalone { get {} set {} }
    public ?string $version { get {} set {} }
    public ?string $xmlVersion { get {} set {} }
    public bool $strictErrorChecking { get {} set {} }
    public ?string $documentURI { get {} set {} }
    public mixed $config { get {} set {} }
    public bool $formatOutput { get {} set {} }
    public bool $validateOnParse { get {} set {} }
    public bool $resolveExternals { get {} set {} }
    public bool $preserveWhiteSpace { get {} set {} }
    public bool $recover { get {} set {} }
    public bool $substituteEntities { get {} set {} }
    public ?\DOMElement $firstElementChild { get {} set {} }
    public ?\DOMElement $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public function __construct(string $version = '1.0', string $encoding = '') {}
    public function createAttribute(string $localName) {}
    public function createAttributeNS(?string $namespace, string $qualifiedName) {}
    public function createCDATASection(string $data) {}
    public function createComment(string $data): \DOMComment {}
    public function createDocumentFragment(): \DOMDocumentFragment {}
    public function createElement(string $localName, string $value = '') {}
    public function createElementNS(?string $namespace, string $qualifiedName, string $value = '') {}
    public function createEntityReference(string $name) {}
    public function createProcessingInstruction(string $target, string $data = '') {}
    public function createTextNode(string $data): \DOMText {}
    public function getElementById(string $elementId): ?\DOMElement {}
    public function getElementsByTagName(string $qualifiedName): \DOMNodeList {}
    public function getElementsByTagNameNS(?string $namespace, string $localName): \DOMNodeList {}
    public function importNode(\DOMNode $node, bool $deep = false) {}
    public function load(string $filename, int $options = 0): bool {}
    public function loadXML(string $source, int $options = 0): bool {}
    public function normalizeDocument(): void {}
    public function registerNodeClass(string $baseClass, ?string $extendedClass): true {}
    public function save(string $filename, int $options = 0): int|false {}
    public function loadHTML(string $source, int $options = 0): bool {}
    public function loadHTMLFile(string $filename, int $options = 0): bool {}
    public function saveHTML(?\DOMNode $node = NULL): string|false {}
    public function saveHTMLFile(string $filename): int|false {}
    public function saveXML(?\DOMNode $node = NULL, int $options = 0): string|false {}
    public function schemaValidate(string $filename, int $flags = 0): bool {}
    public function schemaValidateSource(string $source, int $flags = 0): bool {}
    public function relaxNGValidate(string $filename): bool {}
    public function relaxNGValidateSource(string $source): bool {}
    public function validate(): bool {}
    public function xinclude(int $options = 0): int|false {}
    public function adoptNode(\DOMNode $node): \DOMNode|false {}
    public function append(...$nodes): void {}
    public function prepend(...$nodes): void {}
    public function replaceChildren(...$nodes): void {}
}"#,
    ),
    (
        "dom\\htmldocument",
        r#"<?php namespace Dom;
final class HTMLDocument extends \Dom\Document {
    public static function createEmpty(string $encoding = 'UTF-8'): \Dom\HTMLDocument {}
    public static function createFromFile(string $path, int $options = 0, ?string $overrideEncoding = NULL): \Dom\HTMLDocument {}
    public static function createFromString(string $source, int $options = 0, ?string $overrideEncoding = NULL): \Dom\HTMLDocument {}
    public function saveXml(?\Dom\Node $node = NULL, int $options = 0): string|false {}
    public function saveXmlFile(string $filename, int $options = 0): int|false {}
    public function saveHtml(?\Dom\Node $node = NULL): string {}
    public function saveHtmlFile(string $filename): int|false {}
}"#,
    ),
    (
        "dom\\xmldocument",
        r#"<?php namespace Dom;
final class XMLDocument extends \Dom\Document {
    public string $xmlEncoding { get {} set {} }
    public bool $xmlStandalone { get {} set {} }
    public string $xmlVersion { get {} set {} }
    public bool $formatOutput { get {} set {} }
    public static function createEmpty(string $version = '1.0', string $encoding = 'UTF-8'): \Dom\XMLDocument {}
    public static function createFromFile(string $path, int $options = 0, ?string $overrideEncoding = NULL): \Dom\XMLDocument {}
    public static function createFromString(string $source, int $options = 0, ?string $overrideEncoding = NULL): \Dom\XMLDocument {}
    public function createEntityReference(string $name): \Dom\EntityReference {}
    public function validate(): bool {}
    public function xinclude(int $options = 0): int {}
    public function saveXml(?\Dom\Node $node = NULL, int $options = 0): string|false {}
    public function saveXmlFile(string $filename, int $options = 0): int|false {}
}"#,
    ),
    (
        "domnodelist",
        r#"<?php
class DOMNodeList implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function count(): int {}
    public function getIterator(): \Iterator {}
    public function item(int $index) {}
}"#,
    ),
    (
        "dom\\nodelist",
        r#"<?php namespace Dom;
class NodeList implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function count(): int {}
    public function getIterator(): \Iterator {}
    public function item(int $index): ?\Dom\Node {}
}"#,
    ),
    (
        "domnamednodemap",
        r#"<?php
class DOMNamedNodeMap implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function getNamedItem(string $qualifiedName): ?\DOMNode {}
    public function getNamedItemNS(?string $namespace, string $localName): ?\DOMNode {}
    public function item(int $index): ?\DOMNode {}
    public function count(): int {}
    public function getIterator(): \Iterator {}
}"#,
    ),
    (
        "dom\\namednodemap",
        r#"<?php namespace Dom;
class NamedNodeMap implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function item(int $index): ?\Dom\Attr {}
    public function getNamedItem(string $qualifiedName): ?\Dom\Attr {}
    public function getNamedItemNS(?string $namespace, string $localName): ?\Dom\Attr {}
    public function count(): int {}
    public function getIterator(): \Iterator {}
}"#,
    ),
    (
        "dom\\dtdnamednodemap",
        r#"<?php namespace Dom;
class DtdNamedNodeMap implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function item(int $index): \Dom\Entity|\Dom\Notation|null {}
    public function getNamedItem(string $qualifiedName): \Dom\Entity|\Dom\Notation|null {}
    public function getNamedItemNS(?string $namespace, string $localName): \Dom\Entity|\Dom\Notation|null {}
    public function count(): int {}
    public function getIterator(): \Iterator {}
}"#,
    ),
    (
        "dom\\htmlcollection",
        r#"<?php namespace Dom;
class HTMLCollection implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public function item(int $index): ?\Dom\Element {}
    public function namedItem(string $key): ?\Dom\Element {}
    public function count(): int {}
    public function getIterator(): \Iterator {}
}"#,
    ),
    (
        "domcharacterdata",
        r#"<?php
class DOMCharacterData extends \DOMNode implements \DOMChildNode {
    public string $data { get {} set {} }
    public int $length { get {} set {} }
    public ?\DOMElement $previousElementSibling { get {} set {} }
    public ?\DOMElement $nextElementSibling { get {} set {} }
    public function appendData(string $data): true {}
    public function substringData(int $offset, int $count) {}
    public function insertData(int $offset, string $data): bool {}
    public function deleteData(int $offset, int $count): bool {}
    public function replaceData(int $offset, int $count, string $data): bool {}
    public function replaceWith(...$nodes): void {}
    public function remove(): void {}
    public function before(...$nodes): void {}
    public function after(...$nodes): void {}
}"#,
    ),
    (
        "dom\\characterdata",
        r#"<?php namespace Dom;
class CharacterData extends \Dom\Node implements \Dom\ChildNode {
    public ?\Dom\Element $previousElementSibling { get {} set {} }
    public ?\Dom\Element $nextElementSibling { get {} set {} }
    public string $data { get {} set {} }
    public int $length { get {} set {} }
    public function substringData(int $offset, int $count): string {}
    public function appendData(string $data): void {}
    public function insertData(int $offset, string $data): void {}
    public function deleteData(int $offset, int $count): void {}
    public function replaceData(int $offset, int $count, string $data): void {}
    public function remove(): void {}
    public function before(\Dom\Node|string ...$nodes): void {}
    public function after(\Dom\Node|string ...$nodes): void {}
    public function replaceWith(\Dom\Node|string ...$nodes): void {}
}"#,
    ),
    (
        "domattr",
        r#"<?php
class DOMAttr extends \DOMNode {
    public string $name { get {} set {} }
    public bool $specified { get {} set {} }
    public string $value { get {} set {} }
    public ?\DOMElement $ownerElement { get {} set {} }
    public mixed $schemaTypeInfo { get {} set {} }
    public function __construct(string $name, string $value = '') {}
    public function isId(): bool {}
}"#,
    ),
    (
        "dom\\attr",
        r#"<?php namespace Dom;
class Attr extends \Dom\Node {
    public ?string $namespaceURI { get {} set {} }
    public ?string $prefix { get {} set {} }
    public string $localName { get {} set {} }
    public string $name { get {} set {} }
    public string $value { get {} set {} }
    public ?\Dom\Element $ownerElement { get {} set {} }
    public bool $specified { get {} set {} }
    public function isId(): bool {}
    public function rename(?string $namespaceURI, string $qualifiedName): void {}
}"#,
    ),
    (
        "domelement",
        r#"<?php
class DOMElement extends \DOMNode implements \DOMParentNode, \DOMChildNode {
    public string $tagName { get {} set {} }
    public string $className { get {} set {} }
    public string $id { get {} set {} }
    public mixed $schemaTypeInfo { get {} set {} }
    public ?\DOMElement $firstElementChild { get {} set {} }
    public ?\DOMElement $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public ?\DOMElement $previousElementSibling { get {} set {} }
    public ?\DOMElement $nextElementSibling { get {} set {} }
    public function __construct(string $qualifiedName, ?string $value = NULL, string $namespace = '') {}
    public function getAttribute(string $qualifiedName): string {}
    public function getAttributeNames(): array {}
    public function getAttributeNS(?string $namespace, string $localName): string {}
    public function getAttributeNode(string $qualifiedName) {}
    public function getAttributeNodeNS(?string $namespace, string $localName) {}
    public function getElementsByTagName(string $qualifiedName): \DOMNodeList {}
    public function getElementsByTagNameNS(?string $namespace, string $localName): \DOMNodeList {}
    public function hasAttribute(string $qualifiedName): bool {}
    public function hasAttributeNS(?string $namespace, string $localName): bool {}
    public function removeAttribute(string $qualifiedName): bool {}
    public function removeAttributeNS(?string $namespace, string $localName): void {}
    public function removeAttributeNode(\DOMAttr $attr) {}
    public function setAttribute(string $qualifiedName, string $value) {}
    public function setAttributeNS(?string $namespace, string $qualifiedName, string $value): void {}
    public function setAttributeNode(\DOMAttr $attr) {}
    public function setAttributeNodeNS(\DOMAttr $attr) {}
    public function setIdAttribute(string $qualifiedName, bool $isId): void {}
    public function setIdAttributeNS(string $namespace, string $qualifiedName, bool $isId): void {}
    public function setIdAttributeNode(\DOMAttr $attr, bool $isId): void {}
    public function toggleAttribute(string $qualifiedName, ?bool $force = NULL): bool {}
    public function remove(): void {}
    public function before(...$nodes): void {}
    public function after(...$nodes): void {}
    public function replaceWith(...$nodes): void {}
    public function append(...$nodes): void {}
    public function prepend(...$nodes): void {}
    public function replaceChildren(...$nodes): void {}
    public function insertAdjacentElement(string $where, \DOMElement $element): ?\DOMElement {}
    public function insertAdjacentText(string $where, string $data): void {}
}"#,
    ),
    (
        "dom\\element",
        r#"<?php namespace Dom;
class Element extends \Dom\Node implements \Dom\ParentNode, \Dom\ChildNode {
    public ?string $namespaceURI { get {} set {} }
    public ?string $prefix { get {} set {} }
    public string $localName { get {} set {} }
    public string $tagName { get {} set {} }
    public \Dom\HTMLCollection $children { get {} set {} }
    public ?\Dom\Element $firstElementChild { get {} set {} }
    public ?\Dom\Element $lastElementChild { get {} set {} }
    public int $childElementCount { get {} set {} }
    public ?\Dom\Element $previousElementSibling { get {} set {} }
    public ?\Dom\Element $nextElementSibling { get {} set {} }
    public string $id { get {} set {} }
    public string $className { get {} set {} }
    public \Dom\TokenList $classList { get {} set {} }
    public \Dom\NamedNodeMap $attributes { get {} set {} }
    public string $innerHTML { get {} set {} }
    public string $outerHTML { get {} set {} }
    public string $substitutedNodeValue { get {} set {} }
    public function hasAttributes(): bool {}
    public function getAttributeNames(): array {}
    public function getAttribute(string $qualifiedName): ?string {}
    public function getAttributeNS(?string $namespace, string $localName): ?string {}
    public function setAttribute(string $qualifiedName, string $value): void {}
    public function setAttributeNS(?string $namespace, string $qualifiedName, string $value): void {}
    public function removeAttribute(string $qualifiedName): void {}
    public function removeAttributeNS(?string $namespace, string $localName): void {}
    public function toggleAttribute(string $qualifiedName, ?bool $force = NULL): bool {}
    public function hasAttribute(string $qualifiedName): bool {}
    public function hasAttributeNS(?string $namespace, string $localName): bool {}
    public function getAttributeNode(string $qualifiedName): ?\Dom\Attr {}
    public function getAttributeNodeNS(?string $namespace, string $localName): ?\Dom\Attr {}
    public function setAttributeNode(\Dom\Attr $attr): ?\Dom\Attr {}
    public function setAttributeNodeNS(\Dom\Attr $attr): ?\Dom\Attr {}
    public function removeAttributeNode(\Dom\Attr $attr): \Dom\Attr {}
    public function getElementsByTagName(string $qualifiedName): \Dom\HTMLCollection {}
    public function getElementsByTagNameNS(?string $namespace, string $localName): \Dom\HTMLCollection {}
    public function getElementsByClassName(string $classNames): \Dom\HTMLCollection {}
    public function insertAdjacentElement(\Dom\AdjacentPosition $where, \Dom\Element $element): ?\Dom\Element {}
    public function insertAdjacentText(\Dom\AdjacentPosition $where, string $data): void {}
    public function insertAdjacentHTML(\Dom\AdjacentPosition $where, string $string): void {}
    public function setIdAttribute(string $qualifiedName, bool $isId): void {}
    public function setIdAttributeNS(?string $namespace, string $qualifiedName, bool $isId): void {}
    public function setIdAttributeNode(\Dom\Attr $attr, bool $isId): void {}
    public function remove(): void {}
    public function before(\Dom\Node|string ...$nodes): void {}
    public function after(\Dom\Node|string ...$nodes): void {}
    public function replaceWith(\Dom\Node|string ...$nodes): void {}
    public function append(\Dom\Node|string ...$nodes): void {}
    public function prepend(\Dom\Node|string ...$nodes): void {}
    public function replaceChildren(\Dom\Node|string ...$nodes): void {}
    public function querySelector(string $selectors): ?\Dom\Element {}
    public function querySelectorAll(string $selectors): \Dom\NodeList {}
    public function closest(string $selectors): ?\Dom\Element {}
    public function matches(string $selectors): bool {}
    public function getInScopeNamespaces(): array {}
    public function getDescendantNamespaces(): array {}
    public function rename(?string $namespaceURI, string $qualifiedName): void {}
}"#,
    ),
    (
        "dom\\htmlelement",
        r#"<?php namespace Dom;
class HTMLElement extends \Dom\Element {
}"#,
    ),
    (
        "domtext",
        r#"<?php
class DOMText extends \DOMCharacterData {
    public string $wholeText { get {} set {} }
    public function __construct(string $data = '') {}
    public function isWhitespaceInElementContent(): bool {}
    public function isElementContentWhitespace(): bool {}
    public function splitText(int $offset) {}
}"#,
    ),
    (
        "dom\\text",
        r#"<?php namespace Dom;
class Text extends \Dom\CharacterData {
    public string $wholeText { get {} set {} }
    public function splitText(int $offset): \Dom\Text {}
}"#,
    ),
    (
        "domcomment",
        r#"<?php
class DOMComment extends \DOMCharacterData {
    public function __construct(string $data = '') {}
}"#,
    ),
    (
        "dom\\comment",
        r#"<?php namespace Dom;
class Comment extends \Dom\CharacterData {
}"#,
    ),
    (
        "domcdatasection",
        r#"<?php
class DOMCdataSection extends \DOMText {
    public function __construct(string $data) {}
}"#,
    ),
    (
        "dom\\cdatasection",
        r#"<?php namespace Dom;
class CDATASection extends \Dom\Text {
}"#,
    ),
    (
        "domdocumenttype",
        r#"<?php
class DOMDocumentType extends \DOMNode {
    public string $name { get {} set {} }
    public \DOMNamedNodeMap $entities { get {} set {} }
    public \DOMNamedNodeMap $notations { get {} set {} }
    public string $publicId { get {} set {} }
    public string $systemId { get {} set {} }
    public ?string $internalSubset { get {} set {} }
}"#,
    ),
    (
        "dom\\documenttype",
        r#"<?php namespace Dom;
class DocumentType extends \Dom\Node implements \Dom\ChildNode {
    public string $name { get {} set {} }
    public \Dom\DtdNamedNodeMap $entities { get {} set {} }
    public \Dom\DtdNamedNodeMap $notations { get {} set {} }
    public string $publicId { get {} set {} }
    public string $systemId { get {} set {} }
    public ?string $internalSubset { get {} set {} }
    public function remove(): void {}
    public function before(\Dom\Node|string ...$nodes): void {}
    public function after(\Dom\Node|string ...$nodes): void {}
    public function replaceWith(\Dom\Node|string ...$nodes): void {}
}"#,
    ),
    (
        "domnotation",
        r#"<?php
class DOMNotation extends \DOMNode {
    public string $publicId { get {} set {} }
    public string $systemId { get {} set {} }
}"#,
    ),
    (
        "dom\\notation",
        r#"<?php namespace Dom;
class Notation extends \Dom\Node {
    public string $publicId { get {} set {} }
    public string $systemId { get {} set {} }
}"#,
    ),
    (
        "domentity",
        r#"<?php
class DOMEntity extends \DOMNode {
    public ?string $publicId { get {} set {} }
    public ?string $systemId { get {} set {} }
    public ?string $notationName { get {} set {} }
    public ?string $actualEncoding { get {} set {} }
    public ?string $encoding { get {} set {} }
    public ?string $version { get {} set {} }
}"#,
    ),
    (
        "dom\\entity",
        r#"<?php namespace Dom;
class Entity extends \Dom\Node {
    public ?string $publicId { get {} set {} }
    public ?string $systemId { get {} set {} }
    public ?string $notationName { get {} set {} }
}"#,
    ),
    (
        "domentityreference",
        r#"<?php
class DOMEntityReference extends \DOMNode {
    public function __construct(string $name) {}
}"#,
    ),
    (
        "dom\\entityreference",
        r#"<?php namespace Dom;
class EntityReference extends \Dom\Node {
}"#,
    ),
    (
        "domprocessinginstruction",
        r#"<?php
class DOMProcessingInstruction extends \DOMNode {
    public string $target { get {} set {} }
    public string $data { get {} set {} }
    public function __construct(string $name, string $value = '') {}
}"#,
    ),
    (
        "dom\\processinginstruction",
        r#"<?php namespace Dom;
class ProcessingInstruction extends \Dom\CharacterData {
    public string $target { get {} set {} }
}"#,
    ),
    (
        "domxpath",
        r#"<?php
#[\NotCloneable]
class DOMXPath {
    public \DOMDocument $document { get {} set {} }
    public bool $registerNodeNamespaces { get {} set {} }
    public function __construct(\DOMDocument $document, bool $registerNodeNS = true) {}
    public function evaluate(string $expression, ?\DOMNode $contextNode = NULL, bool $registerNodeNS = true): mixed {}
    public function query(string $expression, ?\DOMNode $contextNode = NULL, bool $registerNodeNS = true): mixed {}
    public function registerNamespace(string $prefix, string $namespace): bool {}
    public function registerPhpFunctions(array|string|null $restrict = NULL): void {}
    public function registerPhpFunctionNS(string $namespaceURI, string $name, callable $callable): void {}
    public static function quote(string $str): string {}
}"#,
    ),
    (
        "dom\\xpath",
        r#"<?php namespace Dom;
#[\NotCloneable]
final class XPath {
    public \Dom\Document $document { get {} set {} }
    public bool $registerNodeNamespaces { get {} set {} }
    public function __construct(\Dom\Document $document, bool $registerNodeNS = true) {}
    public function evaluate(string $expression, ?\Dom\Node $contextNode = NULL, bool $registerNodeNS = true): \Dom\NodeList|string|float|bool|null {}
    public function query(string $expression, ?\Dom\Node $contextNode = NULL, bool $registerNodeNS = true): \Dom\NodeList {}
    public function registerNamespace(string $prefix, string $namespace): bool {}
    public function registerPhpFunctions(array|string|null $restrict = NULL): void {}
    public function registerPhpFunctionNS(string $namespaceURI, string $name, callable $callable): void {}
    public static function quote(string $str): string {}
}"#,
    ),
    (
        "dom\\tokenlist",
        r#"<?php namespace Dom;
final class TokenList implements \IteratorAggregate, \Countable {
    public int $length { get {} set {} }
    public string $value { get {} set {} }
    private function __construct() {}
    public function item(int $index): ?string {}
    public function contains(string $token): bool {}
    public function add(string ...$tokens): void {}
    public function remove(string ...$tokens): void {}
    public function toggle(string $token, ?bool $force = NULL): bool {}
    public function replace(string $token, string $newToken): bool {}
    public function supports(string $token): bool {}
    public function count(): int {}
    public function getIterator(): \Iterator {}
}"#,
    ),
];
