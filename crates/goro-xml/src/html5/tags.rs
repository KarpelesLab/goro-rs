//! HTML tag names the tree builder treats specially, and the name tables of
//! WHATWG HTML "13.2.6.5 The rules for parsing tokens in foreign content"
//! and "13.2.6.1 Creating and inserting nodes" (SVG/MathML adjustments).

/// Known HTML tag names. `Other` covers every other name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum T {
    A,
    Address,
    Applet,
    Area,
    Article,
    Aside,
    B,
    Base,
    Basefont,
    Bgsound,
    Big,
    Blockquote,
    Body,
    Br,
    Button,
    Caption,
    Center,
    Code,
    Col,
    Colgroup,
    Dd,
    Details,
    Dialog,
    Dir,
    Div,
    Dl,
    Dt,
    Em,
    Embed,
    Fieldset,
    Figcaption,
    Figure,
    Font,
    Footer,
    Form,
    Frame,
    Frameset,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Head,
    Header,
    Hgroup,
    Hr,
    Html,
    I,
    Iframe,
    Image,
    Img,
    Input,
    Keygen,
    Li,
    Link,
    Listing,
    Main,
    Marquee,
    Math,
    Menu,
    Meta,
    Nav,
    Nobr,
    Noembed,
    Noframes,
    Noscript,
    Object,
    Ol,
    Optgroup,
    Option,
    P,
    Param,
    Plaintext,
    Pre,
    Rb,
    Rp,
    Rt,
    Rtc,
    Ruby,
    S,
    Script,
    Search,
    Section,
    Select,
    Small,
    Source,
    Span,
    Strike,
    Strong,
    Style,
    Sub,
    Summary,
    Sup,
    Svg,
    Table,
    Tbody,
    Td,
    Template,
    Textarea,
    Tfoot,
    Th,
    Thead,
    Title,
    Tr,
    Track,
    Tt,
    U,
    Ul,
    Var,
    Wbr,
    Xmp,
    Other,
}

impl T {
    pub fn from_name(n: &str) -> T {
        use T::*;
        match n {
            "a" => A,
            "address" => Address,
            "applet" => Applet,
            "area" => Area,
            "article" => Article,
            "aside" => Aside,
            "b" => B,
            "base" => Base,
            "basefont" => Basefont,
            "bgsound" => Bgsound,
            "big" => Big,
            "blockquote" => Blockquote,
            "body" => Body,
            "br" => Br,
            "button" => Button,
            "caption" => Caption,
            "center" => Center,
            "code" => Code,
            "col" => Col,
            "colgroup" => Colgroup,
            "dd" => Dd,
            "details" => Details,
            "dialog" => Dialog,
            "dir" => Dir,
            "div" => Div,
            "dl" => Dl,
            "dt" => Dt,
            "em" => Em,
            "embed" => Embed,
            "fieldset" => Fieldset,
            "figcaption" => Figcaption,
            "figure" => Figure,
            "font" => Font,
            "footer" => Footer,
            "form" => Form,
            "frame" => Frame,
            "frameset" => Frameset,
            "h1" => H1,
            "h2" => H2,
            "h3" => H3,
            "h4" => H4,
            "h5" => H5,
            "h6" => H6,
            "head" => Head,
            "header" => Header,
            "hgroup" => Hgroup,
            "hr" => Hr,
            "html" => Html,
            "i" => I,
            "iframe" => Iframe,
            "image" => Image,
            "img" => Img,
            "input" => Input,
            "keygen" => Keygen,
            "li" => Li,
            "link" => Link,
            "listing" => Listing,
            "main" => Main,
            "marquee" => Marquee,
            "math" => Math,
            "menu" => Menu,
            "meta" => Meta,
            "nav" => Nav,
            "nobr" => Nobr,
            "noembed" => Noembed,
            "noframes" => Noframes,
            "noscript" => Noscript,
            "object" => Object,
            "ol" => Ol,
            "optgroup" => Optgroup,
            "option" => Option,
            "p" => P,
            "param" => Param,
            "plaintext" => Plaintext,
            "pre" => Pre,
            "rb" => Rb,
            "rp" => Rp,
            "rt" => Rt,
            "rtc" => Rtc,
            "ruby" => Ruby,
            "s" => S,
            "script" => Script,
            "search" => Search,
            "section" => Section,
            "select" => Select,
            "small" => Small,
            "source" => Source,
            "span" => Span,
            "strike" => Strike,
            "strong" => Strong,
            "style" => Style,
            "sub" => Sub,
            "summary" => Summary,
            "sup" => Sup,
            "svg" => Svg,
            "table" => Table,
            "tbody" => Tbody,
            "td" => Td,
            "template" => Template,
            "textarea" => Textarea,
            "tfoot" => Tfoot,
            "th" => Th,
            "thead" => Thead,
            "title" => Title,
            "tr" => Tr,
            "track" => Track,
            "tt" => Tt,
            "u" => U,
            "ul" => Ul,
            "var" => Var,
            "wbr" => Wbr,
            "xmp" => Xmp,
            _ => Other,
        }
    }

    /// The "special" category for HTML elements.
    pub fn is_special(self) -> bool {
        use T::*;
        matches!(
            self,
            Address
                | Applet
                | Area
                | Article
                | Aside
                | Base
                | Basefont
                | Bgsound
                | Blockquote
                | Body
                | Br
                | Button
                | Caption
                | Center
                | Col
                | Colgroup
                | Dd
                | Details
                | Dir
                | Div
                | Dl
                | Dt
                | Embed
                | Fieldset
                | Figcaption
                | Figure
                | Footer
                | Form
                | Frame
                | Frameset
                | H1
                | H2
                | H3
                | H4
                | H5
                | H6
                | Head
                | Header
                | Hgroup
                | Hr
                | Html
                | Iframe
                | Img
                | Input
                | Keygen
                | Li
                | Link
                | Listing
                | Main
                | Marquee
                | Menu
                | Meta
                | Nav
                | Noembed
                | Noframes
                | Noscript
                | Object
                | Ol
                | P
                | Param
                | Plaintext
                | Pre
                | Script
                | Search
                | Section
                | Select
                | Source
                | Style
                | Summary
                | Table
                | Tbody
                | Td
                | Template
                | Textarea
                | Tfoot
                | Th
                | Thead
                | Title
                | Tr
                | Track
                | Ul
                | Wbr
                | Xmp
        )
    }

    pub fn is_heading(self) -> bool {
        matches!(self, T::H1 | T::H2 | T::H3 | T::H4 | T::H5 | T::H6)
    }
}

/// SVG tag name case fixes.
pub(crate) fn svg_tag_fix(n: &str) -> Option<&'static str> {
    Some(match n {
        "altglyph" => "altGlyph",
        "altglyphdef" => "altGlyphDef",
        "altglyphitem" => "altGlyphItem",
        "animatecolor" => "animateColor",
        "animatemotion" => "animateMotion",
        "animatetransform" => "animateTransform",
        "clippath" => "clipPath",
        "feblend" => "feBlend",
        "fecolormatrix" => "feColorMatrix",
        "fecomponenttransfer" => "feComponentTransfer",
        "fecomposite" => "feComposite",
        "feconvolvematrix" => "feConvolveMatrix",
        "fediffuselighting" => "feDiffuseLighting",
        "fedisplacementmap" => "feDisplacementMap",
        "fedistantlight" => "feDistantLight",
        "fedropshadow" => "feDropShadow",
        "feflood" => "feFlood",
        "fefunca" => "feFuncA",
        "fefuncb" => "feFuncB",
        "fefuncg" => "feFuncG",
        "fefuncr" => "feFuncR",
        "fegaussianblur" => "feGaussianBlur",
        "feimage" => "feImage",
        "femerge" => "feMerge",
        "femergenode" => "feMergeNode",
        "femorphology" => "feMorphology",
        "feoffset" => "feOffset",
        "fepointlight" => "fePointLight",
        "fespecularlighting" => "feSpecularLighting",
        "fespotlight" => "feSpotLight",
        "fetile" => "feTile",
        "feturbulence" => "feTurbulence",
        "foreignobject" => "foreignObject",
        "glyphref" => "glyphRef",
        "lineargradient" => "linearGradient",
        "radialgradient" => "radialGradient",
        "textpath" => "textPath",
        _ => return None,
    })
}

/// SVG attribute name case fixes.
pub(crate) fn svg_attr_fix(n: &str) -> Option<&'static str> {
    Some(match n {
        "attributename" => "attributeName",
        "attributetype" => "attributeType",
        "basefrequency" => "baseFrequency",
        "baseprofile" => "baseProfile",
        "calcmode" => "calcMode",
        "clippathunits" => "clipPathUnits",
        "diffuseconstant" => "diffuseConstant",
        "edgemode" => "edgeMode",
        "filterunits" => "filterUnits",
        "glyphref" => "glyphRef",
        "gradienttransform" => "gradientTransform",
        "gradientunits" => "gradientUnits",
        "kernelmatrix" => "kernelMatrix",
        "kernelunitlength" => "kernelUnitLength",
        "keypoints" => "keyPoints",
        "keysplines" => "keySplines",
        "keytimes" => "keyTimes",
        "lengthadjust" => "lengthAdjust",
        "limitingconeangle" => "limitingConeAngle",
        "markerheight" => "markerHeight",
        "markerunits" => "markerUnits",
        "markerwidth" => "markerWidth",
        "maskcontentunits" => "maskContentUnits",
        "maskunits" => "maskUnits",
        "numoctaves" => "numOctaves",
        "pathlength" => "pathLength",
        "patterncontentunits" => "patternContentUnits",
        "patterntransform" => "patternTransform",
        "patternunits" => "patternUnits",
        "pointsatx" => "pointsAtX",
        "pointsaty" => "pointsAtY",
        "pointsatz" => "pointsAtZ",
        "preservealpha" => "preserveAlpha",
        "preserveaspectratio" => "preserveAspectRatio",
        "primitiveunits" => "primitiveUnits",
        "refx" => "refX",
        "refy" => "refY",
        "repeatcount" => "repeatCount",
        "repeatdur" => "repeatDur",
        "requiredextensions" => "requiredExtensions",
        "requiredfeatures" => "requiredFeatures",
        "specularconstant" => "specularConstant",
        "specularexponent" => "specularExponent",
        "spreadmethod" => "spreadMethod",
        "startoffset" => "startOffset",
        "stddeviation" => "stdDeviation",
        "stitchtiles" => "stitchTiles",
        "surfacescale" => "surfaceScale",
        "systemlanguage" => "systemLanguage",
        "tablevalues" => "tableValues",
        "targetx" => "targetX",
        "targety" => "targetY",
        "textlength" => "textLength",
        "viewbox" => "viewBox",
        "viewtarget" => "viewTarget",
        "xchannelselector" => "xChannelSelector",
        "ychannelselector" => "yChannelSelector",
        "zoomandpan" => "zoomAndPan",
        _ => return None,
    })
}

/// Foreign attribute adjustments: (prefix, local name, namespace).
pub(crate) fn foreign_attr_fix(
    n: &str,
) -> Option<(Option<&'static str>, &'static str, &'static str)> {
    use crate::tree::{XLINK_NS, XML_NS, XMLNS_NS};
    Some(match n {
        "xlink:actuate" => (Some("xlink"), "actuate", XLINK_NS),
        "xlink:arcrole" => (Some("xlink"), "arcrole", XLINK_NS),
        "xlink:href" => (Some("xlink"), "href", XLINK_NS),
        "xlink:role" => (Some("xlink"), "role", XLINK_NS),
        "xlink:show" => (Some("xlink"), "show", XLINK_NS),
        "xlink:title" => (Some("xlink"), "title", XLINK_NS),
        "xlink:type" => (Some("xlink"), "type", XLINK_NS),
        "xml:lang" => (Some("xml"), "lang", XML_NS),
        "xml:space" => (Some("xml"), "space", XML_NS),
        "xmlns" => (None, "xmlns", XMLNS_NS),
        "xmlns:xlink" => (Some("xmlns"), "xlink", XMLNS_NS),
        _ => return None,
    })
}
