//! Line breaking with the font's exact advances (CSS `white-space: normal` and `overflow-wrap: anywhere`).

use super::text::{advance, Style};

/// Width of `text` on one line (advances plus letter-spacing after every glyph).
pub fn measure(style: &Style, text: &str) -> f32 {
    text.chars().map(|c| advance(style, c) + style.spacing).sum()
}

/// Splits `text` into lines no wider than `max_w`. Words break at spaces; a word wider than a whole line is kept whole
/// unless `anywhere` is set, in which case it breaks between any two characters. `\n` always breaks.
pub fn wrap(text: &str, style: &Style, max_w: f32, anywhere: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut line_w = 0.0;
        let push_word = |word: &str, line: &mut String, line_w: &mut f32, lines: &mut Vec<String>| {
            let space = if line.is_empty() { 0.0 } else { advance(style, ' ') + style.spacing };
            let w = measure(style, word);
            if line.is_empty() || *line_w + space + w <= max_w {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
                *line_w += space + w;
                return;
            }
            lines.push(std::mem::take(line));
            *line_w = 0.0;
            if w <= max_w || !anywhere {
                line.push_str(word);
                *line_w = w;
            } else {
                // break the oversized word between characters
                for ch in word.chars() {
                    let cw = advance(style, ch) + style.spacing;
                    if *line_w + cw > max_w && !line.is_empty() {
                        lines.push(std::mem::take(line));
                        *line_w = 0.0;
                    }
                    line.push(ch);
                    *line_w += cw;
                }
            }
        };
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            // a first word that is itself too wide, in `anywhere` mode, must be broken too
            if anywhere && line.is_empty() && measure(style, word) > max_w {
                line.push(' ');
                line.clear();
                let mut tmp = String::new();
                let mut tw = 0.0;
                for ch in word.chars() {
                    let cw = advance(style, ch) + style.spacing;
                    if tw + cw > max_w && !tmp.is_empty() {
                        lines.push(std::mem::take(&mut tmp));
                        tw = 0.0;
                    }
                    tmp.push(ch);
                    tw += cw;
                }
                line = tmp;
                line_w = tw;
                continue;
            }
            push_word(word, &mut line, &mut line_w, &mut lines);
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::text::Weight;

    fn s() -> Style {
        Style::mono(10.0, Weight::W400) // 6 px per glyph
    }

    #[test]
    fn measure_is_the_sum_of_exact_advances() {
        assert!((measure(&s(), "hello") - 30.0).abs() < 0.01);
        assert!((measure(&s().spacing_px(1.0), "hello") - 35.0).abs() < 0.01);
        assert_eq!(measure(&s(), ""), 0.0);
    }

    #[test]
    fn words_wrap_at_spaces_and_each_line_fits() {
        let lines = wrap("one two three four five", &s(), 60.0, false);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(measure(&s(), l) <= 60.0 + 0.01, "{l:?} is too wide");
        }
        assert_eq!(lines.join(" "), "one two three four five", "no words lost or reordered");
    }

    #[test]
    fn short_text_stays_on_one_line_and_newlines_are_honoured() {
        assert_eq!(wrap("hi there", &s(), 500.0, false), vec!["hi there"]);
        assert_eq!(wrap("a\nb", &s(), 500.0, false), vec!["a", "b"]);
        assert_eq!(wrap("", &s(), 100.0, false), vec![""]);
    }

    #[test]
    fn an_oversized_word_overflows_normally_but_breaks_anywhere_on_request() {
        let long = "C:\\Users\\demo\\AppData\\Roaming\\dev.aiusage.monitor\\usage.db";
        let normal = wrap(long, &s(), 100.0, false);
        assert_eq!(normal, vec![long], "without overflow-wrap the word stays whole");
        let anywhere = wrap(long, &s(), 100.0, true);
        assert!(anywhere.len() > 2);
        for l in &anywhere {
            assert!(measure(&s(), l) <= 100.0 + 0.01, "{l:?}");
        }
        assert_eq!(anywhere.concat(), long, "breaking loses nothing");
    }

    #[test]
    fn a_long_word_after_short_ones_breaks_too() {
        let lines = wrap("db C:\\very\\long\\path\\that\\will\\not\\fit\\anywhere", &s(), 90.0, true);
        for l in &lines {
            assert!(measure(&s(), l) <= 90.0 + 0.01, "{l:?}");
        }
        assert!(lines.concat().replace(' ', "").contains("db"));
    }
}
