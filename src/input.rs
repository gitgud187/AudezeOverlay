//! Input drafts are separate from saved settings: typing never partially commits.
#[derive(Clone, Debug)]
pub struct NumberDraft {
    pub key: &'static str,
    pub text: String,
    pub selected: bool,
}
impl NumberDraft {
    pub fn new(key: &'static str, value: u32) -> Self {
        Self {
            key,
            text: value.to_string(),
            selected: true,
        }
    }
    pub fn type_char(&mut self, ch: char) {
        if ch == '\u{8}' {
            if self.selected {
                self.text.clear();
            } else {
                self.text.pop();
            }
            self.selected = false;
        } else if ch.is_ascii_digit() {
            if self.selected {
                self.text.clear();
            }
            self.selected = false;
            if self.text.len() < 6 {
                self.text.push(ch);
            }
        }
    }
    pub fn value(&self, min: u32, max: u32) -> Result<u32, String> {
        self.text
            .parse::<u32>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("Enter a whole number from {min} to {max} ms."))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_values_keep_single_millisecond_precision() {
        let mut draft = NumberDraft::new("hold", 2500);
        for ch in "797".chars() {
            draft.type_char(ch);
        }
        assert_eq!(draft.value(500, 10000).unwrap(), 797);
        draft.type_char('\u{8}');
        assert!(draft.value(500, 10000).is_err());
        draft.selected = true;
        draft.type_char('0');
        assert_eq!(draft.value(0, 1000).unwrap(), 0);
        draft.type_char('x');
        assert_eq!(draft.text, "0");
    }
}
