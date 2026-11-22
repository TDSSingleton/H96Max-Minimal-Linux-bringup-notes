//! Channel/film/series names as the menus sort and jump by them: shared leads ("|FR| ",
//! "[EN] ") stripped, only letters and digits counted, accents folded.

/// A lead: a code at the start of a name that a provider puts on a whole category. Returns the
/// code (normalized: its words in upper case, one space apart) and the rest of the name.
///   encased, any length:  "[Italy] Rai 1", "|FR| TF1", "(Serie TV) Lost", "[EN] BBC One"
///   one word + ':' / '|' / ' - ', up to 12 letters/digits: "ITALY: Rai 1", "SERIE | Lost"
///   up to 4 letters/digits + any symbol: "FR- M6", "4K/Arte"
///   two capitals + a space: "FR TF1", "UK BBC One"
/// Longer plain words ("BBC One", "France 2", "Le Mans") are names, not leads. A lead only gets
/// stripped when most of the list shares it (strip_leads).
fn lead(label: &str) -> Option<(String, &str)> {
    encased(label).or_else(|| word_lead(label))
}

fn norm(code: &str) -> String {
    code.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_uppercase()).collect::<Vec<_>>().join(" ")
}

/// The rest after a lead: separators skipped (not an opening bracket: a second lead may
/// follow, "[IT] [Serie] Gomorra"); None if no name is left.
fn after_lead(after: &str) -> Option<&str> {
    let rest = after.trim_start_matches(|c: char| !c.is_alphanumeric() && !"[({<«".contains(c));
    (!rest.is_empty()).then_some(rest)
}

fn encased(label: &str) -> Option<(String, &str)> {
    let t = label.trim_start();
    let open = t.chars().next()?;
    let close = match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        '<' => '>',
        '«' => '»',
        '|' => '|',
        _ => return None,
    };
    let inner_start = open.len_utf8();
    let end = t[inner_start..].find(close)? + inner_start;
    let code = norm(&t[inner_start..end]);
    if code.is_empty() || code.chars().count() > 30 {
        return None;
    }
    Some((code, after_lead(&t[end + close.len_utf8()..])?))
}

fn word_lead(label: &str) -> Option<(String, &str)> {
    let t = label.trim_start_matches(|c: char| !c.is_alphanumeric());
    let end = t.find(|c: char| !c.is_alphanumeric())?;
    let (code, after) = t.split_at(end);
    let rest = after_lead(after)?;
    let sep = &after[..after.len() - rest.len()];
    let n = code.chars().count();
    let strong = sep.contains([':', '|']) || sep.contains(" - ");
    let ok = if sep.trim().is_empty() {
        // a space alone separates words: only a two-capital country/language code ("FR TF1")
        n == 2 && code.chars().all(|c| c.is_ascii_uppercase())
    } else {
        n <= 4 || (strong && n <= 12)
    };
    ok.then(|| (code.to_uppercase(), rest))
}

/// The letter each label of one list is listed under. Leads that most of the list shares
/// (a category "enclosed" in FR, or in "FR" then "HD") are stripped first, then everything that
/// is not a letter or digit; what is left starts with the letter (all digits count as "#").
pub fn initials(labels: &[&str]) -> Vec<char> {
    strip_leads(labels).iter().map(|l| initial(l)).collect()
}

/// A name without the leads most of its list shares (see initials).
pub fn strip_leads<'a>(labels: &[&'a str]) -> Vec<&'a str> {
    let mut rest: Vec<&'a str> = labels.to_vec();
    for _ in 0..2 {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for l in &rest {
            if let Some((code, _)) = lead(l) {
                match counts.iter_mut().find(|(c, _)| *c == code) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((code, 1)),
                }
            }
        }
        // shared = at least 2 names and 60 % of the list
        let Some((code, _)) = counts.into_iter().filter(|(_, n)| *n >= 2 && n * 10 >= rest.len() * 6).max_by_key(|(_, n)| *n) else {
            break;
        };
        for l in rest.iter_mut() {
            if let Some((c, r)) = lead(l) {
                if c == code {
                    *l = r;
                }
            }
        }
    }
    rest
}

/// First letter or digit, accents folded ("É" -> 'E'); all digits are '#', nothing is '?'.
fn initial(l: &str) -> char {
    match l.chars().find(|c| c.is_alphanumeric()) {
        Some(c) if c.is_numeric() => '#',
        Some(c) => fold(c).to_ascii_uppercase(),
        None => '?',
    }
}

/// Latin letters with accents -> the plain letter (lower case), so É sorts with E, not after Z.
fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// Sort key piece: a run of digits (compared as a number, so "Canal 2" < "Canal 10") or a run
/// of letters (folded, lower case). Everything that is not a letter or digit is dropped.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Part {
    Num(u64, usize), // value, then digit count ("01" after "1")
    Text(String),
}

fn sort_key(l: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut num = String::new();
    for c in l.chars().filter(|c| c.is_alphanumeric()) {
        if c.is_ascii_digit() {
            if !cur.is_empty() {
                out.push(Part::Text(std::mem::take(&mut cur)));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push(Part::Num(num.parse().unwrap_or(u64::MAX), num.len()));
                num.clear();
            }
            cur.push(fold(c));
        }
    }
    if !cur.is_empty() {
        out.push(Part::Text(cur));
    }
    if !num.is_empty() {
        out.push(Part::Num(num.parse().unwrap_or(u64::MAX), num.len()));
    }
    out
}

/// The A-Z order of one list, by the same names the letter jump uses (shared leads stripped,
/// only letters/digits, accents folded, numbers by value; digits first, then A..Z, then
/// names with no letter at all). Stable: equal names keep the provider's order.
/// Returns the original indices in display order.
pub fn az_order(labels: &[&str]) -> Vec<usize> {
    let rest = strip_leads(labels);
    let keys: Vec<(u8, Vec<Part>)> = rest
        .iter()
        .map(|l| {
            let k = sort_key(l);
            (if k.is_empty() { 1 } else { 0 }, k)
        })
        .collect();
    let mut idx: Vec<usize> = (0..labels.len()).collect();
    idx.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    idx
}

/// `fwd` (Vol-): the first item of the next run of a different letter (wrapping round).
/// Back (Vol+): the start of the current run, or of the previous run when already there. On a sorted
/// list that is "next / previous letter"; on an unsorted one it still skips a block at a time.
/// Returns the new position and its letter.
pub fn letter_jump(labels: &[&str], sel: usize, fwd: bool) -> (usize, char) {
    let n = labels.len();
    if n == 0 {
        return (0, '?');
    }
    let ini = initials(labels);
    let sel = sel.min(n - 1);
    let start_of = |i: usize| {
        let mut j = i;
        while j > 0 && ini[j - 1] == ini[i] {
            j -= 1;
        }
        j
    };
    let to = if fwd {
        (sel + 1..n).find(|&i| ini[i] != ini[sel]).unwrap_or(0)
    } else {
        let s = start_of(sel);
        if s != sel {
            s
        } else if sel == 0 {
            start_of(n - 1)
        } else {
            start_of(sel - 1)
        }
    };
    (to, ini[to])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order() {
        let l = ["|FR| TF1", "|FR| arte", "|FR| Canal 10", "|FR| Canal 2", "|FR| 13eme Rue", "|FR| Équipe", "***"];
        let o: Vec<&str> = az_order(&l).into_iter().map(|i| l[i]).collect();
        assert_eq!(o, ["|FR| 13eme Rue", "|FR| arte", "|FR| Canal 2", "|FR| Canal 10", "|FR| Équipe", "|FR| TF1", "***"]);
        // the sorted list jumps one letter at a time
        assert_eq!(initials(&o), vec!['#', 'A', 'C', 'C', 'E', 'T', '?']);
        // equal names keep the provider's order
        let d = ["B (2)", "a", "B (1)", "b"];
        assert_eq!(az_order(&["b x", "B-x", "a"]), vec![2, 0, 1]);
        assert_eq!(az_order(&d)[0], 1);
    }

    #[test]
    fn leads() {
        assert_eq!(lead("|FR| TF1"), Some(("FR".into(), "TF1")));
        assert_eq!(lead("[EN] BBC One"), Some(("EN".into(), "BBC One")));
        assert_eq!(lead("FR: M6"), Some(("FR".into(), "M6")));
        assert_eq!(lead("fr - Arte"), Some(("FR".into(), "Arte")));
        assert_eq!(lead("FR TF1"), Some(("FR".into(), "TF1")));
        assert_eq!(lead("TF1 HD"), None); // space alone + not all capital letters: a name
        assert_eq!(lead("M6 Boutique"), None);
        assert_eq!(lead("TF1 | HD"), Some(("TF1".into(), "HD"))); // a symbol: a candidate, kept only if shared
        assert_eq!(lead("Canal+"), None);
        assert_eq!(lead("Arte"), None);
        assert_eq!(lead("France 2"), None); // 6 letters: a name
        assert_eq!(lead("Le Mans"), None); // "Le" + space alone is a word
        assert_eq!(lead("BBC One"), None);
        assert_eq!(lead("UK BBC One"), Some(("UK".into(), "BBC One")));
        assert_eq!(lead("VIP | Sport 1"), Some(("VIP".into(), "Sport 1")));
        // encased: any length, several words
        assert_eq!(lead("[Italy] Rai 1"), Some(("ITALY".into(), "Rai 1")));
        assert_eq!(lead("[Serie TV] Lost"), Some(("SERIE TV".into(), "Lost")));
        assert_eq!(lead("(Serie) Lost"), Some(("SERIE".into(), "Lost")));
        assert_eq!(lead("|ITALIA| Rai 2"), Some(("ITALIA".into(), "Rai 2")));
        assert_eq!(lead("[IT] [Serie] Gomorra"), Some(("IT".into(), "[Serie] Gomorra")));
        assert_eq!(lead("[Italy]"), None); // nothing after it
        assert_eq!(lead("[unclosed Rai"), None);
        // one longer word with a strong separator
        assert_eq!(lead("ITALY: Rai 1"), Some(("ITALY".into(), "Rai 1")));
        assert_eq!(lead("Serie - Lost"), Some(("SERIE".into(), "Lost")));
        assert_eq!(lead("Italy-Rai"), None); // a plain hyphen: part of the name
        assert_eq!(lead("Internationals: X"), None); // 14 letters: too long for a bare word
    }

    #[test]
    fn initials_strip_shared_leads() {
        // a French category: every name has the lead, so the letters come from the names
        let fr = ["|FR| Arte", "|FR| BFM TV", "|FR| Canal+", "|FR| 13eme Rue", "FR: TF1"];
        assert_eq!(initials(&fr), vec!['A', 'B', 'C', '#', 'T']);
        // two leads deep: FR then HD
        let hd = ["FR | HD | Arte", "FR | HD | M6", "FR | HD | TF1"];
        assert_eq!(initials(&hd), vec!['A', 'M', 'T']);
        // a mixed list: the lead is the useful letter there, so it stays
        let mixed = ["|FR| TF1", "|EN| BBC", "|DE| ZDF", "|IT| Rai 1", "|ES| La 1"];
        assert_eq!(initials(&mixed), vec!['F', 'E', 'D', 'I', 'E']);
        // ordinary names: nothing shared, nothing stripped (BBC One and Two stay under B)
        let plain = ["Alpha", "BBC One", "BBC Two", "***", "24h", "M6 Boutique"];
        assert_eq!(initials(&plain), vec!['A', 'B', 'B', '?', '#', 'M']);
        // long encased leads, two levels: [IT] then [Serie]
        let it = ["[IT] [Serie] Gomorra", "[IT] [Serie] Suburra", "[IT] [Serie] Boris", "[IT] [Serie] 1992"];
        assert_eq!(initials(&it), vec!['G', 'S', 'B', '#']);
        let italy = ["[Italy] Rai 1", "[Italy] Canale 5", "[Italy] La7", "ITALY: Rete 4"];
        assert_eq!(initials(&italy), vec!['R', 'C', 'L', 'R']);
        // a UK category of mostly BBC channels keeps them under B
        let uk = ["BBC One", "BBC Two", "BBC Four", "ITV1"];
        assert_eq!(initials(&uk), vec!['B', 'B', 'B', 'I']);
    }

    #[test]
    fn jumps() {
        let l = ["Alpha", "arte", "BBC One", "BBC Two", "|FR| Canal+", "13th Street", "24h"];
        assert_eq!(letter_jump(&l, 0, true), (2, 'B')); // A -> B
        assert_eq!(letter_jump(&l, 2, true), (4, 'F')); // B -> F (a single |FR| is not shared)
        assert_eq!(letter_jump(&l, 5, true), (0, 'A')); // # (last run) -> wraps to A
        assert_eq!(letter_jump(&l, 3, false), (2, 'B')); // inside B -> start of B
        assert_eq!(letter_jump(&l, 2, false), (0, 'A')); // start of B -> start of A
        assert_eq!(letter_jump(&l, 0, false), (5, '#')); // start of A -> wraps to start of #
        let fr = ["|FR| Arte", "|FR| Arte HD", "|FR| BFM TV", "|FR| TF1"];
        assert_eq!(letter_jump(&fr, 0, true), (2, 'B')); // F would be useless here
        assert_eq!(letter_jump(&["x"], 0, true), (0, 'X'));
    }
}

