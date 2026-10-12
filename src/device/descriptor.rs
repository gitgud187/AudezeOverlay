//! A minimal reader for HID report descriptors: the top-level collections and
//! the largest input and output report each defines.
//!
//! Linux exposes one hidraw node per USB HID interface, and the interface can
//! hold several collections (consumer controls, the vendor control channel,
//! telephony). Windows presents each collection separately and reports their
//! sizes directly; here they are derived from the descriptor the kernel
//! publishes. Pure and bounded, so it is tested without hardware.
use std::collections::BTreeMap;

/// The HID specification caps a report descriptor at 4096 bytes.
pub(super) const MAX_DESCRIPTOR_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Collection {
    pub usage_page: u32,
    pub usage: u32,
    /// Largest input report in bytes, report ID included.
    pub max_input_bytes: usize,
    /// Largest output report in bytes, report ID included.
    pub max_output_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct Globals {
    usage_page: u32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

/// Sizes of the reports of one top-level collection, in bits by report ID.
#[derive(Default)]
struct Reports {
    input: BTreeMap<u8, u64>,
    output: BTreeMap<u8, u64>,
}
impl Reports {
    fn largest(bits: &BTreeMap<u8, u64>) -> usize {
        bits.iter()
            // An unrepresentable length must stay too large for transport
            // validation, including on 32-bit hosts; never truncate it.
            .map(|(id, bits)| {
                usize::try_from(bits.div_ceil(8))
                    .unwrap_or(usize::MAX)
                    .saturating_add(usize::from(*id != 0))
            })
            .max()
            .unwrap_or(0)
    }
}

/// Returns the top-level collections in descriptor order. A truncated or
/// malformed descriptor yields the collections that were completed before the
/// problem, never a panic.
pub(super) fn collections(descriptor: &[u8]) -> Vec<Collection> {
    let descriptor = &descriptor[..descriptor.len().min(MAX_DESCRIPTOR_BYTES)];
    let mut found = Vec::new();
    let mut globals = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    // The first usage of the upcoming collection, with the usage page an extended
    // (4-byte) usage names for itself. That page never becomes the global one.
    let mut first_usage: Option<(Option<u32>, u32)> = None;
    let mut depth = 0u32;
    let mut open: Option<(u32, u32, Reports)> = None;
    let mut at = 0;
    while at < descriptor.len() {
        let prefix = descriptor[at];
        if prefix == 0xFE {
            // Long items carry no information this reader needs.
            let Some(length) = descriptor.get(at + 1) else {
                break;
            };
            at += 3 + *length as usize;
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            n => n as usize,
        };
        let Some(data) = descriptor.get(at + 1..at + 1 + size) else {
            break;
        };
        at += 1 + size;
        let value = data
            .iter()
            .rev()
            .fold(0u32, |acc, byte| (acc << 8) | *byte as u32);
        match ((prefix >> 2) & 3, prefix >> 4) {
            // Global: usage page, report size, report ID, report count, push, pop.
            (1, 0) => globals.usage_page = value,
            (1, 7) => globals.report_size = value,
            (1, 8) => match u8::try_from(value) {
                Ok(id) if id != 0 => globals.report_id = id,
                _ => break,
            },
            (1, 9) => globals.report_count = value,
            (1, 10) => stack.push(globals),
            (1, 11) => globals = stack.pop().unwrap_or(globals),
            // Local: the first usage names the collection that follows.
            (2, 0) => {
                let usage = if size == 4 {
                    (Some(value >> 16), value & 0xFFFF)
                } else {
                    (None, value)
                };
                first_usage.get_or_insert(usage);
            }
            // Main: collection (10) and end collection (12).
            (0, 10) => {
                if depth == 0 {
                    let (page, usage) = first_usage.unwrap_or((None, 0));
                    open = Some((
                        page.unwrap_or(globals.usage_page),
                        usage,
                        Reports::default(),
                    ));
                }
                depth += 1;
                first_usage = None;
            }
            (0, 12) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((usage_page, usage, reports)) = open.take() {
                        found.push(Collection {
                            usage_page,
                            usage,
                            max_input_bytes: Reports::largest(&reports.input),
                            max_output_bytes: Reports::largest(&reports.output),
                        });
                    }
                }
                first_usage = None;
            }
            // Main: input (8) and output (9) add bits to the current report.
            (0, tag @ (8 | 9)) => {
                if let Some((_, _, reports)) = open.as_mut() {
                    let bits = u64::from(globals.report_size) * u64::from(globals.report_count);
                    let target = if tag == 8 {
                        &mut reports.input
                    } else {
                        &mut reports.output
                    };
                    let count = target.entry(globals.report_id).or_default();
                    let Some(total) = count.checked_add(bits) else {
                        // Do not return the incomplete collection with a
                        // wrapped length that could pass interface selection.
                        break;
                    };
                    *count = total;
                }
                first_usage = None;
            }
            // Main: feature (11) and anything else still ends the local state.
            (0, _) => first_usage = None,
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 167-byte report descriptor read from `/sys/class/hidraw/hidraw0` for
    /// an Audeze Maxwell Xbox dongle (`3329:4b18`, firmware v1.0.1.74) on Linux.
    /// It is hardware-derived data, not a synthetic fixture.
    const MAXWELL_XBOX_DONGLE: [u8; 167] = [
        0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x0c, 0x15, 0x00, 0x25, 0x01, 0x09, 0xe9, 0x09,
        0xea, 0x09, 0xe2, 0x09, 0xcd, 0x09, 0xb5, 0x09, 0xb6, 0x75, 0x01, 0x95, 0x06, 0x81, 0x02,
        0x09, 0x00, 0x95, 0x02, 0x81, 0x02, 0xc0, 0x06, 0x13, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x15,
        0x00, 0x26, 0xff, 0x00, 0x85, 0x06, 0x09, 0x00, 0x75, 0x08, 0x95, 0x3d, 0x91, 0x02, 0x85,
        0x07, 0x09, 0x00, 0x75, 0x08, 0x95, 0x3d, 0x81, 0x02, 0x85, 0x03, 0x09, 0x00, 0x75, 0x08,
        0x95, 0x0d, 0x81, 0x02, 0x85, 0x04, 0x09, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0xc0,
        0x05, 0x0b, 0x09, 0x05, 0xa1, 0x01, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x85, 0x05, 0x09,
        0x20, 0x95, 0x01, 0x81, 0x22, 0x09, 0x2f, 0x95, 0x01, 0x81, 0x06, 0x09, 0x24, 0x09, 0x21,
        0x09, 0x97, 0x09, 0x2a, 0x09, 0x50, 0x95, 0x05, 0x81, 0x06, 0x09, 0x07, 0x05, 0x09, 0x09,
        0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x05, 0x08, 0x85, 0x05, 0x09, 0x17, 0x09, 0x09,
        0x09, 0x18, 0x09, 0x20, 0x09, 0x21, 0x09, 0x2a, 0x95, 0x06, 0x91, 0x22, 0x95, 0x02, 0x91,
        0x01, 0xc0,
    ];

    #[test]
    fn the_vendor_control_collection_reports_the_sizes_windows_would() {
        let found = collections(&MAXWELL_XBOX_DONGLE);
        let pages: Vec<_> = found.iter().map(|c| (c.usage_page, c.usage)).collect();
        assert_eq!(pages, [(0x0c, 1), (0xff13, 1), (0x0b, 5)]);
        let vendor = &found[1];
        // Output report 6 and input report 7 each carry 61 bytes plus the ID.
        assert_eq!(vendor.max_output_bytes, 62);
        assert_eq!(vendor.max_input_bytes, 62);
        // The other collections must never be mistaken for the control channel.
        assert!(found
            .iter()
            .filter(|c| (c.usage_page, c.usage) != (0xff13, 1))
            .all(|c| c.max_output_bytes < 62));
    }

    #[test]
    fn nested_collections_stay_inside_their_top_level_collection() {
        // Application(vendor) { Logical { input 8 bits, report 1 } } then another application.
        let descriptor = [
            0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x09, 0x02, 0xa1, 0x00, 0x85, 0x01, 0x75,
            0x08, 0x95, 0x04, 0x81, 0x02, 0xc0, 0xc0, 0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0xc0,
        ];
        let found = collections(&descriptor);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].usage_page, found[0].usage), (0xff00, 1));
        assert_eq!(found[0].max_input_bytes, 5);
        assert_eq!((found[1].usage_page, found[1].usage), (1, 6));
    }

    #[test]
    fn an_extended_usage_does_not_change_the_usage_page_of_later_collections() {
        // Collection 1 names page ff13 through a 4-byte usage; collection 2 sets
        // only a usage, so it must not inherit page ff13.
        let descriptor = [
            0x0b, 0x01, 0x00, 0x13, 0xff, 0xa1, 0x01, 0xc0, 0x09, 0x01, 0xa1, 0x01, 0xc0,
        ];
        let found = collections(&descriptor);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].usage_page, found[0].usage), (0xff13, 1));
        assert_eq!((found[1].usage_page, found[1].usage), (0, 1));
    }

    #[test]
    fn truncated_or_garbage_descriptors_never_panic_and_stay_bounded() {
        for cut in 0..MAXWELL_XBOX_DONGLE.len() {
            let found = collections(&MAXWELL_XBOX_DONGLE[..cut]);
            assert!(found.len() <= 3, "a prefix cannot invent collections");
        }
        // A long item claiming more data than exists, and a pseudo-random stream.
        assert!(collections(&[0xFE, 0xFF, 0x00]).is_empty());
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<u8> = (0..10_000)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            })
            .collect();
        let _ = collections(&noise);
        assert!(collections(&[]).is_empty());
    }

    #[test]
    fn overflowing_report_lengths_discard_only_the_incomplete_collection() {
        let malformed = [
            0x06, 0x13, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x07, 0x77, 0xff, 0xff, 0xff, 0xff,
            0x97, 0xff, 0xff, 0xff, 0xff, 0x81, 0x02, 0x81, 0x02, 0xc0,
        ];
        assert!(collections(&malformed).is_empty());
        let mut descriptor = MAXWELL_XBOX_DONGLE.to_vec();
        descriptor.extend_from_slice(&malformed);
        assert_eq!(collections(&descriptor), collections(&MAXWELL_XBOX_DONGLE));
    }

    #[test]
    fn invalid_report_ids_cannot_alias_a_control_report() {
        for id in [0u16, 263] {
            let [low, high] = id.to_le_bytes();
            let descriptor = [
                0x06, 0x13, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x86, low, high, 0x75, 8, 0x95, 61, 0x81,
                2, 0xc0,
            ];
            assert!(collections(&descriptor).is_empty());
        }
    }
}
