//! Generated from tidy-html5 5.8.0 include/tidyenum.h (gen_enums.py).
#![allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
/// TidyTagId
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum TagId {
    UNKNOWN,
    A,
    ABBR,
    ACRONYM,
    ADDRESS,
    ALIGN,
    APPLET,
    AREA,
    B,
    BASE,
    BASEFONT,
    BDO,
    BGSOUND,
    BIG,
    BLINK,
    BLOCKQUOTE,
    BODY,
    BR,
    BUTTON,
    CAPTION,
    CENTER,
    CITE,
    CODE,
    COL,
    COLGROUP,
    COMMENT,
    DD,
    DEL,
    DFN,
    DIR,
    DIV,
    DL,
    DT,
    EM,
    EMBED,
    FIELDSET,
    FONT,
    FORM,
    FRAME,
    FRAMESET,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    HEAD,
    HR,
    HTML,
    I,
    IFRAME,
    ILAYER,
    IMG,
    INPUT,
    INS,
    ISINDEX,
    KBD,
    KEYGEN,
    LABEL,
    LAYER,
    LEGEND,
    LI,
    LINK,
    LISTING,
    MAP,
    MATHML,
    MARQUEE,
    MENU,
    META,
    MULTICOL,
    NOBR,
    NOEMBED,
    NOFRAMES,
    NOLAYER,
    NOSAVE,
    NOSCRIPT,
    OBJECT,
    OL,
    OPTGROUP,
    OPTION,
    P,
    PARAM,
    PICTURE,
    PLAINTEXT,
    PRE,
    Q,
    RB,
    RBC,
    RP,
    RT,
    RTC,
    RUBY,
    S,
    SAMP,
    SCRIPT,
    SELECT,
    SERVER,
    SERVLET,
    SMALL,
    SPACER,
    SPAN,
    STRIKE,
    STRONG,
    STYLE,
    SUB,
    SUP,
    SVG,
    TABLE,
    TBODY,
    TD,
    TEXTAREA,
    TFOOT,
    TH,
    THEAD,
    TITLE,
    TR,
    TT,
    U,
    UL,
    VAR,
    WBR,
    XMP,
    NEXTID,
    ARTICLE,
    ASIDE,
    AUDIO,
    BDI,
    CANVAS,
    COMMAND,
    DATA,
    DATALIST,
    DETAILS,
    DIALOG,
    FIGCAPTION,
    FIGURE,
    FOOTER,
    HEADER,
    HGROUP,
    MAIN,
    MARK,
    MENUITEM,
    METER,
    NAV,
    OUTPUT,
    PROGRESS,
    SECTION,
    SOURCE,
    SUMMARY,
    TEMPLATE,
    TIME,
    TRACK,
    VIDEO,
    SLOT,
}
pub const N_TIDY_TAGS: usize = 153;
pub const ALL_TAG_IDS: [TagId; 153] = [
    TagId::UNKNOWN,
    TagId::A,
    TagId::ABBR,
    TagId::ACRONYM,
    TagId::ADDRESS,
    TagId::ALIGN,
    TagId::APPLET,
    TagId::AREA,
    TagId::B,
    TagId::BASE,
    TagId::BASEFONT,
    TagId::BDO,
    TagId::BGSOUND,
    TagId::BIG,
    TagId::BLINK,
    TagId::BLOCKQUOTE,
    TagId::BODY,
    TagId::BR,
    TagId::BUTTON,
    TagId::CAPTION,
    TagId::CENTER,
    TagId::CITE,
    TagId::CODE,
    TagId::COL,
    TagId::COLGROUP,
    TagId::COMMENT,
    TagId::DD,
    TagId::DEL,
    TagId::DFN,
    TagId::DIR,
    TagId::DIV,
    TagId::DL,
    TagId::DT,
    TagId::EM,
    TagId::EMBED,
    TagId::FIELDSET,
    TagId::FONT,
    TagId::FORM,
    TagId::FRAME,
    TagId::FRAMESET,
    TagId::H1,
    TagId::H2,
    TagId::H3,
    TagId::H4,
    TagId::H5,
    TagId::H6,
    TagId::HEAD,
    TagId::HR,
    TagId::HTML,
    TagId::I,
    TagId::IFRAME,
    TagId::ILAYER,
    TagId::IMG,
    TagId::INPUT,
    TagId::INS,
    TagId::ISINDEX,
    TagId::KBD,
    TagId::KEYGEN,
    TagId::LABEL,
    TagId::LAYER,
    TagId::LEGEND,
    TagId::LI,
    TagId::LINK,
    TagId::LISTING,
    TagId::MAP,
    TagId::MATHML,
    TagId::MARQUEE,
    TagId::MENU,
    TagId::META,
    TagId::MULTICOL,
    TagId::NOBR,
    TagId::NOEMBED,
    TagId::NOFRAMES,
    TagId::NOLAYER,
    TagId::NOSAVE,
    TagId::NOSCRIPT,
    TagId::OBJECT,
    TagId::OL,
    TagId::OPTGROUP,
    TagId::OPTION,
    TagId::P,
    TagId::PARAM,
    TagId::PICTURE,
    TagId::PLAINTEXT,
    TagId::PRE,
    TagId::Q,
    TagId::RB,
    TagId::RBC,
    TagId::RP,
    TagId::RT,
    TagId::RTC,
    TagId::RUBY,
    TagId::S,
    TagId::SAMP,
    TagId::SCRIPT,
    TagId::SELECT,
    TagId::SERVER,
    TagId::SERVLET,
    TagId::SMALL,
    TagId::SPACER,
    TagId::SPAN,
    TagId::STRIKE,
    TagId::STRONG,
    TagId::STYLE,
    TagId::SUB,
    TagId::SUP,
    TagId::SVG,
    TagId::TABLE,
    TagId::TBODY,
    TagId::TD,
    TagId::TEXTAREA,
    TagId::TFOOT,
    TagId::TH,
    TagId::THEAD,
    TagId::TITLE,
    TagId::TR,
    TagId::TT,
    TagId::U,
    TagId::UL,
    TagId::VAR,
    TagId::WBR,
    TagId::XMP,
    TagId::NEXTID,
    TagId::ARTICLE,
    TagId::ASIDE,
    TagId::AUDIO,
    TagId::BDI,
    TagId::CANVAS,
    TagId::COMMAND,
    TagId::DATA,
    TagId::DATALIST,
    TagId::DETAILS,
    TagId::DIALOG,
    TagId::FIGCAPTION,
    TagId::FIGURE,
    TagId::FOOTER,
    TagId::HEADER,
    TagId::HGROUP,
    TagId::MAIN,
    TagId::MARK,
    TagId::MENUITEM,
    TagId::METER,
    TagId::NAV,
    TagId::OUTPUT,
    TagId::PROGRESS,
    TagId::SECTION,
    TagId::SOURCE,
    TagId::SUMMARY,
    TagId::TEMPLATE,
    TagId::TIME,
    TagId::TRACK,
    TagId::VIDEO,
    TagId::SLOT,
];
/// TidyAttrId
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum AttrId {
    UNKNOWN,
    ABBR,
    ACCEPT,
    ACCEPT_CHARSET,
    ACCESSKEY,
    ACTION,
    ADD_DATE,
    ALIGN,
    ALINK,
    ALLOWFULLSCREEN,
    ALT,
    ARCHIVE,
    AXIS,
    BACKGROUND,
    BGCOLOR,
    BGPROPERTIES,
    BORDER,
    BORDERCOLOR,
    BOTTOMMARGIN,
    CELLPADDING,
    CELLSPACING,
    CHAR,
    CHAROFF,
    CHARSET,
    CHECKED,
    CITE,
    CLASS,
    CLASSID,
    CLEAR,
    CODE,
    CODEBASE,
    CODETYPE,
    COLOR,
    COLS,
    COLSPAN,
    COMPACT,
    CONTENT,
    COORDS,
    DATA,
    DATAFLD,
    DATAFORMATAS,
    DATAPAGESIZE,
    DATASRC,
    DATETIME,
    DECLARE,
    DEFER,
    DIR,
    DISABLED,
    DOWNLOAD,
    ENCODING,
    ENCTYPE,
    FACE,
    FOR,
    FRAME,
    FRAMEBORDER,
    FRAMESPACING,
    GRIDX,
    GRIDY,
    HEADERS,
    HEIGHT,
    HREF,
    HREFLANG,
    HSPACE,
    HTTP_EQUIV,
    ID,
    IS,
    ISMAP,
    ITEMID,
    ITEMPROP,
    ITEMREF,
    ITEMSCOPE,
    ITEMTYPE,
    LABEL,
    LANG,
    LANGUAGE,
    LAST_MODIFIED,
    LAST_VISIT,
    LEFTMARGIN,
    LINK,
    LONGDESC,
    LOWSRC,
    MARGINHEIGHT,
    MARGINWIDTH,
    MAXLENGTH,
    MEDIA,
    METHOD,
    MULTIPLE,
    NAME,
    NOHREF,
    NORESIZE,
    NOSHADE,
    NOWRAP,
    OBJECT,
    OnAFTERUPDATE,
    OnBEFOREUNLOAD,
    OnBEFOREUPDATE,
    OnBLUR,
    OnCHANGE,
    OnCLICK,
    OnDATAAVAILABLE,
    OnDATASETCHANGED,
    OnDATASETCOMPLETE,
    OnDBLCLICK,
    OnERRORUPDATE,
    OnFOCUS,
    OnKEYDOWN,
    OnKEYPRESS,
    OnKEYUP,
    OnLOAD,
    OnMOUSEDOWN,
    OnMOUSEMOVE,
    OnMOUSEOUT,
    OnMOUSEOVER,
    OnMOUSEUP,
    OnRESET,
    OnROWENTER,
    OnROWEXIT,
    OnSELECT,
    OnSUBMIT,
    OnUNLOAD,
    PROFILE,
    PROMPT,
    RBSPAN,
    READONLY,
    REL,
    REV,
    RIGHTMARGIN,
    ROLE,
    ROWS,
    ROWSPAN,
    RULES,
    SCHEME,
    SCOPE,
    SCROLLING,
    SELECTED,
    SHAPE,
    SHOWGRID,
    SHOWGRIDX,
    SHOWGRIDY,
    SIZE,
    SPAN,
    SRC,
    SRCSET,
    STANDBY,
    START,
    STYLE,
    SUMMARY,
    TABINDEX,
    TARGET,
    TEXT,
    TITLE,
    TOPMARGIN,
    TRANSLATE,
    TYPE,
    USEMAP,
    VALIGN,
    VALUE,
    VALUETYPE,
    VERSION,
    VLINK,
    VSPACE,
    WIDTH,
    WRAP,
    XML_LANG,
    XML_SPACE,
    XMLNS,
    EVENT,
    METHODS,
    N,
    SDAFORM,
    SDAPREF,
    SDASUFF,
    URN,
    ASYNC,
    AUTOCOMPLETE,
    AUTOFOCUS,
    AUTOPLAY,
    CHALLENGE,
    CONTENTEDITABLE,
    CONTEXTMENU,
    CONTROLS,
    CROSSORIGIN,
    DEFAULT,
    DIRNAME,
    DRAGGABLE,
    DROPZONE,
    FORM,
    FORMACTION,
    FORMENCTYPE,
    FORMMETHOD,
    FORMNOVALIDATE,
    FORMTARGET,
    HIDDEN,
    HIGH,
    ICON,
    KEYTYPE,
    KIND,
    LIST,
    LOOP,
    LOW,
    MANIFEST,
    MAX,
    MEDIAGROUP,
    MIN,
    MUTED,
    NOVALIDATE,
    OPEN,
    OPTIMUM,
    OnABORT,
    OnAFTERPRINT,
    OnBEFOREPRINT,
    OnCANPLAY,
    OnCANPLAYTHROUGH,
    OnCONTEXTMENU,
    OnCUECHANGE,
    OnDRAG,
    OnDRAGEND,
    OnDRAGENTER,
    OnDRAGLEAVE,
    OnDRAGOVER,
    OnDRAGSTART,
    OnDROP,
    OnDURATIONCHANGE,
    OnEMPTIED,
    OnENDED,
    OnERROR,
    OnHASHCHANGE,
    OnINPUT,
    OnINVALID,
    OnLOADEDDATA,
    OnLOADEDMETADATA,
    OnLOADSTART,
    OnMESSAGE,
    OnMOUSEWHEEL,
    OnOFFLINE,
    OnONLINE,
    OnPAGEHIDE,
    OnPAGESHOW,
    OnPAUSE,
    OnPLAY,
    OnPLAYING,
    OnPOPSTATE,
    OnPROGRESS,
    OnRATECHANGE,
    OnREADYSTATECHANGE,
    OnREDO,
    OnRESIZE,
    OnSCROLL,
    OnSEEKED,
    OnSEEKING,
    OnSHOW,
    OnSTALLED,
    OnSTORAGE,
    OnSUSPEND,
    OnTIMEUPDATE,
    OnUNDO,
    OnVOLUMECHANGE,
    OnWAITING,
    PATTERN,
    PLACEHOLDER,
    PLAYSINLINE,
    POSTER,
    PRELOAD,
    PUBDATE,
    RADIOGROUP,
    REQUIRED,
    REVERSED,
    SANDBOX,
    SCOPED,
    SEAMLESS,
    SIZES,
    SPELLCHECK,
    SRCDOC,
    SRCLANG,
    STEP,
    ARIA_ACTIVEDESCENDANT,
    ARIA_ATOMIC,
    ARIA_AUTOCOMPLETE,
    ARIA_BUSY,
    ARIA_CHECKED,
    ARIA_CONTROLS,
    ARIA_DESCRIBEDBY,
    ARIA_DISABLED,
    ARIA_DROPEFFECT,
    ARIA_EXPANDED,
    ARIA_FLOWTO,
    ARIA_GRABBED,
    ARIA_HASPOPUP,
    ARIA_HIDDEN,
    ARIA_INVALID,
    ARIA_LABEL,
    ARIA_LABELLEDBY,
    ARIA_LEVEL,
    ARIA_LIVE,
    ARIA_MULTILINE,
    ARIA_MULTISELECTABLE,
    ARIA_ORIENTATION,
    ARIA_OWNS,
    ARIA_POSINSET,
    ARIA_PRESSED,
    ARIA_READONLY,
    ARIA_RELEVANT,
    ARIA_REQUIRED,
    ARIA_SELECTED,
    ARIA_SETSIZE,
    ARIA_SORT,
    ARIA_VALUEMAX,
    ARIA_VALUEMIN,
    ARIA_VALUENOW,
    ARIA_VALUETEXT,
    X,
    Y,
    VIEWBOX,
    PRESERVEASPECTRATIO,
    ZOOMANDPAN,
    BASEPROFILE,
    CONTENTSCRIPTTYPE,
    CONTENTSTYLETYPE,
    DISPLAY,
    ABOUT,
    DATATYPE,
    INLIST,
    PREFIX,
    PROPERTY,
    RESOURCE,
    TYPEOF,
    VOCAB,
    INTEGRITY,
    AS,
    XMLNSXLINK,
    SLOT,
    LOADING,
    FILL,
    FILLRULE,
    STROKE,
    STROKEDASHARRAY,
    STROKEDASHOFFSET,
    STROKELINECAP,
    STROKELINEJOIN,
    STROKEMITERLIMIT,
    STROKEWIDTH,
    COLORINTERPOLATION,
    COLORRENDERING,
    OPACITY,
    STROKEOPACITY,
    FILLOPACITY,
}
pub const N_TIDY_ATTRIBS: usize = 346;
/// TidyOptionId
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum OptId {
    AccessibilityCheckLevel,
    AltText,
    AnchorAsName,
    AsciiChars,
    BlockTags,
    BodyOnly,
    BreakBeforeBR,
    CharEncoding,
    CoerceEndTags,
    CSSPrefix,
    CustomTags,
    DecorateInferredUL,
    Doctype,
    DoctypeMode,
    DropEmptyElems,
    DropEmptyParas,
    DropPropAttrs,
    DuplicateAttrs,
    Emacs,
    EmacsFile,
    EmptyTags,
    EncloseBlockText,
    EncloseBodyText,
    ErrFile,
    EscapeCdata,
    EscapeScripts,
    FixBackslash,
    FixComments,
    FixUri,
    ForceOutput,
    GDocClean,
    HideComments,
    HtmlOut,
    InCharEncoding,
    IndentAttributes,
    IndentCdata,
    IndentContent,
    IndentSpaces,
    InlineTags,
    JoinClasses,
    JoinStyles,
    KeepFileTimes,
    KeepTabs,
    LiteralAttribs,
    LogicalEmphasis,
    LowerLiterals,
    MakeBare,
    MakeClean,
    Mark,
    MergeDivs,
    MergeEmphasis,
    MergeSpans,
    MetaCharset,
    MuteReports,
    MuteShow,
    NCR,
    Newline,
    NumEntities,
    OmitOptionalTags,
    OutCharEncoding,
    OutFile,
    OutputBOM,
    PPrintTabs,
    PreserveEntities,
    PreTags,
    PriorityAttributes,
    PunctWrap,
    Quiet,
    QuoteAmpersand,
    QuoteMarks,
    QuoteNbsp,
    ReplaceColor,
    ShowErrors,
    ShowFilename,
    ShowInfo,
    ShowMarkup,
    ShowMetaChange,
    ShowWarnings,
    SkipNested,
    SortAttributes,
    StrictTagsAttr,
    StyleTags,
    TabSize,
    UpperCaseAttrs,
    UpperCaseTags,
    UseCustomTags,
    VertSpace,
    WarnPropAttrs,
    Word2000,
    WrapAsp,
    WrapAttVals,
    WrapJste,
    WrapLen,
    WrapPhp,
    WrapScriptlets,
    WrapSection,
    WriteBack,
    XhtmlOut,
    XmlDecl,
    XmlOut,
    XmlPIs,
    XmlSpace,
    XmlTags,
}
pub const N_TIDY_OPTIONS: usize = 103;
pub const ALL_OPT_IDS: [OptId; 103] = [
    OptId::AccessibilityCheckLevel,
    OptId::AltText,
    OptId::AnchorAsName,
    OptId::AsciiChars,
    OptId::BlockTags,
    OptId::BodyOnly,
    OptId::BreakBeforeBR,
    OptId::CharEncoding,
    OptId::CoerceEndTags,
    OptId::CSSPrefix,
    OptId::CustomTags,
    OptId::DecorateInferredUL,
    OptId::Doctype,
    OptId::DoctypeMode,
    OptId::DropEmptyElems,
    OptId::DropEmptyParas,
    OptId::DropPropAttrs,
    OptId::DuplicateAttrs,
    OptId::Emacs,
    OptId::EmacsFile,
    OptId::EmptyTags,
    OptId::EncloseBlockText,
    OptId::EncloseBodyText,
    OptId::ErrFile,
    OptId::EscapeCdata,
    OptId::EscapeScripts,
    OptId::FixBackslash,
    OptId::FixComments,
    OptId::FixUri,
    OptId::ForceOutput,
    OptId::GDocClean,
    OptId::HideComments,
    OptId::HtmlOut,
    OptId::InCharEncoding,
    OptId::IndentAttributes,
    OptId::IndentCdata,
    OptId::IndentContent,
    OptId::IndentSpaces,
    OptId::InlineTags,
    OptId::JoinClasses,
    OptId::JoinStyles,
    OptId::KeepFileTimes,
    OptId::KeepTabs,
    OptId::LiteralAttribs,
    OptId::LogicalEmphasis,
    OptId::LowerLiterals,
    OptId::MakeBare,
    OptId::MakeClean,
    OptId::Mark,
    OptId::MergeDivs,
    OptId::MergeEmphasis,
    OptId::MergeSpans,
    OptId::MetaCharset,
    OptId::MuteReports,
    OptId::MuteShow,
    OptId::NCR,
    OptId::Newline,
    OptId::NumEntities,
    OptId::OmitOptionalTags,
    OptId::OutCharEncoding,
    OptId::OutFile,
    OptId::OutputBOM,
    OptId::PPrintTabs,
    OptId::PreserveEntities,
    OptId::PreTags,
    OptId::PriorityAttributes,
    OptId::PunctWrap,
    OptId::Quiet,
    OptId::QuoteAmpersand,
    OptId::QuoteMarks,
    OptId::QuoteNbsp,
    OptId::ReplaceColor,
    OptId::ShowErrors,
    OptId::ShowFilename,
    OptId::ShowInfo,
    OptId::ShowMarkup,
    OptId::ShowMetaChange,
    OptId::ShowWarnings,
    OptId::SkipNested,
    OptId::SortAttributes,
    OptId::StrictTagsAttr,
    OptId::StyleTags,
    OptId::TabSize,
    OptId::UpperCaseAttrs,
    OptId::UpperCaseTags,
    OptId::UseCustomTags,
    OptId::VertSpace,
    OptId::WarnPropAttrs,
    OptId::Word2000,
    OptId::WrapAsp,
    OptId::WrapAttVals,
    OptId::WrapJste,
    OptId::WrapLen,
    OptId::WrapPhp,
    OptId::WrapScriptlets,
    OptId::WrapSection,
    OptId::WriteBack,
    OptId::XhtmlOut,
    OptId::XmlDecl,
    OptId::XmlOut,
    OptId::XmlPIs,
    OptId::XmlSpace,
    OptId::XmlTags,
];

/// tidyStrings (message codes)
pub mod code {
    pub const TIDYSTRINGS_FIRST: u32 = 500;
    pub const LINE_COLUMN_STRING: u32 = 501;
    pub const FN_LINE_COLUMN_STRING: u32 = 502;
    pub const STRING_DISCARDING: u32 = 503;
    pub const STRING_ERROR_COUNT_ERROR: u32 = 504;
    pub const STRING_ERROR_COUNT_WARNING: u32 = 505;
    pub const STRING_HELLO_ACCESS: u32 = 506;
    pub const STRING_HTML_PROPRIETARY: u32 = 507;
    pub const STRING_PLAIN_TEXT: u32 = 508;
    pub const STRING_REPLACING: u32 = 509;
    pub const STRING_SPECIFIED: u32 = 510;
    pub const STRING_XML_DECLARATION: u32 = 511;
    pub const TIDYCUSTOMNO_STRING: u32 = 512;
    pub const TIDYCUSTOMBLOCKLEVEL_STRING: u32 = 513;
    pub const TIDYCUSTOMEMPTY_STRING: u32 = 514;
    pub const TIDYCUSTOMINLINE_STRING: u32 = 515;
    pub const TIDYCUSTOMPRE_STRING: u32 = 516;
    pub const FOOTNOTE_TRIM_EMPTY_ELEMENT: u32 = 517;
    pub const TEXT_ACCESS_ADVICE1: u32 = 518;
    pub const TEXT_ACCESS_ADVICE2: u32 = 519;
    pub const TEXT_BAD_FORM: u32 = 520;
    pub const TEXT_BAD_MAIN: u32 = 521;
    pub const TEXT_HTML_T_ALGORITHM: u32 = 522;
    pub const TEXT_INVALID_URI: u32 = 523;
    pub const TEXT_INVALID_UTF16: u32 = 524;
    pub const TEXT_INVALID_UTF8: u32 = 525;
    pub const TEXT_M_IMAGE_ALT: u32 = 526;
    pub const TEXT_M_IMAGE_MAP: u32 = 527;
    pub const TEXT_M_LINK_ALT: u32 = 528;
    pub const TEXT_M_SUMMARY: u32 = 529;
    pub const TEXT_SGML_CHARS: u32 = 530;
    pub const TEXT_USING_BODY: u32 = 531;
    pub const TEXT_USING_FONT: u32 = 532;
    pub const TEXT_USING_FRAMES: u32 = 533;
    pub const TEXT_USING_LAYER: u32 = 534;
    pub const TEXT_USING_NOBR: u32 = 535;
    pub const TEXT_USING_SPACER: u32 = 536;
    pub const TEXT_VENDOR_CHARS: u32 = 537;
    pub const TEXT_WINDOWS_CHARS: u32 = 538;
    pub const STRING_ERROR_COUNT: u32 = 539;
    pub const STRING_NEEDS_INTERVENTION: u32 = 540;
    pub const STRING_NO_ERRORS: u32 = 541;
    pub const STRING_NOT_ALL_SHOWN: u32 = 542;
    pub const TEXT_GENERAL_INFO_PLEA: u32 = 543;
    pub const TEXT_GENERAL_INFO: u32 = 544;
    pub const REPORT_MESSAGE_FIRST: u32 = 545;
    pub const ADDED_MISSING_CHARSET: u32 = 546;
    pub const ANCHOR_NOT_UNIQUE: u32 = 547;
    pub const ANCHOR_DUPLICATED: u32 = 548;
    pub const APOS_UNDEFINED: u32 = 549;
    pub const ATTR_VALUE_NOT_LCASE: u32 = 550;
    pub const ATTRIBUTE_IS_NOT_ALLOWED: u32 = 551;
    pub const ATTRIBUTE_VALUE_REPLACED: u32 = 552;
    pub const BACKSLASH_IN_URI: u32 = 553;
    pub const BAD_ATTRIBUTE_VALUE_REPLACED: u32 = 554;
    pub const BAD_ATTRIBUTE_VALUE: u32 = 555;
    pub const BAD_CDATA_CONTENT: u32 = 556;
    pub const BAD_SUMMARY_HTML5: u32 = 557;
    pub const BAD_SURROGATE_LEAD: u32 = 558;
    pub const BAD_SURROGATE_PAIR: u32 = 559;
    pub const BAD_SURROGATE_TAIL: u32 = 560;
    pub const CANT_BE_NESTED: u32 = 561;
    pub const COERCE_TO_ENDTAG: u32 = 562;
    pub const CONTENT_AFTER_BODY: u32 = 563;
    pub const CUSTOM_TAG_DETECTED: u32 = 564;
    pub const DISCARDING_UNEXPECTED: u32 = 565;
    pub const DOCTYPE_AFTER_TAGS: u32 = 566;
    pub const DUPLICATE_FRAMESET: u32 = 567;
    pub const ELEMENT_NOT_EMPTY: u32 = 568;
    pub const ELEMENT_VERS_MISMATCH_ERROR: u32 = 569;
    pub const ELEMENT_VERS_MISMATCH_WARN: u32 = 570;
    pub const ENCODING_MISMATCH: u32 = 571;
    pub const ESCAPED_ILLEGAL_URI: u32 = 572;
    pub const FILE_CANT_OPEN: u32 = 573;
    pub const FILE_CANT_OPEN_CFG: u32 = 574;
    pub const FILE_NOT_FILE: u32 = 575;
    pub const FIXED_BACKSLASH: u32 = 576;
    pub const FOUND_STYLE_IN_BODY: u32 = 577;
    pub const ID_NAME_MISMATCH: u32 = 578;
    pub const ILLEGAL_NESTING: u32 = 579;
    pub const ILLEGAL_URI_CODEPOINT: u32 = 580;
    pub const ILLEGAL_URI_REFERENCE: u32 = 581;
    pub const INSERTING_AUTO_ATTRIBUTE: u32 = 582;
    pub const INSERTING_TAG: u32 = 583;
    pub const INVALID_ATTRIBUTE: u32 = 584;
    pub const INVALID_NCR: u32 = 585;
    pub const INVALID_SGML_CHARS: u32 = 586;
    pub const INVALID_UTF8: u32 = 587;
    pub const INVALID_UTF16: u32 = 588;
    pub const INVALID_XML_ID: u32 = 589;
    pub const JOINING_ATTRIBUTE: u32 = 590;
    pub const MALFORMED_COMMENT: u32 = 591;
    pub const MALFORMED_COMMENT_DROPPING: u32 = 592;
    pub const MALFORMED_COMMENT_EOS: u32 = 593;
    pub const MALFORMED_COMMENT_WARN: u32 = 594;
    pub const MALFORMED_DOCTYPE: u32 = 595;
    pub const MISMATCHED_ATTRIBUTE_ERROR: u32 = 596;
    pub const MISMATCHED_ATTRIBUTE_WARN: u32 = 597;
    pub const MISSING_ATTR_VALUE: u32 = 598;
    pub const MISSING_ATTRIBUTE: u32 = 599;
    pub const MISSING_DOCTYPE: u32 = 600;
    pub const MISSING_ENDTAG_BEFORE: u32 = 601;
    pub const MISSING_ENDTAG_FOR: u32 = 602;
    pub const MISSING_ENDTAG_OPTIONAL: u32 = 603;
    pub const MISSING_IMAGEMAP: u32 = 604;
    pub const MISSING_QUOTEMARK: u32 = 605;
    pub const MISSING_QUOTEMARK_OPEN: u32 = 606;
    pub const MISSING_SEMICOLON_NCR: u32 = 607;
    pub const MISSING_SEMICOLON: u32 = 608;
    pub const MISSING_STARTTAG: u32 = 609;
    pub const MISSING_TITLE_ELEMENT: u32 = 610;
    pub const MOVED_STYLE_TO_HEAD: u32 = 611;
    pub const NESTED_EMPHASIS: u32 = 612;
    pub const NESTED_QUOTATION: u32 = 613;
    pub const NEWLINE_IN_URI: u32 = 614;
    pub const NOFRAMES_CONTENT: u32 = 615;
    pub const NON_MATCHING_ENDTAG: u32 = 616;
    pub const OBSOLETE_ELEMENT: u32 = 617;
    pub const OPTION_REMOVED: u32 = 618;
    pub const OPTION_REMOVED_APPLIED: u32 = 619;
    pub const OPTION_REMOVED_UNAPPLIED: u32 = 620;
    pub const PREVIOUS_LOCATION: u32 = 621;
    pub const PROPRIETARY_ATTR_VALUE: u32 = 622;
    pub const PROPRIETARY_ATTRIBUTE: u32 = 623;
    pub const PROPRIETARY_ELEMENT: u32 = 624;
    pub const REMOVED_HTML5: u32 = 625;
    pub const REPEATED_ATTRIBUTE: u32 = 626;
    pub const REPLACING_ELEMENT: u32 = 627;
    pub const REPLACING_UNEX_ELEMENT: u32 = 628;
    pub const SPACE_PRECEDING_XMLDECL: u32 = 629;
    pub const STRING_CONTENT_LOOKS: u32 = 630;
    pub const STRING_ARGUMENT_BAD: u32 = 631;
    pub const STRING_DOCTYPE_GIVEN: u32 = 632;
    pub const STRING_MISSING_MALFORMED: u32 = 633;
    pub const STRING_MUTING_TYPE: u32 = 634;
    pub const STRING_NO_SYSID: u32 = 635;
    pub const STRING_UNKNOWN_OPTION: u32 = 636;
    pub const SUSPECTED_MISSING_QUOTE: u32 = 637;
    pub const TAG_NOT_ALLOWED_IN: u32 = 638;
    pub const TOO_MANY_ELEMENTS_IN: u32 = 639;
    pub const TOO_MANY_ELEMENTS: u32 = 640;
    pub const TRIM_EMPTY_ELEMENT: u32 = 641;
    pub const UNESCAPED_AMPERSAND: u32 = 642;
    pub const UNEXPECTED_END_OF_FILE_ATTR: u32 = 643;
    pub const UNEXPECTED_END_OF_FILE: u32 = 644;
    pub const UNEXPECTED_ENDTAG_ERR: u32 = 645;
    pub const UNEXPECTED_ENDTAG_IN: u32 = 646;
    pub const UNEXPECTED_ENDTAG: u32 = 647;
    pub const UNEXPECTED_EQUALSIGN: u32 = 648;
    pub const UNEXPECTED_GT: u32 = 649;
    pub const UNEXPECTED_QUOTEMARK: u32 = 650;
    pub const UNKNOWN_ELEMENT_LOOKS_CUSTOM: u32 = 651;
    pub const UNKNOWN_ELEMENT: u32 = 652;
    pub const UNKNOWN_ENTITY: u32 = 653;
    pub const USING_BR_INPLACE_OF: u32 = 654;
    pub const VENDOR_SPECIFIC_CHARS: u32 = 655;
    pub const WHITE_IN_URI: u32 = 656;
    pub const XML_DECLARATION_DETECTED: u32 = 657;
    pub const XML_ID_SYNTAX: u32 = 658;
    pub const BLANK_TITLE_ELEMENT: u32 = 659;
    pub const REPORT_MESSAGE_LAST: u32 = 660;
    pub const IMG_MISSING_ALT: u32 = 661;
    pub const IMG_ALT_SUSPICIOUS_FILENAME: u32 = 662;
    pub const IMG_ALT_SUSPICIOUS_FILE_SIZE: u32 = 663;
    pub const IMG_ALT_SUSPICIOUS_PLACEHOLDER: u32 = 664;
    pub const IMG_ALT_SUSPICIOUS_TOO_LONG: u32 = 665;
    pub const IMG_MISSING_LONGDESC_DLINK: u32 = 666;
    pub const IMG_MISSING_DLINK: u32 = 667;
    pub const IMG_MISSING_LONGDESC: u32 = 668;
    pub const IMG_BUTTON_MISSING_ALT: u32 = 669;
    pub const APPLET_MISSING_ALT: u32 = 670;
    pub const OBJECT_MISSING_ALT: u32 = 671;
    pub const AUDIO_MISSING_TEXT_WAV: u32 = 672;
    pub const AUDIO_MISSING_TEXT_AU: u32 = 673;
    pub const AUDIO_MISSING_TEXT_AIFF: u32 = 674;
    pub const AUDIO_MISSING_TEXT_SND: u32 = 675;
    pub const AUDIO_MISSING_TEXT_RA: u32 = 676;
    pub const AUDIO_MISSING_TEXT_RM: u32 = 677;
    pub const FRAME_MISSING_LONGDESC: u32 = 678;
    pub const AREA_MISSING_ALT: u32 = 679;
    pub const SCRIPT_MISSING_NOSCRIPT: u32 = 680;
    pub const ASCII_REQUIRES_DESCRIPTION: u32 = 681;
    pub const IMG_MAP_SERVER_REQUIRES_TEXT_LINKS: u32 = 682;
    pub const MULTIMEDIA_REQUIRES_TEXT: u32 = 683;
    pub const IMG_MAP_CLIENT_MISSING_TEXT_LINKS: u32 = 684;
    pub const INFORMATION_NOT_CONVEYED_IMAGE: u32 = 685;
    pub const INFORMATION_NOT_CONVEYED_APPLET: u32 = 686;
    pub const INFORMATION_NOT_CONVEYED_OBJECT: u32 = 687;
    pub const INFORMATION_NOT_CONVEYED_SCRIPT: u32 = 688;
    pub const INFORMATION_NOT_CONVEYED_INPUT: u32 = 689;
    pub const COLOR_CONTRAST_TEXT: u32 = 690;
    pub const COLOR_CONTRAST_LINK: u32 = 691;
    pub const COLOR_CONTRAST_ACTIVE_LINK: u32 = 692;
    pub const COLOR_CONTRAST_VISITED_LINK: u32 = 693;
    pub const DOCTYPE_MISSING: u32 = 694;
    pub const STYLE_SHEET_CONTROL_PRESENTATION: u32 = 695;
    pub const HEADERS_IMPROPERLY_NESTED: u32 = 696;
    pub const POTENTIAL_HEADER_BOLD: u32 = 697;
    pub const POTENTIAL_HEADER_ITALICS: u32 = 698;
    pub const POTENTIAL_HEADER_UNDERLINE: u32 = 699;
    pub const HEADER_USED_FORMAT_TEXT: u32 = 700;
    pub const LIST_USAGE_INVALID_UL: u32 = 701;
    pub const LIST_USAGE_INVALID_OL: u32 = 702;
    pub const LIST_USAGE_INVALID_LI: u32 = 703;
    pub const LANGUAGE_NOT_IDENTIFIED: u32 = 704;
    pub const LANGUAGE_INVALID: u32 = 705;
    pub const DATA_TABLE_MISSING_HEADERS: u32 = 706;
    pub const DATA_TABLE_MISSING_HEADERS_COLUMN: u32 = 707;
    pub const DATA_TABLE_MISSING_HEADERS_ROW: u32 = 708;
    pub const DATA_TABLE_REQUIRE_MARKUP_COLUMN_HEADERS: u32 = 709;
    pub const DATA_TABLE_REQUIRE_MARKUP_ROW_HEADERS: u32 = 710;
    pub const LAYOUT_TABLES_LINEARIZE_PROPERLY: u32 = 711;
    pub const LAYOUT_TABLE_INVALID_MARKUP: u32 = 712;
    pub const TABLE_MISSING_SUMMARY: u32 = 713;
    pub const TABLE_SUMMARY_INVALID_NULL: u32 = 714;
    pub const TABLE_SUMMARY_INVALID_SPACES: u32 = 715;
    pub const TABLE_SUMMARY_INVALID_PLACEHOLDER: u32 = 716;
    pub const TABLE_MISSING_CAPTION: u32 = 717;
    pub const TABLE_MAY_REQUIRE_HEADER_ABBR: u32 = 718;
    pub const TABLE_MAY_REQUIRE_HEADER_ABBR_NULL: u32 = 719;
    pub const TABLE_MAY_REQUIRE_HEADER_ABBR_SPACES: u32 = 720;
    pub const STYLESHEETS_REQUIRE_TESTING_LINK: u32 = 721;
    pub const STYLESHEETS_REQUIRE_TESTING_STYLE_ELEMENT: u32 = 722;
    pub const STYLESHEETS_REQUIRE_TESTING_STYLE_ATTR: u32 = 723;
    pub const FRAME_SRC_INVALID: u32 = 724;
    pub const TEXT_EQUIVALENTS_REQUIRE_UPDATING_APPLET: u32 = 725;
    pub const TEXT_EQUIVALENTS_REQUIRE_UPDATING_SCRIPT: u32 = 726;
    pub const TEXT_EQUIVALENTS_REQUIRE_UPDATING_OBJECT: u32 = 727;
    pub const PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_SCRIPT: u32 = 728;
    pub const PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_OBJECT: u32 = 729;
    pub const PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_EMBED: u32 = 730;
    pub const PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_APPLET: u32 = 731;
    pub const FRAME_MISSING_NOFRAMES: u32 = 732;
    pub const NOFRAMES_INVALID_NO_VALUE: u32 = 733;
    pub const NOFRAMES_INVALID_CONTENT: u32 = 734;
    pub const NOFRAMES_INVALID_LINK: u32 = 735;
    pub const REMOVE_FLICKER_SCRIPT: u32 = 736;
    pub const REMOVE_FLICKER_OBJECT: u32 = 737;
    pub const REMOVE_FLICKER_EMBED: u32 = 738;
    pub const REMOVE_FLICKER_APPLET: u32 = 739;
    pub const REMOVE_FLICKER_ANIMATED_GIF: u32 = 740;
    pub const REMOVE_BLINK_MARQUEE: u32 = 741;
    pub const REMOVE_AUTO_REFRESH: u32 = 742;
    pub const REMOVE_AUTO_REDIRECT: u32 = 743;
    pub const ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_SCRIPT: u32 = 744;
    pub const ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_OBJECT: u32 = 745;
    pub const ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_APPLET: u32 = 746;
    pub const ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_EMBED: u32 = 747;
    pub const IMAGE_MAP_SERVER_SIDE_REQUIRES_CONVERSION: u32 = 748;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_DOWN: u32 = 749;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_UP: u32 = 750;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_CLICK: u32 = 751;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OVER: u32 = 752;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OUT: u32 = 753;
    pub const SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_MOVE: u32 = 754;
    pub const NEW_WINDOWS_REQUIRE_WARNING_NEW: u32 = 755;
    pub const NEW_WINDOWS_REQUIRE_WARNING_BLANK: u32 = 756;
    pub const REPLACE_DEPRECATED_HTML_APPLET: u32 = 757;
    pub const REPLACE_DEPRECATED_HTML_BASEFONT: u32 = 758;
    pub const REPLACE_DEPRECATED_HTML_CENTER: u32 = 759;
    pub const REPLACE_DEPRECATED_HTML_DIR: u32 = 760;
    pub const REPLACE_DEPRECATED_HTML_FONT: u32 = 761;
    pub const REPLACE_DEPRECATED_HTML_ISINDEX: u32 = 762;
    pub const REPLACE_DEPRECATED_HTML_MENU: u32 = 763;
    pub const REPLACE_DEPRECATED_HTML_S: u32 = 764;
    pub const REPLACE_DEPRECATED_HTML_STRIKE: u32 = 765;
    pub const REPLACE_DEPRECATED_HTML_U: u32 = 766;
    pub const FRAME_MISSING_TITLE: u32 = 767;
    pub const FRAME_TITLE_INVALID_NULL: u32 = 768;
    pub const FRAME_TITLE_INVALID_SPACES: u32 = 769;
    pub const ASSOCIATE_LABELS_EXPLICITLY: u32 = 770;
    pub const ASSOCIATE_LABELS_EXPLICITLY_FOR: u32 = 771;
    pub const ASSOCIATE_LABELS_EXPLICITLY_ID: u32 = 772;
    pub const LINK_TEXT_NOT_MEANINGFUL: u32 = 773;
    pub const LINK_TEXT_MISSING: u32 = 774;
    pub const LINK_TEXT_TOO_LONG: u32 = 775;
    pub const LINK_TEXT_NOT_MEANINGFUL_CLICK_HERE: u32 = 776;
    pub const METADATA_MISSING: u32 = 777;
    pub const METADATA_MISSING_REDIRECT_AUTOREFRESH: u32 = 778;
    pub const SKIPOVER_ASCII_ART: u32 = 779;
    pub const TC_LABEL_COL: u32 = 780;
    pub const TC_LABEL_FILE: u32 = 781;
    pub const TC_LABEL_LANG: u32 = 782;
    pub const TC_LABEL_LEVL: u32 = 783;
    pub const TC_LABEL_OPT: u32 = 784;
    pub const TC_MAIN_ERROR_LOAD_CONFIG: u32 = 785;
    pub const TC_OPT_ACCESS: u32 = 786;
    pub const TC_OPT_ASCII: u32 = 787;
    pub const TC_OPT_ASHTML: u32 = 788;
    pub const TC_OPT_ASXML: u32 = 789;
    pub const TC_OPT_BARE: u32 = 790;
    pub const TC_OPT_BIG5: u32 = 791;
    pub const TC_OPT_CLEAN: u32 = 792;
    pub const TC_OPT_CONFIG: u32 = 793;
    pub const TC_OPT_ERRORS: u32 = 794;
    pub const TC_OPT_FILE: u32 = 795;
    pub const TC_OPT_GDOC: u32 = 796;
    pub const TC_OPT_HELP: u32 = 797;
    pub const TC_OPT_HELPCFG: u32 = 798;
    pub const TC_OPT_HELPENV: u32 = 799;
    pub const TC_OPT_HELPOPT: u32 = 800;
    pub const TC_OPT_IBM858: u32 = 801;
    pub const TC_OPT_INDENT: u32 = 802;
    pub const TC_OPT_ISO2022: u32 = 803;
    pub const TC_OPT_LANGUAGE: u32 = 804;
    pub const TC_OPT_LATIN0: u32 = 805;
    pub const TC_OPT_LATIN1: u32 = 806;
    pub const TC_OPT_MAC: u32 = 807;
    pub const TC_OPT_MODIFY: u32 = 808;
    pub const TC_OPT_NUMERIC: u32 = 809;
    pub const TC_OPT_OMIT: u32 = 810;
    pub const TC_OPT_OUTPUT: u32 = 811;
    pub const TC_OPT_QUIET: u32 = 812;
    pub const TC_OPT_RAW: u32 = 813;
    pub const TC_OPT_SHIFTJIS: u32 = 814;
    pub const TC_OPT_SHOWCFG: u32 = 815;
    pub const TC_OPT_EXP_CFG: u32 = 816;
    pub const TC_OPT_EXP_DEF: u32 = 817;
    pub const TC_OPT_UPPER: u32 = 818;
    pub const TC_OPT_UTF16: u32 = 819;
    pub const TC_OPT_UTF16BE: u32 = 820;
    pub const TC_OPT_UTF16LE: u32 = 821;
    pub const TC_OPT_UTF8: u32 = 822;
    pub const TC_OPT_VERSION: u32 = 823;
    pub const TC_OPT_WIN1252: u32 = 824;
    pub const TC_OPT_WRAP: u32 = 825;
    pub const TC_OPT_XML: u32 = 826;
    pub const TC_OPT_XMLCFG: u32 = 827;
    pub const TC_OPT_XMLSTRG: u32 = 828;
    pub const TC_OPT_XMLERRS: u32 = 829;
    pub const TC_OPT_XMLOPTS: u32 = 830;
    pub const TC_OPT_XMLHELP: u32 = 831;
    pub const TC_STRING_CONF_HEADER: u32 = 832;
    pub const TC_STRING_CONF_NAME: u32 = 833;
    pub const TC_STRING_CONF_TYPE: u32 = 834;
    pub const TC_STRING_CONF_VALUE: u32 = 835;
    pub const TC_STRING_CONF_NOTE: u32 = 836;
    pub const TC_STRING_OPT_NOT_DOCUMENTED: u32 = 837;
    pub const TC_STRING_OUT_OF_MEMORY: u32 = 838;
    pub const TC_STRING_FATAL_ERROR: u32 = 839;
    pub const TC_STRING_FILE_MANIP: u32 = 840;
    pub const TC_STRING_LANG_MUST_SPECIFY: u32 = 841;
    pub const TC_STRING_LANG_NOT_FOUND: u32 = 842;
    pub const TC_STRING_MUST_SPECIFY: u32 = 843;
    pub const TC_STRING_PROCESS_DIRECTIVES: u32 = 844;
    pub const TC_STRING_CHAR_ENCODING: u32 = 845;
    pub const TC_STRING_MISC: u32 = 846;
    pub const TC_STRING_XML: u32 = 847;
    pub const TC_STRING_UNKNOWN_OPTION: u32 = 848;
    pub const TC_STRING_UNKNOWN_OPTION_B: u32 = 849;
    pub const TC_STRING_VERS_A: u32 = 850;
    pub const TC_STRING_VERS_B: u32 = 851;
    pub const TC_TXT_HELP_1: u32 = 852;
    pub const TC_TXT_HELP_2A: u32 = 853;
    pub const TC_TXT_HELP_2B: u32 = 854;
    pub const TC_TXT_HELP_3: u32 = 855;
    pub const TC_TXT_HELP_3A: u32 = 856;
    pub const TC_TXT_HELP_CONFIG: u32 = 857;
    pub const TC_TXT_HELP_CONFIG_NAME: u32 = 858;
    pub const TC_TXT_HELP_CONFIG_TYPE: u32 = 859;
    pub const TC_TXT_HELP_CONFIG_ALLW: u32 = 860;
    pub const TC_TXT_HELP_ENV_1: u32 = 861;
    pub const TC_TXT_HELP_ENV_1A: u32 = 862;
    pub const TC_TXT_HELP_ENV_1B: u32 = 863;
    pub const TC_TXT_HELP_ENV_1C: u32 = 864;
    pub const TC_TXT_HELP_LANG_1: u32 = 865;
    pub const TC_TXT_HELP_LANG_2: u32 = 866;
    pub const TC_TXT_HELP_LANG_3: u32 = 867;
    pub const TIDYSTRINGS_LAST: u32 = 868;
}
/// (key, code) for tidyErrorCodeAsKey / tidyErrorCodeFromKey
pub const CODE_KEYS: &[(&str, u32)] = &[
    ("LINE_COLUMN_STRING", code::LINE_COLUMN_STRING),
    ("FN_LINE_COLUMN_STRING", code::FN_LINE_COLUMN_STRING),
    ("STRING_DISCARDING", code::STRING_DISCARDING),
    ("STRING_ERROR_COUNT_ERROR", code::STRING_ERROR_COUNT_ERROR),
    (
        "STRING_ERROR_COUNT_WARNING",
        code::STRING_ERROR_COUNT_WARNING,
    ),
    ("STRING_HELLO_ACCESS", code::STRING_HELLO_ACCESS),
    ("STRING_HTML_PROPRIETARY", code::STRING_HTML_PROPRIETARY),
    ("STRING_PLAIN_TEXT", code::STRING_PLAIN_TEXT),
    ("STRING_REPLACING", code::STRING_REPLACING),
    ("STRING_SPECIFIED", code::STRING_SPECIFIED),
    ("STRING_XML_DECLARATION", code::STRING_XML_DECLARATION),
    ("TIDYCUSTOMNO_STRING", code::TIDYCUSTOMNO_STRING),
    (
        "TIDYCUSTOMBLOCKLEVEL_STRING",
        code::TIDYCUSTOMBLOCKLEVEL_STRING,
    ),
    ("TIDYCUSTOMEMPTY_STRING", code::TIDYCUSTOMEMPTY_STRING),
    ("TIDYCUSTOMINLINE_STRING", code::TIDYCUSTOMINLINE_STRING),
    ("TIDYCUSTOMPRE_STRING", code::TIDYCUSTOMPRE_STRING),
    (
        "FOOTNOTE_TRIM_EMPTY_ELEMENT",
        code::FOOTNOTE_TRIM_EMPTY_ELEMENT,
    ),
    ("TEXT_ACCESS_ADVICE1", code::TEXT_ACCESS_ADVICE1),
    ("TEXT_ACCESS_ADVICE2", code::TEXT_ACCESS_ADVICE2),
    ("TEXT_BAD_FORM", code::TEXT_BAD_FORM),
    ("TEXT_BAD_MAIN", code::TEXT_BAD_MAIN),
    ("TEXT_HTML_T_ALGORITHM", code::TEXT_HTML_T_ALGORITHM),
    ("TEXT_INVALID_URI", code::TEXT_INVALID_URI),
    ("TEXT_INVALID_UTF16", code::TEXT_INVALID_UTF16),
    ("TEXT_INVALID_UTF8", code::TEXT_INVALID_UTF8),
    ("TEXT_M_IMAGE_ALT", code::TEXT_M_IMAGE_ALT),
    ("TEXT_M_IMAGE_MAP", code::TEXT_M_IMAGE_MAP),
    ("TEXT_M_LINK_ALT", code::TEXT_M_LINK_ALT),
    ("TEXT_M_SUMMARY", code::TEXT_M_SUMMARY),
    ("TEXT_SGML_CHARS", code::TEXT_SGML_CHARS),
    ("TEXT_USING_BODY", code::TEXT_USING_BODY),
    ("TEXT_USING_FONT", code::TEXT_USING_FONT),
    ("TEXT_USING_FRAMES", code::TEXT_USING_FRAMES),
    ("TEXT_USING_LAYER", code::TEXT_USING_LAYER),
    ("TEXT_USING_NOBR", code::TEXT_USING_NOBR),
    ("TEXT_USING_SPACER", code::TEXT_USING_SPACER),
    ("TEXT_VENDOR_CHARS", code::TEXT_VENDOR_CHARS),
    ("TEXT_WINDOWS_CHARS", code::TEXT_WINDOWS_CHARS),
    ("STRING_ERROR_COUNT", code::STRING_ERROR_COUNT),
    ("STRING_NEEDS_INTERVENTION", code::STRING_NEEDS_INTERVENTION),
    ("STRING_NO_ERRORS", code::STRING_NO_ERRORS),
    ("STRING_NOT_ALL_SHOWN", code::STRING_NOT_ALL_SHOWN),
    ("TEXT_GENERAL_INFO_PLEA", code::TEXT_GENERAL_INFO_PLEA),
    ("TEXT_GENERAL_INFO", code::TEXT_GENERAL_INFO),
    ("REPORT_MESSAGE_FIRST", code::REPORT_MESSAGE_FIRST),
    ("ADDED_MISSING_CHARSET", code::ADDED_MISSING_CHARSET),
    ("ANCHOR_NOT_UNIQUE", code::ANCHOR_NOT_UNIQUE),
    ("ANCHOR_DUPLICATED", code::ANCHOR_DUPLICATED),
    ("APOS_UNDEFINED", code::APOS_UNDEFINED),
    ("ATTR_VALUE_NOT_LCASE", code::ATTR_VALUE_NOT_LCASE),
    ("ATTRIBUTE_IS_NOT_ALLOWED", code::ATTRIBUTE_IS_NOT_ALLOWED),
    ("ATTRIBUTE_VALUE_REPLACED", code::ATTRIBUTE_VALUE_REPLACED),
    ("BACKSLASH_IN_URI", code::BACKSLASH_IN_URI),
    (
        "BAD_ATTRIBUTE_VALUE_REPLACED",
        code::BAD_ATTRIBUTE_VALUE_REPLACED,
    ),
    ("BAD_ATTRIBUTE_VALUE", code::BAD_ATTRIBUTE_VALUE),
    ("BAD_CDATA_CONTENT", code::BAD_CDATA_CONTENT),
    ("BAD_SUMMARY_HTML5", code::BAD_SUMMARY_HTML5),
    ("BAD_SURROGATE_LEAD", code::BAD_SURROGATE_LEAD),
    ("BAD_SURROGATE_PAIR", code::BAD_SURROGATE_PAIR),
    ("BAD_SURROGATE_TAIL", code::BAD_SURROGATE_TAIL),
    ("CANT_BE_NESTED", code::CANT_BE_NESTED),
    ("COERCE_TO_ENDTAG", code::COERCE_TO_ENDTAG),
    ("CONTENT_AFTER_BODY", code::CONTENT_AFTER_BODY),
    ("CUSTOM_TAG_DETECTED", code::CUSTOM_TAG_DETECTED),
    ("DISCARDING_UNEXPECTED", code::DISCARDING_UNEXPECTED),
    ("DOCTYPE_AFTER_TAGS", code::DOCTYPE_AFTER_TAGS),
    ("DUPLICATE_FRAMESET", code::DUPLICATE_FRAMESET),
    ("ELEMENT_NOT_EMPTY", code::ELEMENT_NOT_EMPTY),
    (
        "ELEMENT_VERS_MISMATCH_ERROR",
        code::ELEMENT_VERS_MISMATCH_ERROR,
    ),
    (
        "ELEMENT_VERS_MISMATCH_WARN",
        code::ELEMENT_VERS_MISMATCH_WARN,
    ),
    ("ENCODING_MISMATCH", code::ENCODING_MISMATCH),
    ("ESCAPED_ILLEGAL_URI", code::ESCAPED_ILLEGAL_URI),
    ("FILE_CANT_OPEN", code::FILE_CANT_OPEN),
    ("FILE_CANT_OPEN_CFG", code::FILE_CANT_OPEN_CFG),
    ("FILE_NOT_FILE", code::FILE_NOT_FILE),
    ("FIXED_BACKSLASH", code::FIXED_BACKSLASH),
    ("FOUND_STYLE_IN_BODY", code::FOUND_STYLE_IN_BODY),
    ("ID_NAME_MISMATCH", code::ID_NAME_MISMATCH),
    ("ILLEGAL_NESTING", code::ILLEGAL_NESTING),
    ("ILLEGAL_URI_CODEPOINT", code::ILLEGAL_URI_CODEPOINT),
    ("ILLEGAL_URI_REFERENCE", code::ILLEGAL_URI_REFERENCE),
    ("INSERTING_AUTO_ATTRIBUTE", code::INSERTING_AUTO_ATTRIBUTE),
    ("INSERTING_TAG", code::INSERTING_TAG),
    ("INVALID_ATTRIBUTE", code::INVALID_ATTRIBUTE),
    ("INVALID_NCR", code::INVALID_NCR),
    ("INVALID_SGML_CHARS", code::INVALID_SGML_CHARS),
    ("INVALID_UTF8", code::INVALID_UTF8),
    ("INVALID_UTF16", code::INVALID_UTF16),
    ("INVALID_XML_ID", code::INVALID_XML_ID),
    ("JOINING_ATTRIBUTE", code::JOINING_ATTRIBUTE),
    ("MALFORMED_COMMENT", code::MALFORMED_COMMENT),
    (
        "MALFORMED_COMMENT_DROPPING",
        code::MALFORMED_COMMENT_DROPPING,
    ),
    ("MALFORMED_COMMENT_EOS", code::MALFORMED_COMMENT_EOS),
    ("MALFORMED_COMMENT_WARN", code::MALFORMED_COMMENT_WARN),
    ("MALFORMED_DOCTYPE", code::MALFORMED_DOCTYPE),
    (
        "MISMATCHED_ATTRIBUTE_ERROR",
        code::MISMATCHED_ATTRIBUTE_ERROR,
    ),
    ("MISMATCHED_ATTRIBUTE_WARN", code::MISMATCHED_ATTRIBUTE_WARN),
    ("MISSING_ATTR_VALUE", code::MISSING_ATTR_VALUE),
    ("MISSING_ATTRIBUTE", code::MISSING_ATTRIBUTE),
    ("MISSING_DOCTYPE", code::MISSING_DOCTYPE),
    ("MISSING_ENDTAG_BEFORE", code::MISSING_ENDTAG_BEFORE),
    ("MISSING_ENDTAG_FOR", code::MISSING_ENDTAG_FOR),
    ("MISSING_ENDTAG_OPTIONAL", code::MISSING_ENDTAG_OPTIONAL),
    ("MISSING_IMAGEMAP", code::MISSING_IMAGEMAP),
    ("MISSING_QUOTEMARK", code::MISSING_QUOTEMARK),
    ("MISSING_QUOTEMARK_OPEN", code::MISSING_QUOTEMARK_OPEN),
    ("MISSING_SEMICOLON_NCR", code::MISSING_SEMICOLON_NCR),
    ("MISSING_SEMICOLON", code::MISSING_SEMICOLON),
    ("MISSING_STARTTAG", code::MISSING_STARTTAG),
    ("MISSING_TITLE_ELEMENT", code::MISSING_TITLE_ELEMENT),
    ("MOVED_STYLE_TO_HEAD", code::MOVED_STYLE_TO_HEAD),
    ("NESTED_EMPHASIS", code::NESTED_EMPHASIS),
    ("NESTED_QUOTATION", code::NESTED_QUOTATION),
    ("NEWLINE_IN_URI", code::NEWLINE_IN_URI),
    ("NOFRAMES_CONTENT", code::NOFRAMES_CONTENT),
    ("NON_MATCHING_ENDTAG", code::NON_MATCHING_ENDTAG),
    ("OBSOLETE_ELEMENT", code::OBSOLETE_ELEMENT),
    ("OPTION_REMOVED", code::OPTION_REMOVED),
    ("OPTION_REMOVED_APPLIED", code::OPTION_REMOVED_APPLIED),
    ("OPTION_REMOVED_UNAPPLIED", code::OPTION_REMOVED_UNAPPLIED),
    ("PREVIOUS_LOCATION", code::PREVIOUS_LOCATION),
    ("PROPRIETARY_ATTR_VALUE", code::PROPRIETARY_ATTR_VALUE),
    ("PROPRIETARY_ATTRIBUTE", code::PROPRIETARY_ATTRIBUTE),
    ("PROPRIETARY_ELEMENT", code::PROPRIETARY_ELEMENT),
    ("REMOVED_HTML5", code::REMOVED_HTML5),
    ("REPEATED_ATTRIBUTE", code::REPEATED_ATTRIBUTE),
    ("REPLACING_ELEMENT", code::REPLACING_ELEMENT),
    ("REPLACING_UNEX_ELEMENT", code::REPLACING_UNEX_ELEMENT),
    ("SPACE_PRECEDING_XMLDECL", code::SPACE_PRECEDING_XMLDECL),
    ("STRING_CONTENT_LOOKS", code::STRING_CONTENT_LOOKS),
    ("STRING_ARGUMENT_BAD", code::STRING_ARGUMENT_BAD),
    ("STRING_DOCTYPE_GIVEN", code::STRING_DOCTYPE_GIVEN),
    ("STRING_MISSING_MALFORMED", code::STRING_MISSING_MALFORMED),
    ("STRING_MUTING_TYPE", code::STRING_MUTING_TYPE),
    ("STRING_NO_SYSID", code::STRING_NO_SYSID),
    ("STRING_UNKNOWN_OPTION", code::STRING_UNKNOWN_OPTION),
    ("SUSPECTED_MISSING_QUOTE", code::SUSPECTED_MISSING_QUOTE),
    ("TAG_NOT_ALLOWED_IN", code::TAG_NOT_ALLOWED_IN),
    ("TOO_MANY_ELEMENTS_IN", code::TOO_MANY_ELEMENTS_IN),
    ("TOO_MANY_ELEMENTS", code::TOO_MANY_ELEMENTS),
    ("TRIM_EMPTY_ELEMENT", code::TRIM_EMPTY_ELEMENT),
    ("UNESCAPED_AMPERSAND", code::UNESCAPED_AMPERSAND),
    (
        "UNEXPECTED_END_OF_FILE_ATTR",
        code::UNEXPECTED_END_OF_FILE_ATTR,
    ),
    ("UNEXPECTED_END_OF_FILE", code::UNEXPECTED_END_OF_FILE),
    ("UNEXPECTED_ENDTAG_ERR", code::UNEXPECTED_ENDTAG_ERR),
    ("UNEXPECTED_ENDTAG_IN", code::UNEXPECTED_ENDTAG_IN),
    ("UNEXPECTED_ENDTAG", code::UNEXPECTED_ENDTAG),
    ("UNEXPECTED_EQUALSIGN", code::UNEXPECTED_EQUALSIGN),
    ("UNEXPECTED_GT", code::UNEXPECTED_GT),
    ("UNEXPECTED_QUOTEMARK", code::UNEXPECTED_QUOTEMARK),
    (
        "UNKNOWN_ELEMENT_LOOKS_CUSTOM",
        code::UNKNOWN_ELEMENT_LOOKS_CUSTOM,
    ),
    ("UNKNOWN_ELEMENT", code::UNKNOWN_ELEMENT),
    ("UNKNOWN_ENTITY", code::UNKNOWN_ENTITY),
    ("USING_BR_INPLACE_OF", code::USING_BR_INPLACE_OF),
    ("VENDOR_SPECIFIC_CHARS", code::VENDOR_SPECIFIC_CHARS),
    ("WHITE_IN_URI", code::WHITE_IN_URI),
    ("XML_DECLARATION_DETECTED", code::XML_DECLARATION_DETECTED),
    ("XML_ID_SYNTAX", code::XML_ID_SYNTAX),
    ("BLANK_TITLE_ELEMENT", code::BLANK_TITLE_ELEMENT),
    ("REPORT_MESSAGE_LAST", code::REPORT_MESSAGE_LAST),
    ("IMG_MISSING_ALT", code::IMG_MISSING_ALT),
    (
        "IMG_ALT_SUSPICIOUS_FILENAME",
        code::IMG_ALT_SUSPICIOUS_FILENAME,
    ),
    (
        "IMG_ALT_SUSPICIOUS_FILE_SIZE",
        code::IMG_ALT_SUSPICIOUS_FILE_SIZE,
    ),
    (
        "IMG_ALT_SUSPICIOUS_PLACEHOLDER",
        code::IMG_ALT_SUSPICIOUS_PLACEHOLDER,
    ),
    (
        "IMG_ALT_SUSPICIOUS_TOO_LONG",
        code::IMG_ALT_SUSPICIOUS_TOO_LONG,
    ),
    (
        "IMG_MISSING_LONGDESC_DLINK",
        code::IMG_MISSING_LONGDESC_DLINK,
    ),
    ("IMG_MISSING_DLINK", code::IMG_MISSING_DLINK),
    ("IMG_MISSING_LONGDESC", code::IMG_MISSING_LONGDESC),
    ("IMG_BUTTON_MISSING_ALT", code::IMG_BUTTON_MISSING_ALT),
    ("APPLET_MISSING_ALT", code::APPLET_MISSING_ALT),
    ("OBJECT_MISSING_ALT", code::OBJECT_MISSING_ALT),
    ("AUDIO_MISSING_TEXT_WAV", code::AUDIO_MISSING_TEXT_WAV),
    ("AUDIO_MISSING_TEXT_AU", code::AUDIO_MISSING_TEXT_AU),
    ("AUDIO_MISSING_TEXT_AIFF", code::AUDIO_MISSING_TEXT_AIFF),
    ("AUDIO_MISSING_TEXT_SND", code::AUDIO_MISSING_TEXT_SND),
    ("AUDIO_MISSING_TEXT_RA", code::AUDIO_MISSING_TEXT_RA),
    ("AUDIO_MISSING_TEXT_RM", code::AUDIO_MISSING_TEXT_RM),
    ("FRAME_MISSING_LONGDESC", code::FRAME_MISSING_LONGDESC),
    ("AREA_MISSING_ALT", code::AREA_MISSING_ALT),
    ("SCRIPT_MISSING_NOSCRIPT", code::SCRIPT_MISSING_NOSCRIPT),
    (
        "ASCII_REQUIRES_DESCRIPTION",
        code::ASCII_REQUIRES_DESCRIPTION,
    ),
    (
        "IMG_MAP_SERVER_REQUIRES_TEXT_LINKS",
        code::IMG_MAP_SERVER_REQUIRES_TEXT_LINKS,
    ),
    ("MULTIMEDIA_REQUIRES_TEXT", code::MULTIMEDIA_REQUIRES_TEXT),
    (
        "IMG_MAP_CLIENT_MISSING_TEXT_LINKS",
        code::IMG_MAP_CLIENT_MISSING_TEXT_LINKS,
    ),
    (
        "INFORMATION_NOT_CONVEYED_IMAGE",
        code::INFORMATION_NOT_CONVEYED_IMAGE,
    ),
    (
        "INFORMATION_NOT_CONVEYED_APPLET",
        code::INFORMATION_NOT_CONVEYED_APPLET,
    ),
    (
        "INFORMATION_NOT_CONVEYED_OBJECT",
        code::INFORMATION_NOT_CONVEYED_OBJECT,
    ),
    (
        "INFORMATION_NOT_CONVEYED_SCRIPT",
        code::INFORMATION_NOT_CONVEYED_SCRIPT,
    ),
    (
        "INFORMATION_NOT_CONVEYED_INPUT",
        code::INFORMATION_NOT_CONVEYED_INPUT,
    ),
    ("COLOR_CONTRAST_TEXT", code::COLOR_CONTRAST_TEXT),
    ("COLOR_CONTRAST_LINK", code::COLOR_CONTRAST_LINK),
    (
        "COLOR_CONTRAST_ACTIVE_LINK",
        code::COLOR_CONTRAST_ACTIVE_LINK,
    ),
    (
        "COLOR_CONTRAST_VISITED_LINK",
        code::COLOR_CONTRAST_VISITED_LINK,
    ),
    ("DOCTYPE_MISSING", code::DOCTYPE_MISSING),
    (
        "STYLE_SHEET_CONTROL_PRESENTATION",
        code::STYLE_SHEET_CONTROL_PRESENTATION,
    ),
    ("HEADERS_IMPROPERLY_NESTED", code::HEADERS_IMPROPERLY_NESTED),
    ("POTENTIAL_HEADER_BOLD", code::POTENTIAL_HEADER_BOLD),
    ("POTENTIAL_HEADER_ITALICS", code::POTENTIAL_HEADER_ITALICS),
    (
        "POTENTIAL_HEADER_UNDERLINE",
        code::POTENTIAL_HEADER_UNDERLINE,
    ),
    ("HEADER_USED_FORMAT_TEXT", code::HEADER_USED_FORMAT_TEXT),
    ("LIST_USAGE_INVALID_UL", code::LIST_USAGE_INVALID_UL),
    ("LIST_USAGE_INVALID_OL", code::LIST_USAGE_INVALID_OL),
    ("LIST_USAGE_INVALID_LI", code::LIST_USAGE_INVALID_LI),
    ("LANGUAGE_NOT_IDENTIFIED", code::LANGUAGE_NOT_IDENTIFIED),
    ("LANGUAGE_INVALID", code::LANGUAGE_INVALID),
    (
        "DATA_TABLE_MISSING_HEADERS",
        code::DATA_TABLE_MISSING_HEADERS,
    ),
    (
        "DATA_TABLE_MISSING_HEADERS_COLUMN",
        code::DATA_TABLE_MISSING_HEADERS_COLUMN,
    ),
    (
        "DATA_TABLE_MISSING_HEADERS_ROW",
        code::DATA_TABLE_MISSING_HEADERS_ROW,
    ),
    (
        "DATA_TABLE_REQUIRE_MARKUP_COLUMN_HEADERS",
        code::DATA_TABLE_REQUIRE_MARKUP_COLUMN_HEADERS,
    ),
    (
        "DATA_TABLE_REQUIRE_MARKUP_ROW_HEADERS",
        code::DATA_TABLE_REQUIRE_MARKUP_ROW_HEADERS,
    ),
    (
        "LAYOUT_TABLES_LINEARIZE_PROPERLY",
        code::LAYOUT_TABLES_LINEARIZE_PROPERLY,
    ),
    (
        "LAYOUT_TABLE_INVALID_MARKUP",
        code::LAYOUT_TABLE_INVALID_MARKUP,
    ),
    ("TABLE_MISSING_SUMMARY", code::TABLE_MISSING_SUMMARY),
    (
        "TABLE_SUMMARY_INVALID_NULL",
        code::TABLE_SUMMARY_INVALID_NULL,
    ),
    (
        "TABLE_SUMMARY_INVALID_SPACES",
        code::TABLE_SUMMARY_INVALID_SPACES,
    ),
    (
        "TABLE_SUMMARY_INVALID_PLACEHOLDER",
        code::TABLE_SUMMARY_INVALID_PLACEHOLDER,
    ),
    ("TABLE_MISSING_CAPTION", code::TABLE_MISSING_CAPTION),
    (
        "TABLE_MAY_REQUIRE_HEADER_ABBR",
        code::TABLE_MAY_REQUIRE_HEADER_ABBR,
    ),
    (
        "TABLE_MAY_REQUIRE_HEADER_ABBR_NULL",
        code::TABLE_MAY_REQUIRE_HEADER_ABBR_NULL,
    ),
    (
        "TABLE_MAY_REQUIRE_HEADER_ABBR_SPACES",
        code::TABLE_MAY_REQUIRE_HEADER_ABBR_SPACES,
    ),
    (
        "STYLESHEETS_REQUIRE_TESTING_LINK",
        code::STYLESHEETS_REQUIRE_TESTING_LINK,
    ),
    (
        "STYLESHEETS_REQUIRE_TESTING_STYLE_ELEMENT",
        code::STYLESHEETS_REQUIRE_TESTING_STYLE_ELEMENT,
    ),
    (
        "STYLESHEETS_REQUIRE_TESTING_STYLE_ATTR",
        code::STYLESHEETS_REQUIRE_TESTING_STYLE_ATTR,
    ),
    ("FRAME_SRC_INVALID", code::FRAME_SRC_INVALID),
    (
        "TEXT_EQUIVALENTS_REQUIRE_UPDATING_APPLET",
        code::TEXT_EQUIVALENTS_REQUIRE_UPDATING_APPLET,
    ),
    (
        "TEXT_EQUIVALENTS_REQUIRE_UPDATING_SCRIPT",
        code::TEXT_EQUIVALENTS_REQUIRE_UPDATING_SCRIPT,
    ),
    (
        "TEXT_EQUIVALENTS_REQUIRE_UPDATING_OBJECT",
        code::TEXT_EQUIVALENTS_REQUIRE_UPDATING_OBJECT,
    ),
    (
        "PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_SCRIPT",
        code::PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_SCRIPT,
    ),
    (
        "PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_OBJECT",
        code::PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_OBJECT,
    ),
    (
        "PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_EMBED",
        code::PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_EMBED,
    ),
    (
        "PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_APPLET",
        code::PROGRAMMATIC_OBJECTS_REQUIRE_TESTING_APPLET,
    ),
    ("FRAME_MISSING_NOFRAMES", code::FRAME_MISSING_NOFRAMES),
    ("NOFRAMES_INVALID_NO_VALUE", code::NOFRAMES_INVALID_NO_VALUE),
    ("NOFRAMES_INVALID_CONTENT", code::NOFRAMES_INVALID_CONTENT),
    ("NOFRAMES_INVALID_LINK", code::NOFRAMES_INVALID_LINK),
    ("REMOVE_FLICKER_SCRIPT", code::REMOVE_FLICKER_SCRIPT),
    ("REMOVE_FLICKER_OBJECT", code::REMOVE_FLICKER_OBJECT),
    ("REMOVE_FLICKER_EMBED", code::REMOVE_FLICKER_EMBED),
    ("REMOVE_FLICKER_APPLET", code::REMOVE_FLICKER_APPLET),
    (
        "REMOVE_FLICKER_ANIMATED_GIF",
        code::REMOVE_FLICKER_ANIMATED_GIF,
    ),
    ("REMOVE_BLINK_MARQUEE", code::REMOVE_BLINK_MARQUEE),
    ("REMOVE_AUTO_REFRESH", code::REMOVE_AUTO_REFRESH),
    ("REMOVE_AUTO_REDIRECT", code::REMOVE_AUTO_REDIRECT),
    (
        "ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_SCRIPT",
        code::ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_SCRIPT,
    ),
    (
        "ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_OBJECT",
        code::ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_OBJECT,
    ),
    (
        "ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_APPLET",
        code::ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_APPLET,
    ),
    (
        "ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_EMBED",
        code::ENSURE_PROGRAMMATIC_OBJECTS_ACCESSIBLE_EMBED,
    ),
    (
        "IMAGE_MAP_SERVER_SIDE_REQUIRES_CONVERSION",
        code::IMAGE_MAP_SERVER_SIDE_REQUIRES_CONVERSION,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_DOWN",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_DOWN,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_UP",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_UP,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_CLICK",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_CLICK,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OVER",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OVER,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OUT",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_OUT,
    ),
    (
        "SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_MOVE",
        code::SCRIPT_NOT_KEYBOARD_ACCESSIBLE_ON_MOUSE_MOVE,
    ),
    (
        "NEW_WINDOWS_REQUIRE_WARNING_NEW",
        code::NEW_WINDOWS_REQUIRE_WARNING_NEW,
    ),
    (
        "NEW_WINDOWS_REQUIRE_WARNING_BLANK",
        code::NEW_WINDOWS_REQUIRE_WARNING_BLANK,
    ),
    (
        "REPLACE_DEPRECATED_HTML_APPLET",
        code::REPLACE_DEPRECATED_HTML_APPLET,
    ),
    (
        "REPLACE_DEPRECATED_HTML_BASEFONT",
        code::REPLACE_DEPRECATED_HTML_BASEFONT,
    ),
    (
        "REPLACE_DEPRECATED_HTML_CENTER",
        code::REPLACE_DEPRECATED_HTML_CENTER,
    ),
    (
        "REPLACE_DEPRECATED_HTML_DIR",
        code::REPLACE_DEPRECATED_HTML_DIR,
    ),
    (
        "REPLACE_DEPRECATED_HTML_FONT",
        code::REPLACE_DEPRECATED_HTML_FONT,
    ),
    (
        "REPLACE_DEPRECATED_HTML_ISINDEX",
        code::REPLACE_DEPRECATED_HTML_ISINDEX,
    ),
    (
        "REPLACE_DEPRECATED_HTML_MENU",
        code::REPLACE_DEPRECATED_HTML_MENU,
    ),
    ("REPLACE_DEPRECATED_HTML_S", code::REPLACE_DEPRECATED_HTML_S),
    (
        "REPLACE_DEPRECATED_HTML_STRIKE",
        code::REPLACE_DEPRECATED_HTML_STRIKE,
    ),
    ("REPLACE_DEPRECATED_HTML_U", code::REPLACE_DEPRECATED_HTML_U),
    ("FRAME_MISSING_TITLE", code::FRAME_MISSING_TITLE),
    ("FRAME_TITLE_INVALID_NULL", code::FRAME_TITLE_INVALID_NULL),
    (
        "FRAME_TITLE_INVALID_SPACES",
        code::FRAME_TITLE_INVALID_SPACES,
    ),
    (
        "ASSOCIATE_LABELS_EXPLICITLY",
        code::ASSOCIATE_LABELS_EXPLICITLY,
    ),
    (
        "ASSOCIATE_LABELS_EXPLICITLY_FOR",
        code::ASSOCIATE_LABELS_EXPLICITLY_FOR,
    ),
    (
        "ASSOCIATE_LABELS_EXPLICITLY_ID",
        code::ASSOCIATE_LABELS_EXPLICITLY_ID,
    ),
    ("LINK_TEXT_NOT_MEANINGFUL", code::LINK_TEXT_NOT_MEANINGFUL),
    ("LINK_TEXT_MISSING", code::LINK_TEXT_MISSING),
    ("LINK_TEXT_TOO_LONG", code::LINK_TEXT_TOO_LONG),
    (
        "LINK_TEXT_NOT_MEANINGFUL_CLICK_HERE",
        code::LINK_TEXT_NOT_MEANINGFUL_CLICK_HERE,
    ),
    ("METADATA_MISSING", code::METADATA_MISSING),
    (
        "METADATA_MISSING_REDIRECT_AUTOREFRESH",
        code::METADATA_MISSING_REDIRECT_AUTOREFRESH,
    ),
    ("SKIPOVER_ASCII_ART", code::SKIPOVER_ASCII_ART),
    ("TC_LABEL_COL", code::TC_LABEL_COL),
    ("TC_LABEL_FILE", code::TC_LABEL_FILE),
    ("TC_LABEL_LANG", code::TC_LABEL_LANG),
    ("TC_LABEL_LEVL", code::TC_LABEL_LEVL),
    ("TC_LABEL_OPT", code::TC_LABEL_OPT),
    ("TC_MAIN_ERROR_LOAD_CONFIG", code::TC_MAIN_ERROR_LOAD_CONFIG),
    ("TC_OPT_ACCESS", code::TC_OPT_ACCESS),
    ("TC_OPT_ASCII", code::TC_OPT_ASCII),
    ("TC_OPT_ASHTML", code::TC_OPT_ASHTML),
    ("TC_OPT_ASXML", code::TC_OPT_ASXML),
    ("TC_OPT_BARE", code::TC_OPT_BARE),
    ("TC_OPT_BIG5", code::TC_OPT_BIG5),
    ("TC_OPT_CLEAN", code::TC_OPT_CLEAN),
    ("TC_OPT_CONFIG", code::TC_OPT_CONFIG),
    ("TC_OPT_ERRORS", code::TC_OPT_ERRORS),
    ("TC_OPT_FILE", code::TC_OPT_FILE),
    ("TC_OPT_GDOC", code::TC_OPT_GDOC),
    ("TC_OPT_HELP", code::TC_OPT_HELP),
    ("TC_OPT_HELPCFG", code::TC_OPT_HELPCFG),
    ("TC_OPT_HELPENV", code::TC_OPT_HELPENV),
    ("TC_OPT_HELPOPT", code::TC_OPT_HELPOPT),
    ("TC_OPT_IBM858", code::TC_OPT_IBM858),
    ("TC_OPT_INDENT", code::TC_OPT_INDENT),
    ("TC_OPT_ISO2022", code::TC_OPT_ISO2022),
    ("TC_OPT_LANGUAGE", code::TC_OPT_LANGUAGE),
    ("TC_OPT_LATIN0", code::TC_OPT_LATIN0),
    ("TC_OPT_LATIN1", code::TC_OPT_LATIN1),
    ("TC_OPT_MAC", code::TC_OPT_MAC),
    ("TC_OPT_MODIFY", code::TC_OPT_MODIFY),
    ("TC_OPT_NUMERIC", code::TC_OPT_NUMERIC),
    ("TC_OPT_OMIT", code::TC_OPT_OMIT),
    ("TC_OPT_OUTPUT", code::TC_OPT_OUTPUT),
    ("TC_OPT_QUIET", code::TC_OPT_QUIET),
    ("TC_OPT_RAW", code::TC_OPT_RAW),
    ("TC_OPT_SHIFTJIS", code::TC_OPT_SHIFTJIS),
    ("TC_OPT_SHOWCFG", code::TC_OPT_SHOWCFG),
    ("TC_OPT_EXP_CFG", code::TC_OPT_EXP_CFG),
    ("TC_OPT_EXP_DEF", code::TC_OPT_EXP_DEF),
    ("TC_OPT_UPPER", code::TC_OPT_UPPER),
    ("TC_OPT_UTF16", code::TC_OPT_UTF16),
    ("TC_OPT_UTF16BE", code::TC_OPT_UTF16BE),
    ("TC_OPT_UTF16LE", code::TC_OPT_UTF16LE),
    ("TC_OPT_UTF8", code::TC_OPT_UTF8),
    ("TC_OPT_VERSION", code::TC_OPT_VERSION),
    ("TC_OPT_WIN1252", code::TC_OPT_WIN1252),
    ("TC_OPT_WRAP", code::TC_OPT_WRAP),
    ("TC_OPT_XML", code::TC_OPT_XML),
    ("TC_OPT_XMLCFG", code::TC_OPT_XMLCFG),
    ("TC_OPT_XMLSTRG", code::TC_OPT_XMLSTRG),
    ("TC_OPT_XMLERRS", code::TC_OPT_XMLERRS),
    ("TC_OPT_XMLOPTS", code::TC_OPT_XMLOPTS),
    ("TC_OPT_XMLHELP", code::TC_OPT_XMLHELP),
    ("TC_STRING_CONF_HEADER", code::TC_STRING_CONF_HEADER),
    ("TC_STRING_CONF_NAME", code::TC_STRING_CONF_NAME),
    ("TC_STRING_CONF_TYPE", code::TC_STRING_CONF_TYPE),
    ("TC_STRING_CONF_VALUE", code::TC_STRING_CONF_VALUE),
    ("TC_STRING_CONF_NOTE", code::TC_STRING_CONF_NOTE),
    (
        "TC_STRING_OPT_NOT_DOCUMENTED",
        code::TC_STRING_OPT_NOT_DOCUMENTED,
    ),
    ("TC_STRING_OUT_OF_MEMORY", code::TC_STRING_OUT_OF_MEMORY),
    ("TC_STRING_FATAL_ERROR", code::TC_STRING_FATAL_ERROR),
    ("TC_STRING_FILE_MANIP", code::TC_STRING_FILE_MANIP),
    (
        "TC_STRING_LANG_MUST_SPECIFY",
        code::TC_STRING_LANG_MUST_SPECIFY,
    ),
    ("TC_STRING_LANG_NOT_FOUND", code::TC_STRING_LANG_NOT_FOUND),
    ("TC_STRING_MUST_SPECIFY", code::TC_STRING_MUST_SPECIFY),
    (
        "TC_STRING_PROCESS_DIRECTIVES",
        code::TC_STRING_PROCESS_DIRECTIVES,
    ),
    ("TC_STRING_CHAR_ENCODING", code::TC_STRING_CHAR_ENCODING),
    ("TC_STRING_MISC", code::TC_STRING_MISC),
    ("TC_STRING_XML", code::TC_STRING_XML),
    ("TC_STRING_UNKNOWN_OPTION", code::TC_STRING_UNKNOWN_OPTION),
    (
        "TC_STRING_UNKNOWN_OPTION_B",
        code::TC_STRING_UNKNOWN_OPTION_B,
    ),
    ("TC_STRING_VERS_A", code::TC_STRING_VERS_A),
    ("TC_STRING_VERS_B", code::TC_STRING_VERS_B),
    ("TC_TXT_HELP_1", code::TC_TXT_HELP_1),
    ("TC_TXT_HELP_2A", code::TC_TXT_HELP_2A),
    ("TC_TXT_HELP_2B", code::TC_TXT_HELP_2B),
    ("TC_TXT_HELP_3", code::TC_TXT_HELP_3),
    ("TC_TXT_HELP_3A", code::TC_TXT_HELP_3A),
    ("TC_TXT_HELP_CONFIG", code::TC_TXT_HELP_CONFIG),
    ("TC_TXT_HELP_CONFIG_NAME", code::TC_TXT_HELP_CONFIG_NAME),
    ("TC_TXT_HELP_CONFIG_TYPE", code::TC_TXT_HELP_CONFIG_TYPE),
    ("TC_TXT_HELP_CONFIG_ALLW", code::TC_TXT_HELP_CONFIG_ALLW),
    ("TC_TXT_HELP_ENV_1", code::TC_TXT_HELP_ENV_1),
    ("TC_TXT_HELP_ENV_1A", code::TC_TXT_HELP_ENV_1A),
    ("TC_TXT_HELP_ENV_1B", code::TC_TXT_HELP_ENV_1B),
    ("TC_TXT_HELP_ENV_1C", code::TC_TXT_HELP_ENV_1C),
    ("TC_TXT_HELP_LANG_1", code::TC_TXT_HELP_LANG_1),
    ("TC_TXT_HELP_LANG_2", code::TC_TXT_HELP_LANG_2),
    ("TC_TXT_HELP_LANG_3", code::TC_TXT_HELP_LANG_3),
    ("TIDYSTRINGS_LAST", code::TIDYSTRINGS_LAST),
];
