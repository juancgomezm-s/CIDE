//! Default full case folding (C+F, non-Turkic), Unicode 17.0.0.
//! Comparison only: never normalize or rewrite canonical source bytes.
#[path = "unicode_search_data.rs"]
mod data;

pub fn full_case_fold(ch: char) -> impl ExactSizeIterator<Item = char> {
    let code = ch as u32;
    let (mut output, mut count) = ([ch, '\0', '\0'], 1);
    if ch.is_ascii() {
        output[0] = ch.to_ascii_lowercase();
    } else if let Ok(index) = data::SIMPLE.binary_search_by_key(&code, |&(from, _)| from) {
        output[0] = char::from_u32(data::SIMPLE[index].1).expect("generated Unicode scalar");
    } else if let Ok(index) = data::FULL.binary_search_by_key(&code, |&(from, _, _)| from) {
        let (_, mapping, len) = data::FULL[index];
        count = len;
        for i in 0..len {
            output[i] = char::from_u32(mapping[i]).expect("generated Unicode scalar");
        }
    }
    output.into_iter().take(count)
}

/// Search word character: UCD L/M/N/Pc plus join controls, Unicode 17.
/// A deliberate editor rule, not locale-specific UAX #29 word segmentation.
pub fn is_search_word(ch: char) -> bool {
    let code = ch as u32;
    let next = data::WORD.partition_point(|&(start, _)| start <= code);
    next > 0 && code <= data::WORD[next - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_folding_conforms_to_every_default_upstream_mapping() {
        let mut checked = 0;
        for line in include_str!("../../../third_party/unicode-17/CaseFolding.txt").lines() {
            let fields: Vec<_> = line.split('#').next().unwrap().split(';').collect();
            if fields.len() < 3 || !matches!(fields[1].trim(), "C" | "F") {
                continue;
            }
            let from = char::from_u32(u32::from_str_radix(fields[0].trim(), 16).unwrap()).unwrap();
            let expected: String = fields[2]
                .split_whitespace()
                .map(|s| char::from_u32(u32::from_str_radix(s, 16).unwrap()).unwrap())
                .collect();
            assert_eq!(
                full_case_fold(from).collect::<String>(),
                expected,
                "U+{:04X}",
                from as u32
            );
            checked += 1;
        }
        assert!(checked > 1500);
        assert_eq!(
            "Straße Σς İ"
                .chars()
                .flat_map(full_case_fold)
                .collect::<String>(),
            "strasse σσ i\u{307}"
        );
    }
    #[test]
    fn word_rule_includes_marks_connectors_and_supplementary_letters() {
        for ch in ['ñ', '\u{301}', '\u{203f}', '\u{200d}', '\u{10400}', '𰀀'] {
            assert!(is_search_word(ch));
        }
        for ch in [' ', '.', '\r', '\n', '😀'] {
            assert!(!is_search_word(ch));
        }
    }
}
