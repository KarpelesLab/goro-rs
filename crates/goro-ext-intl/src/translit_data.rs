//! Script transforms of the transliterator written as code (ICU has them
//! as rule files): Greek-Latin (UNGEGN, as ICU), Katakana/Hiragana-Latin
//! (Hepburn, as ICU) and Latin-Katakana/Hiragana.

use intl::unicode as un;

fn nfd(s: &[char]) -> Vec<char> {
    un::nfd(s.iter().copied()).collect()
}

fn nfc(s: &[char]) -> Vec<char> {
    un::nfc(s.iter().copied()).collect()
}

fn greek_base(c: char) -> Option<(&'static str, bool)> {
    // (latin, uppercase)
    let lower = c.to_lowercase().next().unwrap_or(c);
    let up = lower != c;
    let l = match lower {
        'α' => "a",
        'β' | 'ϐ' => "b",
        'γ' => "g",
        'δ' => "d",
        'ε' | 'ϵ' => "e",
        'ζ' => "z",
        'η' => "e\u{304}",
        'θ' | 'ϑ' => "th",
        'ι' => "i",
        'κ' | 'ϰ' => "k",
        'λ' => "l",
        'μ' | 'µ' => "m",
        'ν' => "n",
        'ξ' => "x",
        'ο' => "o",
        'π' | 'ϖ' => "p",
        'ρ' | 'ϱ' => "r",
        'σ' | 'ς' | 'ϲ' => "s",
        'τ' => "t",
        'υ' | 'ϒ' => "y",
        'φ' | 'ϕ' => "ph",
        'χ' => "ch",
        'ψ' => "ps",
        'ω' => "o\u{304}",
        _ => return None,
    };
    Some((l, up))
}

fn is_greek_letter(c: char) -> bool {
    greek_base(c).is_some()
}

fn greek_lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Greek-Latin (ICU's UNGEGN-based rules).
pub fn greek_latin(input: &[char]) -> Vec<char> {
    let t = nfd(input);
    let mut out: Vec<char> = Vec::with_capacity(t.len() * 2);
    let n = t.len();
    // the next base letter after index i (skipping marks)
    let next_letter = |i: usize| -> Option<(usize, char)> {
        let mut j = i + 1;
        while j < n && un::is_mark(t[j]) {
            j += 1;
        }
        (j < n).then(|| (j, t[j]))
    };
    let prev_letter = |i: usize| -> Option<char> {
        let mut j = i;
        while j > 0 {
            j -= 1;
            if !un::is_mark(t[j]) {
                return Some(t[j]);
            }
        }
        None
    };
    let mut i = 0;
    while i < n {
        let c = t[i];
        let Some((lat, up)) = greek_base(c) else {
            match c {
                '\u{313}' => {}                   // smooth breathing
                '\u{314}' => {}                   // rough breathing (handled at the vowel)
                '\u{342}' => out.push('\u{302}'), // perispomeni
                '\u{345}' => out.push('i'),       // ypogegrammeni
                ';' if i > 0 && is_greek_letter(prev_letter(i).unwrap_or(' ')) => out.push('?'),
                '\u{37E}' => out.push('?'),
                '\u{387}' => out.push(';'),
                _ => out.push(c),
            }
            i += 1;
            continue;
        };
        let lc = greek_lower(c);
        // marks on this letter
        let mut j = i + 1;
        while j < n && un::is_mark(t[j]) {
            j += 1;
        }
        let marks = &t[i + 1..j];
        let rough = marks.contains(&'\u{314}');
        // context for the capitalisation of digraphs
        let nl = next_letter(i);
        let next_up = nl.is_some_and(|(_, x)| is_greek_letter(x) && greek_lower(x) != x);
        let prev_up = prev_letter(i).is_some_and(|x| is_greek_letter(x) && greek_lower(x) != x);
        let all_caps = up && (next_up || (nl.is_none_or(|(_, x)| !is_greek_letter(x)) && prev_up));
        let mut lat = lat.to_string();
        // γ before γ κ ξ χ is a nasal
        if lc == 'γ' && nl.is_some_and(|(_, x)| matches!(greek_lower(x), 'γ' | 'κ' | 'ξ' | 'χ'))
        {
            lat = "n".into();
        }
        // υ after α ε η ο ω (no diaeresis) is u
        if lc == 'υ' && !marks.contains(&'\u{308}') {
            if let Some(p) = prev_letter(i) {
                if matches!(greek_lower(p), 'α' | 'ε' | 'η' | 'ο' | 'ω') {
                    // only when the previous vowel carries no diaeresis
                    lat = "u".into();
                }
            }
        }
        if lc == 'ρ' && rough {
            lat = "rh".into();
        }
        let mut s: Vec<char> = Vec::new();
        if rough && lc != 'ρ' {
            s.push('h');
        }
        s.extend(lat.chars());
        if up {
            if all_caps {
                for ch in s.iter_mut() {
                    *ch = ch.to_uppercase().next().unwrap_or(*ch);
                }
            } else if let Some(f) = s.first_mut() {
                *f = f.to_uppercase().next().unwrap_or(*f);
            }
        }
        out.extend(s);
        for &m in marks {
            match m {
                '\u{313}' | '\u{314}' => {}
                '\u{342}' => out.push('\u{302}'),
                '\u{345}' => out.push('i'),
                m => out.push(m),
            }
        }
        i = j;
    }
    nfc(&out)
}

/// The Hepburn romanization of the kana (katakana forms).
const KANA: &[(&str, &str)] = &[
    ("キャ", "kya"),
    ("キュ", "kyu"),
    ("キョ", "kyo"),
    ("シャ", "sha"),
    ("シュ", "shu"),
    ("ショ", "sho"),
    ("シェ", "she"),
    ("チャ", "cha"),
    ("チュ", "chu"),
    ("チョ", "cho"),
    ("チェ", "che"),
    ("ニャ", "nya"),
    ("ニュ", "nyu"),
    ("ニョ", "nyo"),
    ("ヒャ", "hya"),
    ("ヒュ", "hyu"),
    ("ヒョ", "hyo"),
    ("ミャ", "mya"),
    ("ミュ", "myu"),
    ("ミョ", "myo"),
    ("リャ", "rya"),
    ("リュ", "ryu"),
    ("リョ", "ryo"),
    ("ギャ", "gya"),
    ("ギュ", "gyu"),
    ("ギョ", "gyo"),
    ("ジャ", "ja"),
    ("ジュ", "ju"),
    ("ジョ", "jo"),
    ("ジェ", "je"),
    ("ヂャ", "dya"),
    ("ヂュ", "dyu"),
    ("ヂョ", "dyo"),
    ("ビャ", "bya"),
    ("ビュ", "byu"),
    ("ビョ", "byo"),
    ("ピャ", "pya"),
    ("ピュ", "pyu"),
    ("ピョ", "pyo"),
    ("ティ", "ti"),
    ("ディ", "di"),
    ("トゥ", "tu"),
    ("ドゥ", "du"),
    ("ファ", "fa"),
    ("フィ", "fi"),
    ("フェ", "fe"),
    ("フォ", "fo"),
    ("ア", "a"),
    ("イ", "i"),
    ("ウ", "u"),
    ("エ", "e"),
    ("オ", "o"),
    ("カ", "ka"),
    ("キ", "ki"),
    ("ク", "ku"),
    ("ケ", "ke"),
    ("コ", "ko"),
    ("サ", "sa"),
    ("シ", "shi"),
    ("ス", "su"),
    ("セ", "se"),
    ("ソ", "so"),
    ("タ", "ta"),
    ("チ", "chi"),
    ("ツ", "tsu"),
    ("テ", "te"),
    ("ト", "to"),
    ("ナ", "na"),
    ("ニ", "ni"),
    ("ヌ", "nu"),
    ("ネ", "ne"),
    ("ノ", "no"),
    ("ハ", "ha"),
    ("ヒ", "hi"),
    ("フ", "fu"),
    ("ヘ", "he"),
    ("ホ", "ho"),
    ("マ", "ma"),
    ("ミ", "mi"),
    ("ム", "mu"),
    ("メ", "me"),
    ("モ", "mo"),
    ("ヤ", "ya"),
    ("ユ", "yu"),
    ("ヨ", "yo"),
    ("ラ", "ra"),
    ("リ", "ri"),
    ("ル", "ru"),
    ("レ", "re"),
    ("ロ", "ro"),
    ("ワ", "wa"),
    ("ヰ", "wi"),
    ("ヱ", "we"),
    ("ヲ", "wo"),
    ("ガ", "ga"),
    ("ギ", "gi"),
    ("グ", "gu"),
    ("ゲ", "ge"),
    ("ゴ", "go"),
    ("ザ", "za"),
    ("ジ", "ji"),
    ("ズ", "zu"),
    ("ゼ", "ze"),
    ("ゾ", "zo"),
    ("ダ", "da"),
    ("ヂ", "di"),
    ("ヅ", "du"),
    ("デ", "de"),
    ("ド", "do"),
    ("バ", "ba"),
    ("ビ", "bi"),
    ("ブ", "bu"),
    ("ベ", "be"),
    ("ボ", "bo"),
    ("パ", "pa"),
    ("ピ", "pi"),
    ("プ", "pu"),
    ("ペ", "pe"),
    ("ポ", "po"),
    ("ヴ", "vu"),
    ("ァ", "~a"),
    ("ィ", "~i"),
    ("ゥ", "~u"),
    ("ェ", "~e"),
    ("ォ", "~o"),
    ("ャ", "~ya"),
    ("ュ", "~yu"),
    ("ョ", "~yo"),
    ("ヮ", "~wa"),
    ("ヵ", "~ka"),
    ("ヶ", "~ke"),
];

fn kata(c: char) -> char {
    // hiragana -> katakana
    match c as u32 {
        0x3041..=0x3096 => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        _ => c,
    }
}

fn hira(c: char) -> char {
    match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

fn macron(v: char) -> Option<char> {
    Some(match v {
        'a' => 'ā',
        'i' => 'ī',
        'u' => 'ū',
        'e' => 'ē',
        'o' => 'ō',
        _ => return None,
    })
}

/// Katakana-Latin / Hiragana-Latin.
pub fn kana_latin(input: &[char]) -> Vec<char> {
    let t: Vec<char> = input.iter().map(|&c| kata(c)).collect();
    let mut out: Vec<char> = Vec::with_capacity(t.len() * 3);
    let mut i = 0;
    let lookup = |i: usize| -> Option<(usize, &'static str)> {
        for (k, v) in KANA {
            let kc: Vec<char> = k.chars().collect();
            if t.len() >= i + kc.len() && t[i..i + kc.len()] == kc[..] {
                return Some((kc.len(), v));
            }
        }
        None
    };
    while i < t.len() {
        let c = t[i];
        match c {
            'ッ' => {
                // gemination of the next consonant
                match lookup(i + 1) {
                    Some((_, v)) if !v.starts_with(['a', 'i', 'u', 'e', 'o', '~']) => {
                        let f = if v.starts_with("ch") {
                            't'
                        } else {
                            v.chars().next().unwrap()
                        };
                        out.push(f);
                    }
                    _ => out.extend("~tsu".chars()),
                }
                i += 1;
            }
            'ン' => {
                out.push('n');
                if let Some((_, v)) = lookup(i + 1) {
                    if v.starts_with(['a', 'i', 'u', 'e', 'o', 'y']) {
                        out.push('\'');
                    }
                }
                i += 1;
            }
            'ー' => {
                match out.last().copied().and_then(macron) {
                    Some(m) => {
                        out.pop();
                        out.push(m);
                    }
                    None => out.push('ー'),
                }
                i += 1;
            }
            _ => match lookup(i) {
                Some((len, v)) => {
                    out.extend(v.chars());
                    i += len;
                }
                None => {
                    out.push(input[i]);
                    i += 1;
                }
            },
        }
    }
    out
}

/// Latin-Katakana (or Latin-Hiragana).
pub fn latin_kana(input: &[char], hiragana: bool) -> Vec<char> {
    let t: Vec<char> = nfc(input);
    let mut out: Vec<char> = Vec::new();
    let mut i = 0;
    let unmacron = |c: char| -> Option<char> {
        Some(match c {
            'ā' => 'a',
            'ī' => 'i',
            'ū' => 'u',
            'ē' => 'e',
            'ō' => 'o',
            _ => return None,
        })
    };
    let lower: Vec<char> = t
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    // match the longest romaji at i (macron vowels count as their vowel)
    let romaji_at = |i: usize| -> Option<(usize, &'static str, bool)> {
        let mut best: Option<(usize, &'static str, bool)> = None;
        for (k, v) in KANA {
            if v.starts_with('~') {
                continue;
            }
            let vc: Vec<char> = v.chars().collect();
            if lower.len() < i + vc.len() {
                continue;
            }
            let mut long = false;
            let ok = vc.iter().enumerate().all(|(n, &ch)| {
                let x = lower[i + n];
                if x == ch {
                    return true;
                }
                if n == vc.len() - 1 && unmacron(x) == Some(ch) {
                    long = true;
                    return true;
                }
                false
            });
            if ok && best.is_none_or(|b| vc.len() > b.0) {
                best = Some((vc.len(), k, long));
            }
        }
        best
    };
    while i < lower.len() {
        let c = lower[i];
        if c == '\'' {
            i += 1;
            continue;
        }
        // n before a consonant / the end
        if c == 'n' {
            let nx = lower.get(i + 1).copied();
            let vowel_next = nx.is_some_and(|x| "aiueoyāīūēō".contains(x));
            if !vowel_next {
                out.push('ン');
                i += 1;
                if nx == Some('\'') {
                    i += 1;
                }
                continue;
            }
        }
        // doubled consonant (or tch)
        if c.is_ascii_alphabetic() && !"aiueon".contains(c) {
            if let Some(&nx) = lower.get(i + 1) {
                if nx == c || (c == 't' && nx == 'c') {
                    out.push('ッ');
                    i += 1;
                    continue;
                }
            }
        }
        match romaji_at(i) {
            Some((len, k, long)) => {
                out.extend(k.chars());
                if long {
                    out.push('ー');
                }
                i += len;
            }
            None => {
                out.push(t[i]);
                i += 1;
            }
        }
    }
    if hiragana {
        out = out.into_iter().map(hira).collect();
    }
    out
}

/// Hiragana-Katakana / Katakana-Hiragana.
pub fn kana_swap(input: &[char], to_kata: bool) -> Vec<char> {
    input
        .iter()
        .map(|&c| if to_kata { kata(c) } else { hira(c) })
        .collect()
}
