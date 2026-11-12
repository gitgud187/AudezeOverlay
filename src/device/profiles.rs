//! Exact USB identities traced from Audeze App's backend. See
//! docs/DEVICE_PROTOCOL.md for provenance and the hardware validation boundary.
use crate::model::{Capabilities, Connection, HeadsetFamily};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub product_id: u16,
    pub family: HeadsetFamily,
    pub connection: Connection,
    pub xbox: bool,
}
impl Profile {
    /// The Maxwell 1 on its Bluetooth control channel.
    ///
    /// Bluetooth reports no USB product ID, so `product_id` is zero. The headset
    /// is recognized by its exact paired name and its control service, not by any
    /// ID. Whether it is the Xbox or PlayStation variant is not determined over
    /// Bluetooth (the headset-type read is unverified), so `xbox` is false; it only
    /// selects EQ names, and this model has no EQ editing.
    pub fn bluetooth_maxwell() -> Self {
        Self {
            product_id: 0,
            family: HeadsetFamily::Maxwell,
            connection: Connection::Bluetooth,
            xbox: false,
        }
    }
    pub fn supports_query(self, query: super::protocol::Query) -> bool {
        use super::protocol::Query;
        if self.connection == Connection::Bluetooth {
            // Only the reads verified over Bluetooth; no ANC and no EQ.
            return matches!(
                query,
                Query::Mix
                    | Query::Side
                    | Query::Level
                    | Query::Noise
                    | Query::Mute
                    | Query::Battery
                    | Query::MaxSidetone
            );
        }
        match query {
            Query::Anc => self.capabilities().anc,
            Query::EqBands | Query::EqWriteAck => self.capabilities().eq_edit,
            _ => true,
        }
    }
    pub fn supports_command(self, command: &super::command::Command) -> bool {
        use super::command::Command;
        if self.connection == Connection::Bluetooth {
            // Only the writes verified over Bluetooth.
            return matches!(
                command,
                Command::Mix(_)
                    | Command::Sidetone(_)
                    | Command::SidetoneLevel(_)
                    | Command::Noise(_)
            );
        }
        match command {
            Command::Anc(_) => self.capabilities().anc,
            Command::EqLoad(_) | Command::EqGain { .. } => self.capabilities().eq_edit,
            _ => true,
        }
    }
    pub fn from_usb(vendor: u16, product: u16) -> Option<Self> {
        if vendor != 0x3329 {
            return None;
        }
        let (family, connection, xbox) = match product {
            0x4b18 => (HeadsetFamily::Maxwell, Connection::WirelessDongle, true),
            0x4b19 => (HeadsetFamily::Maxwell, Connection::WirelessDongle, false),
            0x4b1a => (HeadsetFamily::Maxwell, Connection::WiredUsb, false),
            0x4b1e => (HeadsetFamily::Maxwell, Connection::WiredUsb, true),
            0x4b28 => (HeadsetFamily::Maxwell2, Connection::WirelessDongle, true),
            0x4b29 => (HeadsetFamily::Maxwell2, Connection::WirelessDongle, false),
            0x4b2a => (HeadsetFamily::Maxwell2, Connection::WiredUsb, false),
            0x4b2e => (HeadsetFamily::Maxwell2, Connection::WiredUsb, true),
            0x4ba8 => (HeadsetFamily::Maxwell2Anc, Connection::WirelessDongle, true),
            0x4ba9 => (
                HeadsetFamily::Maxwell2Anc,
                Connection::WirelessDongle,
                false,
            ),
            0x4baa => (HeadsetFamily::Maxwell2Anc, Connection::WiredUsb, false),
            0x4bae => (HeadsetFamily::Maxwell2Anc, Connection::WiredUsb, true),
            _ => return None,
        };
        Some(Self {
            product_id: product,
            family,
            connection,
            xbox,
        })
    }
    pub fn capabilities(self) -> Capabilities {
        Capabilities {
            anc: self.family == HeadsetFamily::Maxwell2Anc,
            eq_edit: self.family == HeadsetFamily::Maxwell2Anc,
        }
    }
    pub fn route(self) -> u8 {
        if self.connection == Connection::WirelessDongle {
            0x80
        } else {
            0
        }
    }
    pub fn verified(self) -> bool {
        self.product_id == 0x4ba9
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_do_not_infer_support_from_product_ranges() {
        assert!(Profile::from_usb(0x3329, 0x4ba7).is_none());
        assert!(Profile::from_usb(0x1234, 0x4ba9).is_none());
        let xbox = Profile::from_usb(0x3329, 0x4ba8).unwrap();
        assert!(xbox.capabilities().anc && xbox.xbox);
        assert_eq!(xbox.route(), 0x80);
        let cable = Profile::from_usb(0x3329, 0x4baa).unwrap();
        assert_eq!(cable.family, HeadsetFamily::Maxwell2Anc);
        assert_eq!(cable.route(), 0);
        assert!(!cable.verified());
        for pid in [0x4b18, 0x4b19, 0x4b28, 0x4b29] {
            let p = Profile::from_usb(0x3329, pid).unwrap();
            assert!(!p.capabilities().anc && !p.capabilities().eq_edit);
            assert!(!p.supports_command(&super::super::command::Command::Anc("On".into())));
            assert!(!p.supports_query(super::super::protocol::Query::Anc));
            assert!(!p.supports_command(&super::super::command::Command::EqLoad(6)));
            assert!(p.supports_command(&super::super::command::Command::EqSelect(6)));
        }
    }

    #[test]
    fn bluetooth_allows_only_the_verified_reads_and_writes() {
        use super::super::{command::Command, protocol::Query};
        let profile = Profile::bluetooth_maxwell();
        assert!(!profile.verified());
        for query in [
            Query::Mix,
            Query::Side,
            Query::Level,
            Query::Noise,
            Query::Mute,
            Query::Battery,
            Query::MaxSidetone,
        ] {
            assert!(profile.supports_query(query), "{query:?}");
        }
        for query in [
            Query::Anc,
            Query::EqIndex,
            Query::EqBands,
            Query::EqWriteAck,
            Query::EqSelectAck,
        ] {
            assert!(!profile.supports_query(query), "{query:?}");
        }
        for command in [
            Command::Mix(50),
            Command::Sidetone(true),
            Command::SidetoneLevel(40),
            Command::Noise("Low".into()),
        ] {
            assert!(profile.supports_command(&command), "{command:?}");
        }
        for command in [
            Command::Anc("On".into()),
            Command::EqSelect(2),
            Command::EqRefresh,
            Command::EqLoad(6),
        ] {
            assert!(!profile.supports_command(&command), "{command:?}");
        }
    }

    #[test]
    fn the_linux_udev_rule_lists_exactly_the_recognized_products() {
        let rule = include_str!("../../packaging/linux/70-audeze-overlay.rules");
        let listed: std::collections::BTreeSet<u16> = rule
            .split(r#"ATTRS{idProduct}=="#)
            .nth(1)
            .and_then(|rest| rest.trim_start_matches('"').split('"').next())
            .expect("the rule matches on product IDs")
            .split('|')
            .map(|id| u16::from_str_radix(id, 16).expect("hexadecimal product ID"))
            .collect();
        let recognized: std::collections::BTreeSet<u16> = (0..=u16::MAX)
            .filter(|product| Profile::from_usb(0x3329, *product).is_some())
            .collect();
        assert_eq!(
            listed, recognized,
            "docs/LINUX.md promises exactly these devices"
        );
        assert!(rule.contains(r#"ATTRS{idVendor}=="3329""#));
    }
}
