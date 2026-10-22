//! access.c: the accessibility checks (WAI guidelines, accessibility-check
//! levels 1-3).

use crate::attrs;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::lexer;
use crate::message;
use crate::tags;
use crate::tmbstr;

/// TEXTBUF_SIZE
const TEXTBUF_SIZE: usize = 128;

/// TidyAccessImpl
#[derive(Clone, Debug, Default)]
pub struct AccessImpl {
    /// gets set from Tidy variable AccessibilityCheckLevel
    pub prioritychk: i32,
    /// Number of characters that are found within the concatenated text
    pub counter: i32,
    /// list of characters in the text nodes found within a container element
    pub text_node: Vec<u8>,
    /// Number of frame elements found within a frameset
    pub num_frames: i32,
    /// Number of 'longdesc' attributes found within a frameset
    pub has_checked_long_desc: i32,
    pub checked_headers: i32,
    pub list_elements: i32,
    pub other_list_elements: i32,
    pub has_use_map: bool,
    pub has_name: bool,
    pub has_map: bool,
    pub has_th: bool,
    pub has_valid_for: bool,
    pub has_valid_id: bool,
    pub has_valid_row_headers: bool,
    pub has_valid_column_headers: bool,
    pub has_invalid_row_header: bool,
    pub has_invalid_column_header: bool,
    pub for_id: i32,
}

const IMAGE_EXTENSIONS: &[&[u8]] = &[
    b".jpg", b".gif", b".tif", b".pct", b".pic", b".iff", b".dib", b".tga", b".pcx", b".png",
    b".jpeg", b".tiff", b".bmp",
];

const SOUND_EXTENSIONS: &[&[u8]] = &[b".wav", b".au", b".aiff", b".snd", b".ra", b".rm"];

const SOUND_EXT_ERR_CODES: &[u32] = &[
    AUDIO_MISSING_TEXT_WAV,
    AUDIO_MISSING_TEXT_AU,
    AUDIO_MISSING_TEXT_AIFF,
    AUDIO_MISSING_TEXT_SND,
    AUDIO_MISSING_TEXT_RA,
    AUDIO_MISSING_TEXT_RM,
];

const MEDIA_EXTENSIONS: &[&[u8]] = &[
    b".mpg", b".mov", b".asx", b".avi", b".ivf", b".m1v", b".mmm", b".mp2v", b".mpa", b".mpe",
    b".mpeg", b".ram", b".smi", b".smil", b".swf", b".wm", b".wma", b".wmv",
];

const FRAME_EXTENSIONS: &[&[u8]] = &[
    b".htm", b".html", b".shtm", b".shtml", b".cfm", b".cfml", b".asp", b".cgi", b".pl", b".smil",
];

const COLOR_VALUES: &[[i32; 3]] = &[
    [240, 248, 255],
    [250, 235, 215],
    [0, 255, 255],
    [127, 255, 212],
    [240, 255, 255],
    [245, 245, 220],
    [255, 228, 196],
    [0, 0, 0],
    [255, 235, 205],
    [0, 0, 255],
    [138, 43, 226],
    [165, 42, 42],
    [222, 184, 135],
    [95, 158, 160],
    [127, 255, 0],
    [210, 105, 30],
    [255, 127, 80],
    [100, 149, 237],
    [255, 248, 220],
    [220, 20, 60],
    [0, 255, 255],
    [0, 0, 139],
    [0, 139, 139],
    [184, 134, 11],
    [169, 169, 169],
    [0, 100, 0],
    [169, 169, 169],
    [189, 183, 107],
    [139, 0, 139],
    [85, 107, 47],
    [255, 140, 0],
    [153, 50, 204],
    [139, 0, 0],
    [233, 150, 122],
    [143, 188, 143],
    [72, 61, 139],
    [47, 79, 79],
    [47, 79, 79],
    [0, 206, 209],
    [148, 0, 211],
    [255, 20, 147],
    [0, 191, 255],
    [105, 105, 105],
    [105, 105, 105],
    [30, 144, 255],
    [178, 34, 34],
    [255, 250, 240],
    [34, 139, 34],
    [255, 0, 255],
    [220, 220, 220],
    [248, 248, 255],
    [255, 215, 0],
    [218, 165, 32],
    [128, 128, 128],
    [0, 128, 0],
    [173, 255, 47],
    [128, 128, 128],
    [240, 255, 240],
    [255, 105, 180],
    [205, 92, 92],
    [75, 0, 130],
    [255, 255, 240],
    [240, 230, 140],
    [230, 230, 250],
    [255, 240, 245],
    [124, 252, 0],
    [255, 250, 205],
    [173, 216, 230],
    [240, 128, 128],
    [224, 255, 255],
    [250, 250, 210],
    [211, 211, 211],
    [144, 238, 144],
    [211, 211, 211],
    [255, 182, 193],
    [255, 160, 122],
    [32, 178, 170],
    [135, 206, 250],
    [119, 136, 153],
    [119, 136, 153],
    [176, 196, 222],
    [255, 255, 224],
    [0, 255, 0],
    [50, 205, 50],
    [250, 240, 230],
    [255, 0, 255],
    [128, 0, 0],
    [102, 205, 170],
    [0, 0, 205],
    [186, 85, 211],
    [147, 112, 219],
    [60, 179, 113],
    [123, 104, 238],
    [0, 250, 154],
    [72, 209, 204],
    [199, 21, 133],
    [25, 25, 112],
    [245, 255, 250],
    [255, 228, 225],
    [255, 228, 181],
    [255, 222, 173],
    [0, 0, 128],
    [253, 245, 230],
    [128, 128, 0],
    [107, 142, 35],
    [255, 165, 0],
    [255, 69, 0],
    [218, 112, 214],
    [238, 232, 170],
    [152, 251, 152],
    [175, 238, 238],
    [219, 112, 147],
    [255, 239, 213],
    [255, 218, 185],
    [205, 133, 63],
    [255, 192, 203],
    [221, 160, 221],
    [176, 224, 230],
    [128, 0, 128],
    [102, 51, 153],
    [255, 0, 0],
    [188, 143, 143],
    [65, 105, 225],
    [139, 69, 19],
    [250, 128, 114],
    [244, 164, 96],
    [46, 139, 87],
    [255, 245, 238],
    [160, 82, 45],
    [192, 192, 192],
    [135, 206, 235],
    [106, 90, 205],
    [112, 128, 144],
    [112, 128, 144],
    [255, 250, 250],
    [0, 255, 127],
    [70, 130, 180],
    [210, 180, 140],
    [0, 128, 128],
    [216, 191, 216],
    [255, 99, 71],
    [64, 224, 208],
    [238, 130, 238],
    [245, 222, 179],
    [255, 255, 255],
    [245, 245, 245],
    [255, 255, 0],
    [154, 205, 50],
];

const COLOR_NAMES: &[&[u8]] = &[
    b"aliceblue",
    b"antiquewhite",
    b"aqua",
    b"aquamarine",
    b"azure",
    b"beige",
    b"bisque",
    b"black",
    b"blanchedalmond",
    b"blue",
    b"blueviolet",
    b"brown",
    b"burlywood",
    b"cadetblue",
    b"chartreuse",
    b"chocolate",
    b"coral",
    b"cornflowerblue",
    b"cornsilk",
    b"crimson",
    b"cyan",
    b"darkblue",
    b"darkcyan",
    b"darkgoldenrod",
    b"darkgray",
    b"darkgreen",
    b"darkgrey",
    b"darkkhaki",
    b"darkmagenta",
    b"darkolivegreen",
    b"darkorange",
    b"darkorchid",
    b"darkred",
    b"darksalmon",
    b"darkseagreen",
    b"darkslateblue",
    b"darkslategray",
    b"darkslategrey",
    b"darkturquoise",
    b"darkviolet",
    b"deeppink",
    b"deepskyblue",
    b"dimgray",
    b"dimgrey",
    b"dodgerblue",
    b"firebrick",
    b"floralwhite",
    b"forestgreen",
    b"fuchsia",
    b"gainsboro",
    b"ghostwhite",
    b"gold",
    b"goldenrod",
    b"gray",
    b"green",
    b"greenyellow",
    b"grey",
    b"honeydew",
    b"hotpink",
    b"indianred",
    b"indigo",
    b"ivory",
    b"khaki",
    b"lavender",
    b"lavenderblush",
    b"lawngreen",
    b"lemonchiffon",
    b"lightblue",
    b"lightcoral",
    b"lightcyan",
    b"lightgoldenrodyellow",
    b"lightgray",
    b"lightgreen",
    b"lightgrey",
    b"lightpink",
    b"lightsalmon",
    b"lightseagreen",
    b"lightskyblue",
    b"lightslategray",
    b"lightslategrey",
    b"lightsteelblue",
    b"lightyellow",
    b"lime",
    b"limegreen",
    b"linen",
    b"magenta",
    b"maroon",
    b"mediumaquamarine",
    b"mediumblue",
    b"mediumorchid",
    b"mediumpurple",
    b"mediumseagreen",
    b"mediumslateblue",
    b"mediumspringgreen",
    b"mediumturquoise",
    b"mediumvioletred",
    b"midnightblue",
    b"mintcream",
    b"mistyrose",
    b"moccasin",
    b"navajowhite",
    b"navy",
    b"oldlace",
    b"olive",
    b"olivedrab",
    b"orange",
    b"orangered",
    b"orchid",
    b"palegoldenrod",
    b"palegreen",
    b"paleturquoise",
    b"palevioletred",
    b"papayawhip",
    b"peachpuff",
    b"peru",
    b"pink",
    b"plum",
    b"powderblue",
    b"purple",
    b"rebeccapurple",
    b"red",
    b"rosybrown",
    b"royalblue",
    b"saddlebrown",
    b"salmon",
    b"sandybrown",
    b"seagreen",
    b"seashell",
    b"sienna",
    b"silver",
    b"skyblue",
    b"slateblue",
    b"slategray",
    b"slategrey",
    b"snow",
    b"springgreen",
    b"steelblue",
    b"tan",
    b"teal",
    b"thistle",
    b"tomato",
    b"turquoise",
    b"violet",
    b"wheat",
    b"white",
    b"whitesmoke",
    b"yellow",
    b"yellowgreen",
];

// ---------------------------------------------------------------------------
// string helpers

/// strstr(haystack, needle) != NULL (C strings)
fn strstr(haystack: &[u8], needle: &[u8]) -> bool {
    let h = tmbstr::cstr(haystack);
    let n = tmbstr::cstr(needle);
    if n.is_empty() {
        return true;
    }
    h.windows(n.len()).any(|w| w == n)
}

/// atoi() (a NULL value counts as 0)
fn atoi(s: Option<&[u8]>) -> i32 {
    let Some(s) = s else {
        return 0;
    };
    let s = tmbstr::cstr(s);
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add((s[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        v = -v;
    }
    v as i32
}

/// IsFilePath(): is the URI a file rather than a TLD?
fn is_file_path(path: &[u8]) -> bool {
    #[derive(PartialEq)]
    enum St {
        Initial,
        ProtocolFound,
        SlashFound,
        FileFound,
    }
    let mut state = St::Initial;
    for &c in tmbstr::cstr(path) {
        if state == St::FileFound {
            break;
        }
        match state {
            St::Initial => {
                if c == b':' {
                    state = St::ProtocolFound;
                }
            }
            St::ProtocolFound => {
                if c == b'/' {
                    state = St::SlashFound;
                }
            }
            St::SlashFound => {
                if c == b'/' {
                    state = St::ProtocolFound;
                } else {
                    state = St::FileFound;
                }
            }
            St::FileFound => {}
        }
    }
    state == St::FileFound || state == St::Initial
}

/// GetFileExtension(): the extension portion of the path (if any), at most
/// maxExt - 1 bytes
fn get_file_extension(path: &[u8], max_ext: usize) -> Vec<u8> {
    let path = tmbstr::cstr(path);
    let mut i = path.len() as isize - 1;
    if i < 0 {
        return Vec::new();
    }
    loop {
        let c = path[i as usize];
        if c == b'/' || c == b'\\' {
            break;
        } else if c == b'.' {
            let rest = &path[i as usize..];
            return rest[..rest.len().min(max_ext - 1)].to_vec();
        }
        i -= 1;
        if i <= 0 {
            break;
        }
    }
    Vec::new()
}

fn ext_in(ext: &[u8], list: &[&[u8]]) -> Option<usize> {
    list.iter().position(|e| tmbstr::tmbstrcasecmp(ext, e) == 0)
}

/// IsImage()
fn is_image(itype: &[u8]) -> bool {
    if !is_file_path(itype) {
        return false;
    }
    let ext = get_file_extension(itype, 20);
    ext_in(&ext, IMAGE_EXTENSIONS).is_some()
}

/// IsSoundFile(): the error code of the sound type, or 0
fn is_sound_file(stype: &[u8]) -> u32 {
    if !is_file_path(stype) {
        return 0;
    }
    let ext = get_file_extension(stype, 20);
    match ext_in(&ext, SOUND_EXTENSIONS) {
        Some(i) => SOUND_EXT_ERR_CODES[i],
        None => 0,
    }
}

/// IsValidSrcExtension()
fn is_valid_src_extension(stype: &[u8]) -> bool {
    if !is_file_path(stype) {
        return false;
    }
    let ext = get_file_extension(stype, 20);
    ext_in(&ext, FRAME_EXTENSIONS).is_some()
}

/// IsValidMediaExtension()
fn is_valid_media_extension(stype: &[u8]) -> bool {
    if !is_file_path(stype) {
        return false;
    }
    let ext = get_file_extension(stype, 20);
    ext_in(&ext, MEDIA_EXTENSIONS).is_some()
}

/// IsWhitespace(): all white space (a NULL string counts as white space)
fn is_whitespace(s: Option<&[u8]>) -> bool {
    match s {
        None => true,
        Some(s) => tmbstr::cstr(s)
            .iter()
            .all(|&c| lexer::is_white(c as i8 as i32 as u32)),
    }
}

/// hasValue()
fn has_value(doc: &Doc, av: Option<AttId>) -> bool {
    match av {
        Some(a) => !is_whitespace(doc[a].value.as_deref()),
        None => false,
    }
}

/// IsPlaceholderAlt()
fn is_placeholder_alt(txt: &[u8]) -> bool {
    strstr(txt, b"image") || strstr(txt, b"photo")
}

/// IsPlaceHolderObject()
fn is_place_holder_object(txt: &[u8]) -> bool {
    strstr(txt, b"object")
}

/// EndsWithBytes()
fn ends_with_bytes(txt: &[u8]) -> bool {
    let t = tmbstr::cstr(txt);
    t.len() >= 5 && &t[t.len() - 5..] == b"bytes"
}

/// textFromOneNode(): the characters of one text node (at most
/// TEXTBUF_SIZE - 1)
fn text_from_one_node(doc: &Doc, node: Option<NodeId>) -> Vec<u8> {
    let mut txt = Vec::new();
    if let Some(n) = node {
        let lb = &doc.lxr().lexbuf;
        for i in doc[n].start..doc[n].end {
            txt.push(lb.get(i as usize).copied().unwrap_or(0));
            if txt.len() > TEXTBUF_SIZE - 1 {
                txt.pop();
                break;
            }
        }
    }
    tmbstr::cstr(&txt).to_vec()
}

/// getTextNode(): concatenates the text of a text node
fn get_text_node(doc: &mut Doc, node: Option<NodeId>) {
    if tags::node_is_text(doc, node) {
        let n = node.unwrap();
        for i in doc[n].start..doc[n].end {
            if doc.access.counter as usize >= TEXTBUF_SIZE - 1 {
                return;
            }
            let c = doc.lxr().lexbuf.get(i as usize).copied().unwrap_or(0);
            doc.access.text_node.push(c);
            doc.access.counter += 1;
        }
        let mut c = doc[n].content;
        while let Some(x) = c {
            get_text_node(doc, Some(x));
            c = doc[x].next;
        }
    }
}

/// getTextNodeClear()
fn get_text_node_clear(doc: &mut Doc, node: NodeId) -> Vec<u8> {
    doc.access.text_node.clear();
    doc.access.counter = 0;
    let c = doc[node].content;
    get_text_node(doc, c);
    tmbstr::cstr(&doc.access.text_node).to_vec()
}

fn level1_enabled(doc: &Doc) -> bool {
    matches!(doc.access.prioritychk, 1..=3)
}

fn level2_enabled(doc: &Doc) -> bool {
    matches!(doc.access.prioritychk, 2 | 3)
}

fn level3_enabled(doc: &Doc) -> bool {
    doc.access.prioritychk == 3
}

fn report(doc: &mut Doc, node: NodeId, code: u32) {
    message::report_access_error(doc, Some(node), code);
}

fn attrs_of(doc: &Doc, node: NodeId) -> Vec<AttId> {
    doc.attr_list(node)
}

fn value(doc: &Doc, a: AttId) -> Option<Vec<u8>> {
    doc[a].value.clone()
}

fn vlen(doc: &Doc, a: AttId) -> usize {
    doc[a].value.as_deref().map_or(0, |v| tmbstr::cstr(v).len())
}

/// CheckColorAvailable()
fn check_color_available(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        if doc.is(node, TagId::IMG) {
            report(doc, node, INFORMATION_NOT_CONVEYED_IMAGE);
        } else if doc.is(node, TagId::APPLET) {
            report(doc, node, INFORMATION_NOT_CONVEYED_APPLET);
        } else if doc.is(node, TagId::OBJECT) {
            report(doc, node, INFORMATION_NOT_CONVEYED_OBJECT);
        } else if doc.is(node, TagId::SCRIPT) {
            report(doc, node, INFORMATION_NOT_CONVEYED_SCRIPT);
        } else if doc.is(node, TagId::INPUT) {
            report(doc, node, INFORMATION_NOT_CONVEYED_INPUT);
        }
    }
}

/// CheckColorContrast()
fn check_color_contrast(doc: &mut Doc, node: NodeId) {
    let mut rgb_bg = [255, 255, 255];
    if level3_enabled(doc) {
        let mut got_bg = true;
        for av in attrs_of(doc, node) {
            if doc.attr_is(Some(av), AttrId::BGCOLOR) && has_value(doc, Some(av)) {
                got_bg = get_rgb(&value(doc, av).unwrap_or_default(), &mut rgb_bg);
            }
        }
        for av in attrs_of(doc, node) {
            if !got_bg {
                break;
            }
            let mut errcode = 0;
            if doc.attr_is(Some(av), AttrId::TEXT) {
                errcode = COLOR_CONTRAST_TEXT;
            } else if doc.attr_is(Some(av), AttrId::LINK) {
                errcode = COLOR_CONTRAST_LINK;
            } else if doc.attr_is(Some(av), AttrId::ALINK) {
                errcode = COLOR_CONTRAST_ACTIVE_LINK;
            } else if doc.attr_is(Some(av), AttrId::VLINK) {
                errcode = COLOR_CONTRAST_VISITED_LINK;
            }
            if errcode != 0 && has_value(doc, Some(av)) {
                let mut rgb_fg = [0, 0, 0];
                if get_rgb(&value(doc, av).unwrap_or_default(), &mut rgb_fg)
                    && !compare_colors(&rgb_bg, &rgb_fg)
                {
                    report(doc, node, errcode);
                }
            }
        }
    }
}

fn minmax(i1: i32, i2: i32) -> i32 {
    i1.max(i2) - i1.min(i2)
}

fn brightness(rgb: &[i32; 3]) -> i32 {
    ((rgb[0] * 299) + (rgb[1] * 587) + (rgb[2] * 114)) / 1000
}

/// CompareColors(): two RGB colors with good contrast
fn compare_colors(rgb_bg: &[i32; 3], rgb_fg: &[i32; 3]) -> bool {
    let diff_bright = minmax(brightness(rgb_bg), brightness(rgb_fg));
    let diff_color =
        minmax(rgb_bg[0], rgb_fg[0]) + minmax(rgb_bg[1], rgb_fg[1]) + minmax(rgb_bg[2], rgb_fg[2]);
    diff_bright > 180 && diff_color > 500
}

/// GetRgb()
fn get_rgb(color: &[u8], rgb: &mut [i32; 3]) -> bool {
    for (x, name) in COLOR_NAMES.iter().enumerate() {
        if strstr(name, color) {
            *rgb = COLOR_VALUES[x];
            return true;
        }
    }
    let c = tmbstr::cstr(color);
    if c.len() == 7 && c[0] == b'#' {
        rgb[0] = (ctox(c[1]) * 16) + ctox(c[2]);
        rgb[1] = (ctox(c[3]) * 16) + ctox(c[4]);
        rgb[2] = (ctox(c[5]) * 16) + ctox(c[6]);
        return true;
    }
    false
}

/// ctox()
fn ctox(ch: u8) -> i32 {
    match ch {
        b'0'..=b'9' => (ch - b'0') as i32,
        b'a'..=b'f' => (ch - b'a') as i32 + 10,
        b'A'..=b'F' => (ch - b'A') as i32 + 10,
        _ => -1,
    }
}

fn is_d(word: &[u8]) -> bool {
    word == b"d" || word == b"D"
}

/// CheckImage()
fn check_image(doc: &mut Doc, node: NodeId) {
    let mut node = node;
    let mut has_alt = false;
    let mut has_is_map = false;
    let mut has_long_desc = false;
    let mut has_dlink = false;
    let mut has_valid_height = false;
    let mut has_valid_width_bullet = false;
    let mut has_valid_width_hr = false;
    let mut has_triggered_missing_long_desc = false;

    if level1_enabled(doc) {
        for av in attrs_of(doc, node) {
            let a = Some(av);
            if doc.attr_is(a, AttrId::ALT) {
                if let Some(v) = value(doc, av) {
                    let len = tmbstr::cstr(&v).len();
                    if len < 150
                        && !is_placeholder_alt(&v)
                        && !is_place_holder_object(&v)
                        && !ends_with_bytes(&v)
                        && !is_image(&v)
                    {
                        has_alt = true;
                    } else if len > 150 {
                        has_alt = true;
                        report(doc, node, IMG_ALT_SUSPICIOUS_TOO_LONG);
                    } else if is_image(&v) {
                        has_alt = true;
                        report(doc, node, IMG_ALT_SUSPICIOUS_FILENAME);
                    } else if is_placeholder_alt(&v) {
                        has_alt = true;
                        report(doc, node, IMG_ALT_SUSPICIOUS_PLACEHOLDER);
                    } else if ends_with_bytes(&v) {
                        has_alt = true;
                        report(doc, node, IMG_ALT_SUSPICIOUS_FILE_SIZE);
                    }
                }
            } else if doc.attr_is(a, AttrId::WIDTH) {
                if has_value(doc, a) {
                    let width = atoi(doc[av].value.as_deref());
                    if width < 30 {
                        has_valid_width_bullet = true;
                    }
                    if width > 150 {
                        has_valid_width_hr = true;
                    }
                }
            } else if doc.attr_is(a, AttrId::HEIGHT) {
                if has_value(doc, a) && atoi(doc[av].value.as_deref()) < 30 {
                    has_valid_height = true;
                }
            } else if doc.attr_is(a, AttrId::LONGDESC) {
                if has_value(doc, a) && vlen(doc, av) > 1 {
                    has_long_desc = true;
                }
            } else if doc.attr_is(a, AttrId::USEMAP) {
                if has_value(doc, a) {
                    doc.access.has_use_map = true;
                }
            } else if doc.attr_is(a, AttrId::ISMAP) {
                has_is_map = true;
            }
        }

        if doc.is_o(doc[node].next, TagId::A) {
            node = doc[node].next.unwrap();
            if let Some(c) = doc[node].content {
                if doc[c].tag.is_none() {
                    let word = text_from_one_node(doc, Some(c));
                    if is_d(&word) {
                        has_dlink = true;
                    }
                }
            }
        }

        if let Some(nx) = doc[node].next {
            if doc[nx].tag.is_none() {
                node = nx;
                if doc.is_o(doc[node].next, TagId::A) {
                    node = doc[node].next.unwrap();
                    if let Some(c) = doc[node].content {
                        if doc[c].tag.is_none() {
                            let word = text_from_one_node(doc, Some(c));
                            if is_d(&word) {
                                has_dlink = true;
                            }
                        }
                    }
                }
            }
        }

        if !has_alt {
            report(doc, node, IMG_MISSING_ALT);
        }

        if !has_long_desc && has_valid_height && (has_valid_width_hr || has_valid_width_bullet) {
            has_triggered_missing_long_desc = true;
        }

        if !has_triggered_missing_long_desc {
            if has_dlink && !has_long_desc {
                report(doc, node, IMG_MISSING_LONGDESC);
            }
            if has_long_desc && !has_dlink {
                report(doc, node, IMG_MISSING_DLINK);
            }
            if !has_long_desc && !has_dlink {
                report(doc, node, IMG_MISSING_LONGDESC_DLINK);
            }
        }

        if has_is_map {
            report(doc, node, IMAGE_MAP_SERVER_SIDE_REQUIRES_CONVERSION);
            report(doc, node, IMG_MAP_SERVER_REQUIRES_TEXT_LINKS);
        }
    }
}

/// CheckApplet()
fn check_applet(doc: &mut Doc, node: NodeId) {
    let mut has_alt = false;
    let mut has_description = false;
    if level1_enabled(doc) {
        for av in attrs_of(doc, node) {
            if doc.attr_is(Some(av), AttrId::ALT) && doc[av].value.is_some() {
                has_alt = true;
            }
        }
        if !has_alt {
            if let Some(c) = doc[node].content {
                let mut word: Option<Vec<u8>> = None;
                if doc[c].tag.is_none() {
                    word = Some(text_from_one_node(doc, Some(c)));
                }
                if let Some(cc) = doc[c].content {
                    if doc[cc].tag.is_none() {
                        word = Some(text_from_one_node(doc, Some(cc)));
                    }
                }
                if let Some(w) = word {
                    if !is_whitespace(Some(&w)) {
                        has_description = true;
                    }
                }
            }
        }
        if !has_description && !has_alt {
            report(doc, node, APPLET_MISSING_ALT);
        }
    }
}

/// CheckObject()
fn check_object(doc: &mut Doc, node: NodeId) {
    let mut has_alt = false;
    let mut has_description = false;
    if level1_enabled(doc) {
        if let Some(c) = doc[node].content {
            if doc[c].type_ != NodeType::TextNode {
                for av in attrs_of(doc, c) {
                    if doc.attr_is(Some(av), AttrId::ALT) {
                        has_alt = true;
                        break;
                    }
                }
            }
            if !has_alt {
                let mut word: Option<Vec<u8>> = None;
                if tags::node_is_text(doc, Some(c)) {
                    word = Some(text_from_one_node(doc, Some(c)));
                }
                if word.is_none() && tags::node_is_text(doc, doc[c].content) {
                    word = Some(text_from_one_node(doc, doc[c].content));
                }
                if let Some(w) = word {
                    if !is_whitespace(Some(&w)) {
                        has_description = true;
                    }
                }
            }
        }
        if !has_alt && !has_description {
            report(doc, node, OBJECT_MISSING_ALT);
        }
    }
}

/// CheckMissingStyleSheets()
fn check_missing_style_sheets(doc: &mut Doc, node: NodeId) -> bool {
    let mut sspresent = false;
    let mut content = doc[node].content;
    while let Some(c) = content {
        if sspresent {
            break;
        }
        sspresent = doc.is(c, TagId::LINK)
            || doc.is(c, TagId::STYLE)
            || doc.is(c, TagId::FONT)
            || doc.is(c, TagId::BASEFONT);
        for av in attrs_of(doc, c) {
            if sspresent {
                break;
            }
            let a = Some(av);
            sspresent = doc.attr_is(a, AttrId::STYLE)
                || doc.attr_is(a, AttrId::TEXT)
                || doc.attr_is(a, AttrId::VLINK)
                || doc.attr_is(a, AttrId::ALINK)
                || doc.attr_is(a, AttrId::LINK);
            if !sspresent && doc.attr_is(a, AttrId::REL) {
                sspresent = doc.attr_value_is(a, b"stylesheet");
            }
        }
        if !sspresent {
            sspresent = check_missing_style_sheets(doc, c);
        }
        content = doc[c].next;
    }
    sspresent
}

/// CheckFrame()
fn check_frame(doc: &mut Doc, node: NodeId) {
    let mut has_title = false;
    doc.access.num_frames += 1;
    if level1_enabled(doc) {
        for av in attrs_of(doc, node) {
            let a = Some(av);
            if doc.attr_is(a, AttrId::LONGDESC) {
                if has_value(doc, a) && vlen(doc, av) > 1 {
                    doc.access.has_checked_long_desc += 1;
                }
            } else if doc.attr_is(a, AttrId::SRC) {
                if has_value(doc, a) && !is_valid_src_extension(&value(doc, av).unwrap_or_default())
                {
                    report(doc, node, FRAME_SRC_INVALID);
                }
            } else if doc.attr_is(a, AttrId::TITLE) {
                if has_value(doc, a) {
                    has_title = true;
                }
                if !has_title {
                    if doc[av].value.is_none() || vlen(doc, av) == 0 {
                        has_title = true;
                        report(doc, node, FRAME_TITLE_INVALID_NULL);
                    } else if is_whitespace(doc[av].value.as_deref()) && vlen(doc, av) > 0 {
                        has_title = true;
                        report(doc, node, FRAME_TITLE_INVALID_SPACES);
                    }
                }
            }
        }
        if !has_title {
            report(doc, node, FRAME_MISSING_TITLE);
        }
        if doc.access.num_frames == 3 && doc.access.has_checked_long_desc < 3 {
            doc.access.num_frames = 0;
            report(doc, node, FRAME_MISSING_LONGDESC);
        }
    }
}

/// CheckIFrame()
fn check_iframe(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let av = attrs::attr_get_by_id(doc, node, AttrId::SRC);
        if has_value(doc, av)
            && !is_valid_src_extension(&value(doc, av.unwrap()).unwrap_or_default())
        {
            report(doc, node, FRAME_SRC_INVALID);
        }
    }
}

/// CheckAnchorAccess()
fn check_anchor_access(doc: &mut Doc, node: NodeId) {
    let mut has_description = false;
    let mut has_triggered_link = false;

    for av in attrs_of(doc, node) {
        let a = Some(av);
        if level1_enabled(doc) && doc.attr_is(a, AttrId::HREF) && has_value(doc, a) {
            let v = value(doc, av).unwrap_or_default();
            let ext = get_file_extension(&v, 20);
            if is_valid_media_extension(&v) {
                report(doc, node, MULTIMEDIA_REQUIRES_TEXT);
            }
            if ext.len() < 6 && !ext.is_empty() {
                let errcode = is_sound_file(&v);
                if errcode != 0 {
                    if let Some(nx) = doc[node].next {
                        if doc[nx].tag.is_none() {
                            let word = text_from_one_node(doc, Some(nx));
                            if !is_whitespace(Some(&word)) {
                                has_description = true;
                            }
                        }
                    }
                    if !has_description {
                        report(doc, node, errcode);
                    }
                }
            }
        }
        if level2_enabled(doc) && doc.attr_is(a, AttrId::TARGET) {
            if doc.attr_value_is(a, b"_new") {
                report(doc, node, NEW_WINDOWS_REQUIRE_WARNING_NEW);
            } else if doc.attr_value_is(a, b"_blank") {
                report(doc, node, NEW_WINDOWS_REQUIRE_WARNING_BLANK);
            }
        }
    }

    if level2_enabled(doc) {
        if let Some(c) = doc[node].content {
            if doc[c].tag.is_none() {
                let word = text_from_one_node(doc, Some(c));
                if !is_whitespace(Some(&word)) {
                    if word == b"more" {
                        has_triggered_link = true;
                    }
                    if word == b"click here" {
                        report(doc, node, LINK_TEXT_NOT_MEANINGFUL_CLICK_HERE);
                    }
                    if !has_triggered_link && word.len() < 6 {
                        report(doc, node, LINK_TEXT_NOT_MEANINGFUL);
                    }
                    if word.len() > 60 {
                        report(doc, node, LINK_TEXT_TOO_LONG);
                    }
                }
            }
        }
        if doc[node].content.is_none() {
            report(doc, node, LINK_TEXT_MISSING);
        }
    }
}

/// CheckArea()
fn check_area(doc: &mut Doc, node: NodeId) {
    let mut has_alt = false;
    for av in attrs_of(doc, node) {
        let a = Some(av);
        if level1_enabled(doc) && doc.attr_is(a, AttrId::ALT) && doc[av].value.is_some() {
            has_alt = true;
        }
        if level2_enabled(doc) && doc.attr_is(a, AttrId::TARGET) {
            if doc.attr_value_is(a, b"_new") {
                report(doc, node, NEW_WINDOWS_REQUIRE_WARNING_NEW);
            } else if doc.attr_value_is(a, b"_blank") {
                report(doc, node, NEW_WINDOWS_REQUIRE_WARNING_BLANK);
            }
        }
    }
    if level1_enabled(doc) && !has_alt {
        report(doc, node, AREA_MISSING_ALT);
    }
}

/// CheckScriptAcc()
fn check_script_acc(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) && !doc.is_o(doc[node].next, TagId::NOSCRIPT) {
        report(doc, node, SCRIPT_MISSING_NOSCRIPT);
    }
}

/// CheckRows()
fn check_rows(doc: &mut Doc, node: Option<NodeId>) {
    let mut num_tr = 0;
    let mut num_valid_th = 0;
    doc.access.checked_headers += 1;
    let mut node = node;
    while let Some(n) = node {
        num_tr += 1;
        if doc.is_o(doc[n].content, TagId::TH) {
            doc.access.has_th = true;
            let c = doc[n].content.unwrap();
            if tags::node_is_text(doc, doc[c].content) {
                let word = text_from_one_node(doc, doc[c].content);
                if !is_whitespace(Some(&word)) {
                    num_valid_th += 1;
                }
            }
        }
        node = doc[n].next;
    }
    if num_tr == num_valid_th {
        doc.access.has_valid_row_headers = true;
    }
    if num_tr >= 2 && num_tr > num_valid_th && num_valid_th >= 2 && doc.access.has_th {
        doc.access.has_invalid_row_header = true;
    }
}

/// CheckColumns()
fn check_columns(doc: &mut Doc, node: NodeId) {
    let mut num_th = 0;
    let mut is_missing_header = false;
    doc.access.checked_headers += 1;
    if doc.is_o(doc[node].content, TagId::TH) {
        doc.access.has_th = true;
        let mut tnode = doc[node].content;
        while let Some(t) = tnode {
            if doc.is(t, TagId::TH) {
                if tags::node_is_text(doc, doc[t].content) {
                    let word = text_from_one_node(doc, doc[t].content);
                    if !is_whitespace(Some(&word)) {
                        num_th += 1;
                    }
                }
            } else {
                is_missing_header = true;
            }
            tnode = doc[t].next;
        }
    }
    if !is_missing_header && num_th > 0 {
        doc.access.has_valid_column_headers = true;
    }
    if is_missing_header && num_th >= 2 {
        doc.access.has_invalid_column_header = true;
    }
}

/// CheckTH()
fn check_th(doc: &mut Doc, node: NodeId) {
    let mut has_abbr = false;
    if level3_enabled(doc) {
        for av in attrs_of(doc, node) {
            if doc.attr_is(Some(av), AttrId::ABBR) {
                let v = value(doc, av);
                if v.is_some() && !is_whitespace(v.as_deref()) {
                    has_abbr = true;
                }
                if v.is_none() || vlen(doc, av) == 0 {
                    has_abbr = true;
                    report(doc, node, TABLE_MAY_REQUIRE_HEADER_ABBR_NULL);
                }
                if is_whitespace(v.as_deref()) && vlen(doc, av) > 0 {
                    has_abbr = true;
                    report(doc, node, TABLE_MAY_REQUIRE_HEADER_ABBR_SPACES);
                }
            }
        }
        let word = text_from_one_node(doc, doc[node].content);
        if !is_whitespace(Some(&word)) && word.len() > 15 && !has_abbr {
            report(doc, node, TABLE_MAY_REQUIRE_HEADER_ABBR);
        }
    }
}

/// CheckMultiHeaders()
fn check_multi_headers(doc: &mut Doc, node: NodeId) {
    let mut valid_col_span_rows = true;
    let mut valid_col_span_columns = true;
    if level1_enabled(doc) {
        if doc[node].content.is_some() {
            let mut tnode = doc[node].content;
            while let Some(t) = tnode {
                if doc.is(t, TagId::TR) {
                    let mut flag = 0;
                    if doc[t].content.is_some() {
                        let mut temp = doc[t].content;
                        if flag == 0 {
                            while let Some(x) = temp {
                                if doc.is(x, TagId::TH) {
                                    for av in attrs_of(doc, x) {
                                        if doc.attr_is(Some(av), AttrId::COLSPAN)
                                            && atoi(doc[av].value.as_deref()) > 1
                                        {
                                            valid_col_span_columns = false;
                                        }
                                        if doc.attr_is(Some(av), AttrId::ROWSPAN)
                                            && atoi(doc[av].value.as_deref()) > 1
                                        {
                                            valid_col_span_rows = false;
                                        }
                                    }
                                }
                                temp = doc[x].next;
                            }
                            flag = 1;
                        }
                    }
                    let _ = flag;
                }
                tnode = doc[t].next;
            }
            if !valid_col_span_rows {
                report(doc, node, DATA_TABLE_REQUIRE_MARKUP_ROW_HEADERS);
                message::dialogue(doc, TEXT_HTML_T_ALGORITHM, &[]);
            }
            if !valid_col_span_columns {
                report(doc, node, DATA_TABLE_REQUIRE_MARKUP_COLUMN_HEADERS);
                message::dialogue(doc, TEXT_HTML_T_ALGORITHM, &[]);
            }
        }
    }
}

/// CheckTable()
fn check_table(doc: &mut Doc, node: NodeId) {
    let mut num_tr = 0;
    let mut has_summary = false;
    let mut has_caption = false;

    if level3_enabled(doc) {
        for av in attrs_of(doc, node) {
            let a = Some(av);
            if doc.attr_is(a, AttrId::SUMMARY) {
                if has_value(doc, a) {
                    has_summary = true;
                    if doc.attr_contains(a, b"summary") && doc.attr_contains(a, b"table") {
                        report(doc, node, TABLE_SUMMARY_INVALID_PLACEHOLDER);
                    }
                }
                if doc[av].value.is_none() || vlen(doc, av) == 0 {
                    has_summary = true;
                    report(doc, node, TABLE_SUMMARY_INVALID_NULL);
                } else if is_whitespace(doc[av].value.as_deref()) && vlen(doc, av) > 0 {
                    has_summary = true;
                    report(doc, node, TABLE_SUMMARY_INVALID_SPACES);
                }
            }
        }
        if doc[node].content.is_none() {
            report(doc, node, DATA_TABLE_MISSING_HEADERS);
            return;
        }
    }

    if level1_enabled(doc) {
        check_multi_headers(doc, node);
    }

    if level2_enabled(doc) {
        if doc.is_o(doc[node].content, TagId::CAPTION) {
            let tnode = doc[node].content.unwrap();
            let mut word: Option<Vec<u8>> = None;
            if let Some(c) = doc[tnode].content {
                if doc[c].tag.is_none() {
                    word = Some(get_text_node_clear(doc, tnode));
                }
            }
            if !is_whitespace(word.as_deref()) {
                has_caption = true;
            }
        }
        if !has_caption {
            report(doc, node, TABLE_MISSING_CAPTION);
        }
    }

    if let Some(c) = doc[node].content {
        if doc.is(c, TagId::CAPTION) && doc.is_o(doc[c].next, TagId::TR) {
            let n = doc[c].next.unwrap();
            check_columns(doc, n);
        } else if doc.is(c, TagId::TR) {
            check_columns(doc, c);
        }
    }

    if !doc.access.has_valid_column_headers {
        if let Some(c) = doc[node].content {
            if doc.is(c, TagId::CAPTION) && doc.is_o(doc[c].next, TagId::TR) {
                let n = doc[c].next;
                check_rows(doc, n);
            } else if doc.is(c, TagId::TR) {
                check_rows(doc, Some(c));
            }
        }
    }

    if level3_enabled(doc) && !has_summary {
        report(doc, node, TABLE_MISSING_SUMMARY);
    }

    if level2_enabled(doc) {
        if doc[node].content.is_some() {
            let mut temp = doc[node].content;
            while let Some(t) = temp {
                if doc.is(t, TagId::TR) {
                    num_tr += 1;
                }
                temp = doc[t].next;
            }
            if num_tr == 1 {
                report(doc, node, LAYOUT_TABLES_LINEARIZE_PROPERLY);
            }
        }
        if doc.access.has_th {
            report(doc, node, LAYOUT_TABLE_INVALID_MARKUP);
        }
    }

    if level1_enabled(doc) && doc.access.checked_headers == 2 {
        let a = &doc.access;
        let (vr, vc, ir, ic) = (
            a.has_valid_row_headers,
            a.has_valid_column_headers,
            a.has_invalid_row_header,
            a.has_invalid_column_header,
        );
        if !vr && !vc && !ir && !ic {
            report(doc, node, DATA_TABLE_MISSING_HEADERS);
        }
        if !vr && ir {
            report(doc, node, DATA_TABLE_MISSING_HEADERS_ROW);
        }
        if !vc && ic {
            report(doc, node, DATA_TABLE_MISSING_HEADERS_COLUMN);
        }
    }
}

/// CheckASCII()
fn check_ascii(doc: &mut Doc, node: NodeId) {
    let mut skip_over: Option<Vec<u8>> = None;
    let mut is_ascii = false;
    let mut has_skip_over_link = 0;
    let mut new_lines: i32 = -1;
    let mut matching_count = 0;

    if level1_enabled(doc) {
        if let Some(c) = doc[node].content {
            let (start, end) = (doc[c].start, doc[c].end);
            let lb = &doc.lxr().lexbuf;
            let at = |x: u32| lb.get(x as usize).copied().unwrap_or(0);
            let mut i = start.wrapping_add(1);
            while i < end {
                matching_count = 0;
                if at(i) == b'\n' {
                    new_lines += 1;
                }
                let compare_letter = at(i);
                let mut x = i;
                while x < i.wrapping_add(5) {
                    if at(x) == compare_letter {
                        matching_count += 1;
                    } else {
                        break;
                    }
                    x += 1;
                }
                if matching_count >= 5 {
                    break;
                }
                i += 1;
            }

            if new_lines >= 6 || matching_count >= 5 {
                is_ascii = true;
            }

            if is_ascii {
                if let Some(p) = doc[node].prev {
                    if let Some(temp1) = doc[p].prev {
                        for av in attrs_of(doc, temp1) {
                            if doc.attr_is(Some(av), AttrId::HREF) && has_value(doc, Some(av)) {
                                skip_over = value(doc, av);
                                has_skip_over_link += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    if level2_enabled(doc) {
        if has_skip_over_link == 1 && doc.is_o(doc[node].next, TagId::A) {
            let temp2 = doc[node].next.unwrap();
            for av in attrs_of(doc, temp2) {
                if doc.attr_is(Some(av), AttrId::NAME) && has_value(doc, Some(av)) {
                    let v = value(doc, av).unwrap_or_default();
                    if strstr(skip_over.as_deref().unwrap_or_default(), &v) {
                        has_skip_over_link += 1;
                    }
                }
            }
        }
        if is_ascii {
            report(doc, node, ASCII_REQUIRES_DESCRIPTION);
            if level3_enabled(doc) && has_skip_over_link < 2 {
                report(doc, node, SKIPOVER_ASCII_ART);
            }
        }
    }
}

/// CheckFormControls()
fn check_form_controls(doc: &mut Doc, node: NodeId) {
    let (vf, vi) = (doc.access.has_valid_for, doc.access.has_valid_id);
    if !vf && vi {
        report(doc, node, ASSOCIATE_LABELS_EXPLICITLY_FOR);
    }
    if !vi && vf {
        report(doc, node, ASSOCIATE_LABELS_EXPLICITLY_ID);
    }
    if !vi && !vf {
        report(doc, node, ASSOCIATE_LABELS_EXPLICITLY);
    }
}

/// CheckLabel()
fn check_label(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) {
        let av = attrs::attr_get_by_id(doc, node, AttrId::FOR);
        if has_value(doc, av) {
            doc.access.has_valid_for = true;
        }
        doc.access.for_id += 1;
        if doc.access.for_id == 2 {
            doc.access.for_id = 0;
            check_form_controls(doc, node);
        }
    }
}

/// CheckInputLabel()
fn check_input_label(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) {
        for av in attrs_of(doc, node) {
            if doc.attr_is(Some(av), AttrId::ID) && has_value(doc, Some(av)) {
                doc.access.has_valid_id = true;
            }
        }
        doc.access.for_id += 1;
        if doc.access.for_id == 2 {
            doc.access.for_id = 0;
            check_form_controls(doc, node);
        }
    }
}

/// CheckInputAttributes()
fn check_input_attributes(doc: &mut Doc, node: NodeId) {
    let mut has_alt = false;
    let mut must_have_alt = false;
    for av in attrs_of(doc, node) {
        let a = Some(av);
        if doc.attr_is(a, AttrId::TYPE)
            && has_value(doc, a)
            && level1_enabled(doc)
            && doc.attr_value_is(a, b"image")
        {
            must_have_alt = true;
        }
        if doc.attr_is(a, AttrId::ALT) && has_value(doc, a) {
            has_alt = true;
        }
    }
    if must_have_alt && !has_alt {
        report(doc, node, IMG_BUTTON_MISSING_ALT);
    }
}

/// CheckFrameSet()
fn check_frame_set(doc: &mut Doc, node: NodeId) {
    let mut has_no_frames = false;
    if level1_enabled(doc) {
        if doc.bad_access & BA_INVALID_LINK_NOFRAMES != 0 {
            report(doc, node, NOFRAMES_INVALID_LINK);
            doc.bad_access &= !BA_INVALID_LINK_NOFRAMES;
        }
        let mut temp = doc[node].content;
        while let Some(t) = temp {
            if doc.is(t, TagId::NOFRAMES) {
                has_no_frames = true;
                let tc = doc[t].content;
                let para = tc
                    .and_then(|c| doc[c].content)
                    .filter(|&p| doc.is(p, TagId::P));
                if let Some(para) = para {
                    if tags::node_is_text(doc, doc[para].content) {
                        let word = text_from_one_node(doc, doc[para].content);
                        if strstr(&word, b"browser") {
                            report(doc, para, NOFRAMES_INVALID_CONTENT);
                        }
                    }
                } else if tc.is_none() || is_whitespace(Some(&text_from_one_node(doc, tc))) {
                    report(doc, t, NOFRAMES_INVALID_NO_VALUE);
                }
            }
            temp = doc[t].next;
        }
        if !has_no_frames {
            report(doc, node, FRAME_MISSING_NOFRAMES);
        }
    }
}

/// CheckHeaderNesting()
fn check_header_nesting(doc: &mut Doc, node: NodeId) {
    let mut num_words = 1;
    let mut is_valid_increase = false;
    let mut needs_description = false;
    if level2_enabled(doc) {
        if let Some(c) = doc[node].content {
            if doc[c].tag.is_none() {
                let word = text_from_one_node(doc, Some(c));
                num_words += word.iter().filter(|&&b| b == b' ').count();
                if num_words > 20 {
                    needs_description = true;
                }
            }
        }
        if tags::node_is_header(doc, Some(node)) {
            let level = tags::node_header_level(doc, Some(node));
            is_valid_increase = true;
            let mut temp = doc[node].next;
            while let Some(t) = temp {
                let nested = tags::node_header_level(doc, Some(t));
                if nested >= level {
                    is_valid_increase = nested <= level + 1;
                    break;
                }
                temp = doc[t].next;
            }
        }
        if !is_valid_increase {
            report(doc, node, HEADERS_IMPROPERLY_NESTED);
        }
        if needs_description {
            report(doc, node, HEADER_USED_FORMAT_TEXT);
        }
    }
}

/// CheckParagraphHeader()
fn check_paragraph_header(doc: &mut Doc, node: NodeId) {
    let mut is_not_header = false;
    if level2_enabled(doc) {
        if let Some(c) = doc[node].content {
            if doc[c].tag.is_some() {
                let mut temp = Some(c);
                while let Some(t) = temp {
                    if doc[t].tag.is_none() {
                        is_not_header = true;
                        break;
                    }
                    temp = doc[t].next;
                }
            }
            if !is_not_header {
                if doc.is(c, TagId::STRONG) {
                    report(doc, node, POTENTIAL_HEADER_BOLD);
                }
                if doc.is(c, TagId::U) {
                    report(doc, node, POTENTIAL_HEADER_UNDERLINE);
                }
                if doc.is(c, TagId::EM) {
                    report(doc, node, POTENTIAL_HEADER_ITALICS);
                }
            }
        }
    }
}

/// CheckEmbed()
fn check_embed(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let av = attrs::attr_get_by_id(doc, node, AttrId::SRC);
        if has_value(doc, av)
            && is_valid_media_extension(&value(doc, av.unwrap()).unwrap_or_default())
        {
            report(doc, node, MULTIMEDIA_REQUIRES_TEXT);
        }
    }
}

/// CheckHTMLAccess()
fn check_html_access(doc: &mut Doc, node: NodeId) {
    let mut valid_lang = false;
    if level3_enabled(doc) {
        let av = attrs::attr_get_by_id(doc, node, AttrId::LANG);
        if av.is_some() {
            valid_lang = true;
            if !has_value(doc, av) {
                report(doc, node, LANGUAGE_INVALID);
            }
        }
        if !valid_lang {
            report(doc, node, LANGUAGE_NOT_IDENTIFIED);
        }
    }
}

/// CheckBlink()
fn check_blink(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) && tags::node_is_text(doc, doc[node].content) {
        let word = text_from_one_node(doc, doc[node].content);
        if !is_whitespace(Some(&word)) {
            report(doc, node, REMOVE_BLINK_MARQUEE);
        }
    }
}

/// CheckMarquee(): (sic) tests the MARQUEE node itself for being text
fn check_marquee(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) && tags::node_is_text(doc, Some(node)) {
        let word = text_from_one_node(doc, doc[node].content);
        if !is_whitespace(Some(&word)) {
            report(doc, node, REMOVE_BLINK_MARQUEE);
        }
    }
}

/// CheckLink()
fn check_link(doc: &mut Doc, node: NodeId) {
    let mut has_rel = false;
    let mut has_type = false;
    if level1_enabled(doc) {
        for av in attrs_of(doc, node) {
            let a = Some(av);
            if doc.attr_is(a, AttrId::REL)
                && has_value(doc, a)
                && doc.attr_contains(a, b"stylesheet")
            {
                has_rel = true;
            }
            if doc.attr_is(a, AttrId::TYPE) && has_value(doc, a) {
                has_type = true;
            }
        }
        if has_rel && has_type {
            report(doc, node, STYLESHEETS_REQUIRE_TESTING_LINK);
        }
    }
}

/// CheckStyle()
fn check_style(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        report(doc, node, STYLESHEETS_REQUIRE_TESTING_STYLE_ELEMENT);
    }
}

/// DynamicContent()
fn dynamic_content(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let msgcode = if doc.is(node, TagId::APPLET) {
            TEXT_EQUIVALENTS_REQUIRE_UPDATING_APPLET
        } else if doc.is(node, TagId::SCRIPT) {
            TEXT_EQUIVALENTS_REQUIRE_UPDATING_SCRIPT
        } else if doc.is(node, TagId::OBJECT) {
            TEXT_EQUIVALENTS_REQUIRE_UPDATING_OBJECT
        } else {
            0
        };
        if msgcode != 0 {
            report(doc, node, msgcode);
        }
    }
}

/// ProgrammaticObjects()
fn programmatic_objects(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let msgcode = if doc.is(node, TagId::SCRIPT) {
            PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_SCRIPT
        } else if doc.is(node, TagId::OBJECT) {
            PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_OBJECT
        } else if doc.is(node, TagId::EMBED) {
            PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_EMBED
        } else if doc.is(node, TagId::APPLET) {
            PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_APPLET
        } else {
            0
        };
        if msgcode != 0 {
            report(doc, node, msgcode);
        }
    }
}

/// AccessibleCompatible()
fn accessible_compatible(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let msgcode = if doc.is(node, TagId::SCRIPT) {
            ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_SCRIPT
        } else if doc.is(node, TagId::OBJECT) {
            ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_OBJECT
        } else if doc.is(node, TagId::EMBED) {
            ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_EMBED
        } else if doc.is(node, TagId::APPLET) {
            ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_APPLET
        } else {
            0
        };
        if msgcode != 0 {
            report(doc, node, msgcode);
        }
    }
}

/// CheckFlicker()
fn check_flicker(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let mut msgcode = 0;
        if doc.is(node, TagId::SCRIPT) {
            msgcode = REMOVE_FLICKER_SCRIPT;
        } else if doc.is(node, TagId::OBJECT) {
            msgcode = REMOVE_FLICKER_OBJECT;
        } else if doc.is(node, TagId::EMBED) {
            msgcode = REMOVE_FLICKER_EMBED;
        } else if doc.is(node, TagId::APPLET) {
            msgcode = REMOVE_FLICKER_APPLET;
        } else if doc.is(node, TagId::IMG) {
            let av = attrs::attr_get_by_id(doc, node, AttrId::SRC);
            if has_value(doc, av) {
                let ext = get_file_extension(&value(doc, av.unwrap()).unwrap_or_default(), 20);
                if tmbstr::tmbstrcasecmp(&ext, b".gif") == 0 {
                    msgcode = REMOVE_FLICKER_ANIMATED_GIF;
                }
            }
        }
        if msgcode != 0 {
            report(doc, node, msgcode);
        }
    }
}

/// CheckDeprecated()
fn check_deprecated(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) {
        let msgcode = match doc.tag_id(Some(node)) {
            TagId::APPLET => REPLACE_DEPRECATED_HTML_APPLET,
            TagId::BASEFONT => REPLACE_DEPRECATED_HTML_BASEFONT,
            TagId::CENTER => REPLACE_DEPRECATED_HTML_CENTER,
            TagId::DIR => REPLACE_DEPRECATED_HTML_DIR,
            TagId::FONT => REPLACE_DEPRECATED_HTML_FONT,
            TagId::ISINDEX => REPLACE_DEPRECATED_HTML_ISINDEX,
            TagId::MENU => REPLACE_DEPRECATED_HTML_MENU,
            TagId::S => REPLACE_DEPRECATED_HTML_S,
            TagId::STRIKE => REPLACE_DEPRECATED_HTML_STRIKE,
            TagId::U => REPLACE_DEPRECATED_HTML_U,
            _ => 0,
        };
        if msgcode != 0 {
            report(doc, node, msgcode);
        }
    }
}

/// CheckScriptKeyboardAccessible()
fn check_script_keyboard_accessible(doc: &mut Doc, node: NodeId) {
    let mut has_on_mouse_down = 0;
    let mut has_on_mouse_up = 0;
    let mut has_on_click = 0;
    let mut has_on_mouse_out = 0;
    let mut has_on_mouse_over = 0;
    let mut has_on_mouse_move = 0;

    if level2_enabled(doc) {
        for av in attrs_of(doc, node) {
            let a = Some(av);
            if doc.attr_is(a, AttrId::OnMOUSEDOWN) {
                has_on_mouse_down += 1;
            }
            if doc.attr_is(a, AttrId::OnMOUSEUP) {
                has_on_mouse_up += 1;
            }
            if doc.attr_is(a, AttrId::OnCLICK) {
                has_on_click += 1;
            }
            if doc.attr_is(a, AttrId::OnMOUSEOUT) {
                has_on_mouse_out += 1;
            }
            if doc.attr_is(a, AttrId::OnMOUSEOVER) {
                has_on_mouse_over += 1;
            }
            if doc.attr_is(a, AttrId::OnMOUSEMOVE) {
                has_on_mouse_move += 1;
            }
            if doc.attr_is(a, AttrId::OnKEYDOWN) {
                has_on_mouse_down += 1;
            }
            if doc.attr_is(a, AttrId::OnKEYUP) {
                has_on_mouse_up += 1;
            }
            if doc.attr_is(a, AttrId::OnKEYPRESS) {
                has_on_click += 1;
            }
            if doc.attr_is(a, AttrId::OnBLUR) {
                has_on_mouse_out += 1;
            }
        }
        if has_on_mouse_down == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_DOWN);
        }
        if has_on_mouse_up == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_UP);
        }
        if has_on_click == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_CLICK);
        }
        if has_on_mouse_out == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OUT);
        }
        if has_on_mouse_over == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OVER);
        }
        if has_on_mouse_move == 1 {
            report(doc, node, SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_MOVE);
        }
        let mut content = doc[node].content;
        while let Some(c) = content {
            check_script_keyboard_accessible(doc, c);
            content = doc[c].next;
        }
    }
}

/// CheckMetaData()
fn check_meta_data(doc: &mut Doc, node: NodeId, has_meta_data: bool) -> bool {
    let mut has_meta_data = has_meta_data;
    let mut has_http_equiv = false;
    let mut has_content = false;
    let mut contains_attr = false;

    if level2_enabled(doc) {
        if doc.is(node, TagId::META) {
            for av in attrs_of(doc, node) {
                let a = Some(av);
                if doc.attr_is(a, AttrId::HTTP_EQUIV) && has_value(doc, a) {
                    contains_attr = true;
                    if doc.attr_value_is(a, b"refresh") {
                        has_http_equiv = true;
                        report(doc, node, REMOVE_AUTO_REFRESH);
                    }
                }
                if doc.attr_is(a, AttrId::CONTENT) && has_value(doc, a) {
                    contains_attr = true;
                    let v = value(doc, av).unwrap_or_default();
                    if tmbstr::tmbstrncmp(&v, b"http:", 5) == 0 {
                        has_content = true;
                        report(doc, node, REMOVE_AUTO_REDIRECT);
                    }
                }
                if tags::is_html5_mode(doc) && doc.attr_is(a, AttrId::CHARSET) && has_value(doc, a)
                {
                    contains_attr = true;
                }
            }
            if has_content || has_http_equiv {
                has_meta_data = true;
                report(doc, node, METADATA_MISSING_REDIRECT_AUTOREFRESH);
            } else if contains_attr && !has_content && !has_http_equiv {
                has_meta_data = true;
            }
        }

        if !has_meta_data && doc.is(node, TagId::ADDRESS) && doc.is_o(doc[node].content, TagId::A) {
            has_meta_data = true;
        }

        if !has_meta_data
            && !doc.is(node, TagId::TITLE)
            && tags::node_is_text(doc, doc[node].content)
        {
            let word = text_from_one_node(doc, doc[node].content);
            if !is_whitespace(Some(&word)) {
                has_meta_data = true;
            }
        }

        if !has_meta_data && doc.is(node, TagId::LINK) {
            let av = attrs::attr_get_by_id(doc, node, AttrId::REL);
            if !doc.attr_contains(av, b"stylesheet") {
                has_meta_data = true;
            }
        }

        let mut n = doc[node].content;
        while let Some(c) = n {
            has_meta_data = check_meta_data(doc, c, has_meta_data);
            n = doc[c].next;
        }
    }
    has_meta_data
}

/// MetaDataPresent()
fn meta_data_present(doc: &mut Doc, node: NodeId) {
    if level2_enabled(doc) {
        report(doc, node, METADATA_MISSING);
    }
}

/// CheckDocType()
fn check_doc_type(doc: &mut Doc) {
    if level2_enabled(doc) {
        let mut dtnode = lexer::find_doc_type(doc);
        if let Some(dt) = dtnode {
            if doc[dt].end != 0 {
                let word = text_from_one_node(doc, Some(dt));
                if tags::is_html5_mode(doc) {
                    if !strstr(&word, b"HTML") && !strstr(&word, b"html") {
                        dtnode = None;
                    }
                } else if !strstr(&word, b"HTML PUBLIC") && !strstr(&word, b"html PUBLIC") {
                    dtnode = None;
                }
            }
        }
        if dtnode.is_none() {
            let root = doc.root;
            report(doc, root, DOCTYPE_MISSING);
        }
    }
}

/// FindLinkA()
fn find_link_a(doc: &Doc, node: NodeId, url: &[u8]) -> bool {
    let mut found = false;
    let mut n = doc[node].content;
    while let Some(c) = n {
        if found {
            break;
        }
        if doc.is(c, TagId::A) {
            let href = attrs::attr_get_by_id(doc, c, AttrId::HREF);
            found = has_value(doc, href)
                && tmbstr::tmbstrcmp(url, doc[href.unwrap()].value.as_deref().unwrap_or_default())
                    == 0;
        } else {
            found = find_link_a(doc, c, url);
        }
        n = doc[c].next;
    }
    found
}

/// CheckMapLinks()
fn check_map_links(doc: &mut Doc, node: NodeId) {
    if !level3_enabled(doc) {
        return;
    }
    let mut child = doc[node].content;
    while let Some(c) = child {
        if doc.is(c, TagId::AREA) {
            let href = attrs::attr_get_by_id(doc, c, AttrId::HREF);
            if has_value(doc, href) {
                let url = value(doc, href.unwrap()).unwrap_or_default();
                let root = doc.root;
                if !find_link_a(doc, root, &url) {
                    report(doc, node, IMG_MAP_CLIENT_MISSING_TEXT_LINKS);
                }
            }
        }
        child = doc[c].next;
    }
}

/// CheckForStyleAttribute()
fn check_for_style_attribute(doc: &mut Doc, node: NodeId) {
    if level1_enabled(doc) {
        let style = attrs::attr_get_by_id(doc, node, AttrId::STYLE);
        if has_value(doc, style) {
            report(doc, node, STYLESHEETS_REQUIRE_TESTING_STYLE_ATTR);
        }
    }
    let mut content = doc[node].content;
    while let Some(c) = content {
        check_for_style_attribute(doc, c);
        content = doc[c].next;
    }
}

/// CheckForListElements()
fn check_for_list_elements(doc: &mut Doc, node: NodeId) {
    if doc.is(node, TagId::LI) {
        doc.access.list_elements += 1;
    } else if doc.is(node, TagId::OL) || doc.is(node, TagId::UL) {
        doc.access.other_list_elements += 1;
    }
    let mut n = doc[node].content;
    while let Some(c) = n {
        check_for_list_elements(doc, c);
        n = doc[c].next;
    }
}

/// CheckListUsage()
fn check_list_usage(doc: &mut Doc, node: NodeId) {
    if !level2_enabled(doc) {
        return;
    }
    let mut msgcode = 0;
    if doc.is(node, TagId::OL) {
        msgcode = LIST_USAGE_INVALID_OL;
    } else if doc.is(node, TagId::UL) {
        msgcode = LIST_USAGE_INVALID_UL;
    }
    if msgcode != 0 {
        if !doc.is_o(doc[node].content, TagId::LI) {
            report(doc, node, msgcode);
        } else if doc[node].implicit {
            report(doc, node, LIST_USAGE_INVALID_LI);
        }
    } else if doc.is(node, TagId::LI) {
        let parent = doc[node].parent;
        let parent_is_list = doc.is_o(parent, TagId::OL) || doc.is_o(parent, TagId::UL);
        if parent.is_none() || !parent_is_list {
            report(doc, node, LIST_USAGE_INVALID_LI);
        } else if doc[node].implicit {
            let code = if doc.is_o(parent, TagId::UL) {
                LIST_USAGE_INVALID_UL
            } else {
                LIST_USAGE_INVALID_OL
            };
            report(doc, node, code);
        }
    }
}

/// InitAccessibilityChecks()
fn init_accessibility_checks(doc: &mut Doc, level123: i32) {
    doc.access = AccessImpl {
        prioritychk: level123,
        ..AccessImpl::default()
    };
}

/// AccessibilityCheckNode()
fn accessibility_check_node(doc: &mut Doc, node: NodeId) {
    let tid = doc.tag_id(Some(node));
    if tid == TagId::BODY {
        check_color_contrast(doc, node);
    } else if tid == TagId::HEAD {
        if !check_meta_data(doc, node, false) {
            meta_data_present(doc, node);
        }
    } else if tid == TagId::A {
        check_anchor_access(doc, node);
    } else if tid == TagId::IMG {
        check_flicker(doc, node);
        check_color_available(doc, node);
        check_image(doc, node);
    } else if tid == TagId::MAP {
        check_map_links(doc, node);
    } else if tid == TagId::AREA {
        check_area(doc, node);
    } else if tid == TagId::APPLET {
        check_deprecated(doc, node);
        programmatic_objects(doc, node);
        dynamic_content(doc, node);
        accessible_compatible(doc, node);
        check_flicker(doc, node);
        check_color_available(doc, node);
        check_applet(doc, node);
    } else if tid == TagId::OBJECT {
        programmatic_objects(doc, node);
        dynamic_content(doc, node);
        accessible_compatible(doc, node);
        check_flicker(doc, node);
        check_color_available(doc, node);
        check_object(doc, node);
    } else if tid == TagId::FRAME {
        check_frame(doc, node);
    } else if tid == TagId::IFRAME {
        check_iframe(doc, node);
    } else if tid == TagId::SCRIPT {
        dynamic_content(doc, node);
        programmatic_objects(doc, node);
        accessible_compatible(doc, node);
        check_flicker(doc, node);
        check_color_available(doc, node);
        check_script_acc(doc, node);
    } else if tid == TagId::TABLE {
        check_color_contrast(doc, node);
        check_table(doc, node);
    } else if tid == TagId::PRE || tid == TagId::XMP {
        check_ascii(doc, node);
    } else if tid == TagId::LABEL {
        check_label(doc, node);
    } else if tid == TagId::INPUT {
        check_color_available(doc, node);
        check_input_label(doc, node);
        check_input_attributes(doc, node);
    } else if tid == TagId::FRAMESET {
        check_frame_set(doc, node);
    } else if tags::node_is_header(doc, Some(node)) {
        check_header_nesting(doc, node);
    } else if tid == TagId::P {
        check_paragraph_header(doc, node);
    } else if tid == TagId::HTML {
        check_html_access(doc, node);
    } else if tid == TagId::BLINK {
        check_blink(doc, node);
    } else if tid == TagId::MARQUEE {
        check_marquee(doc, node);
    } else if tid == TagId::LINK {
        check_link(doc, node);
    } else if tid == TagId::STYLE {
        check_color_contrast(doc, node);
        check_style(doc, node);
    } else if tid == TagId::EMBED {
        check_embed(doc, node);
        programmatic_objects(doc, node);
        accessible_compatible(doc, node);
        check_flicker(doc, node);
    } else if matches!(
        tid,
        TagId::BASEFONT
            | TagId::CENTER
            | TagId::ISINDEX
            | TagId::U
            | TagId::FONT
            | TagId::DIR
            | TagId::S
            | TagId::STRIKE
            | TagId::MENU
    ) {
        check_deprecated(doc, node);
    } else if tid == TagId::TH {
        check_th(doc, node);
    } else if matches!(tid, TagId::LI | TagId::OL | TagId::UL) {
        check_list_usage(doc, node);
    }

    let mut content = doc[node].content;
    while let Some(c) = content {
        accessibility_check_node(doc, c);
        content = doc[c].next;
    }
}

/// AccessibilityChecks()
pub fn accessibility_checks(doc: &mut Doc) {
    let level = doc.cfg(OptId::AccessibilityCheckLevel) as i32;
    init_accessibility_checks(doc, level);

    message::dialogue(doc, STRING_HELLO_ACCESS, &[]);

    let root = doc.root;
    check_script_keyboard_accessible(doc, root);
    check_for_style_attribute(doc, root);
    check_doc_type(doc);

    if level2_enabled(doc) && !check_missing_style_sheets(doc, root) {
        report(doc, root, STYLE_SHEET_CONTROL_PRESENTATION);
    }

    check_for_list_elements(doc, root);
    accessibility_check_node(doc, root);
}
