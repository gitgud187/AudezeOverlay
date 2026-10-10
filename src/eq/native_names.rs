//! Read-only import of Qt's per-user labels for the supported PS ANC dongle.
//! Names are PC-local; the headset stores the curves, not these labels.
use super::{Preferences, PRESET_NAMES};
use std::path::PathBuf;
impl Preferences {
    pub fn import_native_names(&mut self) {
        let Some(root) = std::env::var_os("LOCALAPPDATA") else {
            return;
        };
        let path = PathBuf::from(root).join("Audeze/Audeze/settings.ini");
        // Never read firmware caches or unbounded user files on startup.
        if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= 65536) {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            self.import_ini(&text);
        }
    }
    fn import_ini(&mut self, text: &str) {
        let mut section = "";
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
            }
            if section != "maxwell_2_anc" {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() != "ps\\eq_names" {
                continue;
            }
            let names = qt_list(value);
            if names.len() != 4 {
                return;
            }
            for (i, name) in names.into_iter().enumerate() {
                if self.names[i] == PRESET_NAMES[i + 6] && !name.trim().is_empty() {
                    self.names[i] = name.trim().chars().take(32).collect();
                }
            }
        }
    }
}
fn qt_list(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                result.push(word.trim().to_owned());
                word.clear();
            }
            '\\' => match chars.next() {
                Some('x') => {
                    let mut digits = String::new();
                    while digits.len() < 4 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
                        digits.push(chars.next().unwrap());
                    }
                    if let Some(c) = u32::from_str_radix(&digits, 16)
                        .ok()
                        .and_then(char::from_u32)
                    {
                        word.push(c);
                    }
                }
                Some('n') => word.push('\n'),
                Some('t') => word.push('\t'),
                Some(c) => word.push(c),
                None => (),
            },
            c => word.push(c),
        }
    }
    result.push(word.trim().to_owned());
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_only_supported_device_and_keeps_overlay_renames() {
        let mut prefs = Preferences::default();
        prefs.names[1] = "My own name".into();
        prefs.import_ini("[maxwell_2]\nps\\eq_names=Wrong, Wrong, Wrong, Wrong\n[maxwell_2_anc]\nps\\eq_names=Gaming, Native, , \n");
        assert_eq!(
            prefs.names,
            ["Gaming", "My own name", "Custom 3", "Custom 4"]
        );
    }
    #[test]
    fn handles_quoted_commas_and_qt_unicode_escapes() {
        assert_eq!(
            qt_list(r#""Bass, warm", Caf\xe9, , "#),
            ["Bass, warm", "Café", "", ""]
        );
    }
}
