//! Notice lifetime is explicit so reconnection cannot hide unrelated failures.
pub(in crate::ui) enum Notice {
    Persistent(String),
    UntilConnected(&'static str),
}

impl Notice {
    pub(in crate::ui) fn text(&self) -> &str {
        match self {
            Self::Persistent(text) => text,
            Self::UntilConnected(text) => text,
        }
    }

    pub(in crate::ui) fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    pub(in crate::ui) fn clear(&mut self) {
        *self = Self::Persistent(String::new());
    }

    pub(in crate::ui) fn connected(&mut self) {
        if matches!(self, Self::UntilConnected(_)) {
            self.clear();
        }
    }
}

impl From<String> for Notice {
    fn from(text: String) -> Self {
        Self::Persistent(text)
    }
}

impl From<&str> for Notice {
    fn from(text: &str) -> Self {
        Self::Persistent(text.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_clears_only_connection_notices() {
        let mut notice = Notice::UntilConnected("Reconnect the headset.");
        assert!(!notice.is_empty());
        notice.connected();
        assert!(notice.is_empty());
        for message in ["Could not save settings", "EQ change was not confirmed"] {
            notice = message.into();
            notice.connected();
            assert_eq!(notice.text(), message);
        }
    }
}
