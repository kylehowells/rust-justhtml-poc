//! HTML parsing constants

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Void elements that have no closing tag
pub static VOID_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "area", "base", "br", "col", "embed", "hr", "img", "input",
        "link", "meta", "param", "source", "track", "wbr",
    ].into_iter().collect()
});

/// Raw text elements (contents not parsed as HTML)
pub static RAW_TEXT_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["script", "style"].into_iter().collect()
});

/// Escapable raw text elements
pub static ESCAPABLE_RAW_TEXT_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["textarea", "title"].into_iter().collect()
});

/// Formatting elements for the adoption agency algorithm
pub static FORMATTING_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "b", "big", "code", "em", "font", "i", "nobr",
        "s", "small", "strike", "strong", "tt", "u",
    ].into_iter().collect()
});

/// Special elements that have special parsing rules
pub static SPECIAL_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "address", "applet", "area", "article", "aside", "base", "basefont",
        "bgsound", "blockquote", "body", "br", "button", "caption", "center",
        "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "embed",
        "fieldset", "figcaption", "figure", "footer", "form", "frame", "frameset",
        "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr",
        "html", "iframe", "img", "input", "keygen", "li", "link", "listing",
        "main", "marquee", "menu", "meta", "nav", "noembed", "noframes",
        "noscript", "object", "ol", "p", "param", "plaintext", "pre", "script",
        "search", "section", "select", "source", "style", "summary", "table",
        "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "title",
        "tr", "track", "ul", "wbr", "xmp",
    ].into_iter().collect()
});

/// Elements that imply closing a <p> element
pub static P_CLOSING_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "address", "article", "aside", "blockquote", "center", "details",
        "dialog", "dir", "div", "dl", "fieldset", "figcaption", "figure",
        "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header",
        "hgroup", "hr", "main", "menu", "nav", "ol", "p", "pre", "search",
        "section", "table", "ul",
    ].into_iter().collect()
});

/// Scope elements for checking element scope
pub static SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "applet", "caption", "html", "table", "td", "th", "marquee", "object", "template",
        // MathML elements
        "mi", "mo", "mn", "ms", "mtext", "annotation-xml",
        // SVG elements
        "foreignObject", "desc", "title",
    ].into_iter().collect()
});

/// List item scope elements
pub static LIST_ITEM_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut set = SCOPE_ELEMENTS.clone();
    set.insert("ol");
    set.insert("ul");
    set
});

/// Button scope elements
pub static BUTTON_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut set = SCOPE_ELEMENTS.clone();
    set.insert("button");
    set
});

/// Table scope elements
pub static TABLE_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["html", "table", "template"].into_iter().collect()
});

/// Select scope elements
pub static SELECT_SCOPE_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["optgroup", "option"].into_iter().collect()
});

/// Elements that are implicitly closed by certain other elements
pub static IMPLIED_END_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc",
    ].into_iter().collect()
});

/// Thoroughly implied end tags (includes more elements)
pub static THOROUGHLY_IMPLIED_END_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut set = IMPLIED_END_TAGS.clone();
    for tag in ["caption", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr"] {
        set.insert(tag);
    }
    set
});

/// SVG element case adjustments
pub static SVG_ELEMENT_ADJUSTMENTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("altglyph", "altGlyph"),
        ("altglyphdef", "altGlyphDef"),
        ("altglyphitem", "altGlyphItem"),
        ("animatecolor", "animateColor"),
        ("animatemotion", "animateMotion"),
        ("animatetransform", "animateTransform"),
        ("clippath", "clipPath"),
        ("feblend", "feBlend"),
        ("fecolormatrix", "feColorMatrix"),
        ("fecomponenttransfer", "feComponentTransfer"),
        ("fecomposite", "feComposite"),
        ("feconvolvematrix", "feConvolveMatrix"),
        ("fediffuselighting", "feDiffuseLighting"),
        ("fedisplacementmap", "feDisplacementMap"),
        ("fedistantlight", "feDistantLight"),
        ("fedropshadow", "feDropShadow"),
        ("feflood", "feFlood"),
        ("fefunca", "feFuncA"),
        ("fefuncb", "feFuncB"),
        ("fefuncg", "feFuncG"),
        ("fefuncr", "feFuncR"),
        ("fegaussianblur", "feGaussianBlur"),
        ("feimage", "feImage"),
        ("femerge", "feMerge"),
        ("femergenode", "feMergeNode"),
        ("femorphology", "feMorphology"),
        ("feoffset", "feOffset"),
        ("fepointlight", "fePointLight"),
        ("fespecularlighting", "feSpecularLighting"),
        ("fespotlight", "feSpotLight"),
        ("fetile", "feTile"),
        ("feturbulence", "feTurbulence"),
        ("foreignobject", "foreignObject"),
        ("glyphref", "glyphRef"),
        ("lineargradient", "linearGradient"),
        ("radialgradient", "radialGradient"),
        ("textpath", "textPath"),
    ].into_iter().collect()
});

/// SVG attribute case adjustments
pub static SVG_ATTRIBUTE_ADJUSTMENTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("attributename", "attributeName"),
        ("attributetype", "attributeType"),
        ("basefrequency", "baseFrequency"),
        ("baseprofile", "baseProfile"),
        ("calcmode", "calcMode"),
        ("clippathunits", "clipPathUnits"),
        ("diffuseconstant", "diffuseConstant"),
        ("edgemode", "edgeMode"),
        ("filterunits", "filterUnits"),
        ("glyphref", "glyphRef"),
        ("gradienttransform", "gradientTransform"),
        ("gradientunits", "gradientUnits"),
        ("kernelmatrix", "kernelMatrix"),
        ("kernelunitlength", "kernelUnitLength"),
        ("keypoints", "keyPoints"),
        ("keysplines", "keySplines"),
        ("keytimes", "keyTimes"),
        ("lengthadjust", "lengthAdjust"),
        ("limitingconeangle", "limitingConeAngle"),
        ("markerheight", "markerHeight"),
        ("markerunits", "markerUnits"),
        ("markerwidth", "markerWidth"),
        ("maskcontentunits", "maskContentUnits"),
        ("maskunits", "maskUnits"),
        ("numoctaves", "numOctaves"),
        ("pathlength", "pathLength"),
        ("patterncontentunits", "patternContentUnits"),
        ("patterntransform", "patternTransform"),
        ("patternunits", "patternUnits"),
        ("pointsatx", "pointsAtX"),
        ("pointsaty", "pointsAtY"),
        ("pointsatz", "pointsAtZ"),
        ("preservealpha", "preserveAlpha"),
        ("preserveaspectratio", "preserveAspectRatio"),
        ("primitiveunits", "primitiveUnits"),
        ("refx", "refX"),
        ("refy", "refY"),
        ("repeatcount", "repeatCount"),
        ("repeatdur", "repeatDur"),
        ("requiredextensions", "requiredExtensions"),
        ("requiredfeatures", "requiredFeatures"),
        ("specularconstant", "specularConstant"),
        ("specularexponent", "specularExponent"),
        ("spreadmethod", "spreadMethod"),
        ("startoffset", "startOffset"),
        ("stddeviation", "stdDeviation"),
        ("stitchtiles", "stitchTiles"),
        ("surfacescale", "surfaceScale"),
        ("systemlanguage", "systemLanguage"),
        ("tablevalues", "tableValues"),
        ("targetx", "targetX"),
        ("targety", "targetY"),
        ("textlength", "textLength"),
        ("viewbox", "viewBox"),
        ("viewtarget", "viewTarget"),
        ("xchannelselector", "xChannelSelector"),
        ("ychannelselector", "yChannelSelector"),
        ("zoomandpan", "zoomAndPan"),
    ].into_iter().collect()
});

/// MathML attribute case adjustments
pub static MATHML_ATTRIBUTE_ADJUSTMENTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("definitionurl", "definitionURL"),
    ].into_iter().collect()
});

/// Check if character is ASCII whitespace
#[inline]
pub fn is_ascii_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// Check if character is ASCII alpha
#[inline]
pub fn is_ascii_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Check if character is ASCII alphanumeric
#[inline]
pub fn is_ascii_alphanumeric(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// Check if character is ASCII hex digit
#[inline]
pub fn is_ascii_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Check if character is ASCII digit
#[inline]
pub fn is_ascii_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Check if character is ASCII upper alpha
#[inline]
pub fn is_ascii_upper_alpha(c: char) -> bool {
    c.is_ascii_uppercase()
}
