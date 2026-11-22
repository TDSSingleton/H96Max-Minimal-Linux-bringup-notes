//! Text as the screen shows it: how many cells a character takes (CJK = 2, combining marks = 0),
//! and the visual form of right-to-left text (Arabic letters joined through the Unicode
//! presentation forms, RTL runs reversed). Cheap, table-driven, no dependencies: good enough
//! for channel and film names, not a full bidi/shaping engine.

/// Cells a character takes on the console grid.
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x300 {
        return if u < 0x20 || u == 0x7f { 0 } else { 1 };
    }
    if is_mark(c) || matches!(u, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2064 | 0xfe00..=0xfe0f | 0xfeff) {
        return 0;
    }
    let wide = matches!(u,
        0x1100..=0x115f | 0x2e80..=0x303e | 0x3041..=0x33ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff
        | 0xa000..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe30..=0xfe4f | 0xff00..=0xff60
        | 0xffe0..=0xffe6 | 0x1f300..=0x1f64f | 0x1f900..=0x1f9ff | 0x20000..=0x3fffd);
    if wide { 2 } else { 1 }
}

/// Display width of a string in cells.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Combining marks (drawn over the previous letter; we skip them).
fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036f | 0x0483..=0x0489 | 0x0591..=0x05bd | 0x05bf | 0x05c1..=0x05c2
        | 0x05c4..=0x05c5 | 0x05c7 | 0x0610..=0x061a | 0x064b..=0x065f | 0x0670
        | 0x06d6..=0x06dc | 0x06df..=0x06e4 | 0x06e7..=0x06e8 | 0x06ea..=0x06ed
        | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0x3099..=0x309a | 0xfe20..=0xfe2f)
}

/// Strong right-to-left: Hebrew, Arabic (with its presentation forms), Syriac, Thaana, NKo.
pub fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfefc)
        && !is_mark(c)
        && !c.is_ascii_digit()
        && !is_arabic_digit(c)
}

fn is_arabic_digit(c: char) -> bool {
    matches!(c as u32, 0x0660..=0x0669 | 0x06f0..=0x06f9)
}

// ---------- Arabic shaping ----------

#[derive(Clone, Copy, PartialEq)]
enum Join {
    Dual,  // joins on both sides: isolated, final, initial, medial
    Right, // joins only to the letter before it: isolated, final
    None,  // hamza
    Causing, // tatweel: joins both, has no forms of its own
}

/// (presentation-form base = isolated form; final/initial/medial follow at +1/+2/+3)
fn letter(c: char) -> Option<(u32, Join)> {
    use Join::*;
    let (b, j) = match c as u32 {
        0x0621 => (0xfe80, None),
        0x0622 => (0xfe81, Right),
        0x0623 => (0xfe83, Right),
        0x0624 => (0xfe85, Right),
        0x0625 => (0xfe87, Right),
        0x0626 => (0xfe89, Dual),
        0x0627 => (0xfe8d, Right),
        0x0628 => (0xfe8f, Dual),
        0x0629 => (0xfe93, Right),
        0x062a => (0xfe95, Dual),
        0x062b => (0xfe99, Dual),
        0x062c => (0xfe9d, Dual),
        0x062d => (0xfea1, Dual),
        0x062e => (0xfea5, Dual),
        0x062f => (0xfea9, Right),
        0x0630 => (0xfeab, Right),
        0x0631 => (0xfead, Right),
        0x0632 => (0xfeaf, Right),
        0x0633 => (0xfeb1, Dual),
        0x0634 => (0xfeb5, Dual),
        0x0635 => (0xfeb9, Dual),
        0x0636 => (0xfebd, Dual),
        0x0637 => (0xfec1, Dual),
        0x0638 => (0xfec5, Dual),
        0x0639 => (0xfec9, Dual),
        0x063a => (0xfecd, Dual),
        0x0640 => (0x0640, Causing),
        0x0641 => (0xfed1, Dual),
        0x0642 => (0xfed5, Dual),
        0x0643 => (0xfed9, Dual),
        0x0644 => (0xfedd, Dual),
        0x0645 => (0xfee1, Dual),
        0x0646 => (0xfee5, Dual),
        0x0647 => (0xfee9, Dual),
        0x0648 => (0xfeed, Right),
        0x0649 => (0xfeef, Right),
        0x064a => (0xfef1, Dual),
        // Persian / Urdu letters (Presentation Forms-A)
        0x067e => (0xfb56, Dual),  // peh
        0x0686 => (0xfb7a, Dual),  // tcheh
        0x0698 => (0xfb8a, Right), // jeh
        0x06a9 => (0xfb8e, Dual),  // keheh
        0x06af => (0xfb92, Dual),  // gaf
        0x06cc => (0xfbfc, Dual),  // farsi yeh
        _ => return Option::None,
    };
    Some((b, j))
}

/// Can `c` connect to the letter after it (in logical order)?
fn joins_next(c: char) -> bool {
    matches!(letter(c), Some((_, Join::Dual | Join::Causing)))
}

/// Can `c` connect to the letter before it?
fn joins_prev(c: char) -> bool {
    matches!(letter(c), Some((_, Join::Dual | Join::Right | Join::Causing)))
}

/// Lam + alef -> one ligature (isolated form; final = +1).
fn lam_alef(next: char) -> Option<u32> {
    match next as u32 {
        0x0622 => Some(0xfef5),
        0x0623 => Some(0xfef7),
        0x0625 => Some(0xfef9),
        0x0627 => Some(0xfefb),
        _ => None,
    }
}

/// Arabic letters (logical order) -> their joined presentation forms. Marks are dropped (the
/// screen does not draw them); everything else passes through.
fn shape(chars: &[char]) -> Vec<char> {
    let letters: Vec<char> = chars.iter().copied().filter(|c| !is_mark(*c)).collect();
    let mut out = Vec::with_capacity(letters.len());
    let mut i = 0;
    while i < letters.len() {
        let c = letters[i];
        let prev = if i > 0 { Some(letters[i - 1]) } else { None };
        let next = letters.get(i + 1).copied();
        let Some((base, join)) = letter(c) else {
            out.push(c);
            i += 1;
            continue;
        };
        let conn_prev = joins_prev(c) && prev.is_some_and(joins_next);
        if c == '\u{0644}' {
            if let Some(lig) = next.and_then(lam_alef) {
                out.push(char::from_u32(lig + conn_prev as u32).unwrap_or(c));
                i += 2;
                continue;
            }
        }
        let conn_next = joins_next(c) && next.is_some_and(joins_prev);
        let form = match (join, conn_prev, conn_next) {
            (Join::Causing | Join::None, ..) => 0,
            (_, false, false) => 0,
            (_, true, false) => 1,
            (Join::Dual, false, true) => 2,
            (Join::Dual, true, true) => 3,
            (_, true, true) => 1, // a right-joining letter never joins forward
            (_, false, true) => 0,
        };
        let code = if join == Join::Causing { base } else { base + form };
        out.push(char::from_u32(code).unwrap_or(c));
        i += 1;
    }
    out
}

// ---------- right-to-left runs ----------

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        _ => c,
    }
}

fn is_ltr_strong(c: char) -> bool {
    c.is_alphabetic() && !is_rtl(c) && !is_mark(c)
}

fn is_number_part(c: char) -> bool {
    c.is_ascii_digit() || is_arabic_digit(c)
}

/// The visual form of one line: left-to-right paragraph (the UI's direction), each run of
/// right-to-left text (from its first RTL letter to its last RTL letter or number, with the
/// spaces and punctuation inside it) shaped and reversed, numbers inside kept left-to-right, brackets mirrored.
/// The display width never changes (a lam-alef ligature is padded with a space), so layouts
/// computed on the logical text still line up.
pub fn visual(s: &str) -> String {
    if !s.chars().any(is_rtl) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if !is_rtl(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        let mut last_rtl = i;
        let mut j = i + 1;
        while j < chars.len() && !is_ltr_strong(chars[j]) {
            // a number after RTL text (before any Latin letter) belongs to the RTL run (UBA
            // W2/W7/N1: "כאן 11" shows as "11 ןאכ"); a number before it does not
            if is_rtl(chars[j]) || is_number_part(chars[j]) {
                last_rtl = j;
            }
            j += 1;
        }
        let run = &chars[start..=last_rtl];
        let before: usize = run.iter().map(|c| char_width(*c)).sum();
        let shaped = shape(run);
        // reverse, then put each number back in reading order
        let mut rev: Vec<char> = shaped.iter().rev().map(|c| mirror(*c)).collect();
        let mut k = 0;
        while k < rev.len() {
            if is_number_part(rev[k]) {
                let mut e = k;
                while e + 1 < rev.len()
                    && (is_number_part(rev[e + 1])
                        || (matches!(rev[e + 1], '.' | ',' | ':') && rev.get(e + 2).is_some_and(|c| is_number_part(*c))))
                {
                    e += 1;
                }
                rev[k..=e].reverse();
                k = e + 1;
            } else {
                k += 1;
            }
        }
        let after: usize = rev.iter().map(|c| char_width(*c)).sum();
        out.extend(rev);
        out.extend(std::iter::repeat(' ').take(before.saturating_sub(after)));
        i = last_rtl + 1;
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("日本"), 4);
        assert_eq!(width("ＮＨＫ"), 6);
        assert_eq!(width("é"), 1);
        assert_eq!(width("e\u{301}"), 1); // combining acute
        assert_eq!(width("مرحبا"), 5);
        assert_eq!(width("\u{200f}x"), 1);
    }

    #[test]
    fn latin_untouched() {
        assert_eq!(visual("|FR| TF1 HD"), "|FR| TF1 HD");
        assert_eq!(visual("日本テレビ"), "日本テレビ");
    }

    #[test]
    fn hebrew_reversed() {
        // "שלום" (shalom) shown right to left; Latin around it stays
        assert_eq!(visual("TV שלום Max"), "TV םולש Max");
        // a number after RTL text joins the run (shows on its left), one before it does not
        assert_eq!(visual("כאן 11"), "11 ןאכ");
        assert_eq!(visual("11 כאן"), "11 ןאכ");
        assert_eq!(visual("כאן 11 HD"), "11 ןאכ HD");
    }

    #[test]
    fn arabic_joined_and_reversed() {
        // باب (bab): beh initial, alef final, beh isolated (alef does not join forward);
        // visual order is reversed
        let v: Vec<u32> = visual("باب").chars().map(|c| c as u32).collect();
        assert_eq!(v, vec![0xfe8f, 0xfe8e, 0xfe91]);
        // سلام (salam): seen initial, lam-alef ligature (final), meem isolated; width kept
        let v = visual("سلام");
        assert_eq!(width(&v), 4);
        let cps: Vec<u32> = v.chars().map(|c| c as u32).collect();
        assert_eq!(cps, vec![0xfee1, 0xfefc, 0xfeb3, 0x20]);
        // a number inside Arabic keeps its digit order; brackets are mirrored
        let v = visual("قناة 24 (HD)");
        assert!(v.starts_with("24 "));
        assert!(v.ends_with(" (HD)"));
        // محمد (Muhammad): meem initial, hah medial, meem medial, dal final (dal is
        // right-joining); shown reversed. The worked example in research/notes/14-text-rendering.md
        let cps: Vec<u32> = visual("محمد").chars().map(|c| c as u32).collect();
        assert_eq!(cps, vec![0xfeaa, 0xfee4, 0xfea4, 0xfee3]);
        // marks are dropped, the run keeps its width
        assert_eq!(width(&visual("مُحَمَّد")), width("مُحَمَّد"));
    }
}
