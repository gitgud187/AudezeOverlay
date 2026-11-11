//! The byte path between the sound protocol and one kind of connection.
//!
//! The headset speaks the same `05 5A`/`05 5B`/`05 5D` frames whatever carries
//! them. A [`Link`] moves frames in and out and hides everything specific to the
//! connection: for the USB dongle and cable that is the HID report wrapper and
//! route byte, which [`HidLink`] owns.
use super::hid::{Hid, InputStream};

type Result<T> = std::result::Result<T, String>;

pub(super) trait Link {
    /// Sends one complete request frame (`05 5A len …`).
    fn send(&mut self, frame: &[u8]) -> Result<()>;
    /// Replaces `chunk` with the response bytes that have already arrived, without
    /// waiting for any. The bytes are a stream: a frame may span several chunks.
    ///
    /// Returns whether the connection delivered anything at all, even if none of
    /// it was usable. The caller reads again only while that is true and a frame
    /// is still incomplete.
    fn receive(&mut self, chunk: &mut Vec<u8>) -> Result<bool>;
    /// Unsolicited reports, for diagnostics. Connections without them say so.
    fn listen(&self) -> Result<Box<dyn InputStream>> {
        Err("This connection has no unsolicited report stream.".into())
    }
}

/// Frames carried in the vendor HID reports of the dongle or a USB cable.
///
/// An output report is `06 len route body…` and an input report `07 len route
/// body…`, with the body at most 61 bytes. `route` is `0x80` for the dongle and
/// `0` for the cable; a report for the other route is ignored.
pub(super) struct HidLink {
    hid: Box<dyn Hid>,
    /// Length of the output report, report ID included.
    output_len: usize,
    route: u8,
}

impl HidLink {
    pub(super) fn new(hid: Box<dyn Hid>, output_len: usize, route: u8) -> Self {
        Self {
            hid,
            output_len,
            route,
        }
    }
}

impl Link for HidLink {
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let length = u8::try_from(frame.len())
            .ok()
            .filter(|_| frame.len() + 3 <= self.output_len)
            .ok_or("The request does not fit in one output report.")?;
        let mut storage = [0u8; 512];
        let report = storage
            .get_mut(..self.output_len)
            .ok_or("The output report is longer than any supported interface.")?;
        report[0] = 6;
        report[1] = length;
        report[2] = self.route;
        report[3..3 + frame.len()].copy_from_slice(frame);
        self.hid.write_report(report)
    }

    fn receive(&mut self, chunk: &mut Vec<u8>) -> Result<bool> {
        let mut report = [0u8; 64];
        report[0] = 7;
        self.hid.get_input_report(&mut report)?;
        chunk.clear();
        if let Some(body) = report_body(&report, self.route) {
            chunk.extend_from_slice(body);
        }
        Ok(report[1] != 0)
    }

    fn listen(&self) -> Result<Box<dyn InputStream>> {
        self.hid.listen()
    }
}

/// The frame bytes inside one HID input report, or `None` when the report is not
/// a well-formed reply for `route`.
fn report_body(report: &[u8], route: u8) -> Option<&[u8]> {
    if report.len() < 3 || report[1] as usize > report.len() - 3 {
        return None;
    }
    if report[0] != 7 || report[1] > 61 || report[2] != route {
        return None;
    }
    Some(&report[3..3 + report[1] as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    /// Replays scripted input reports and records every output report.
    #[derive(Default)]
    struct ScriptedHid {
        inputs: RefCell<VecDeque<[u8; 64]>>,
        writes: Rc<RefCell<Vec<Vec<u8>>>>,
    }
    impl Hid for ScriptedHid {
        fn write_report(&self, report: &[u8]) -> Result<()> {
            self.writes.borrow_mut().push(report.to_vec());
            Ok(())
        }
        fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()> {
            if let Some(next) = self.inputs.borrow_mut().pop_front() {
                *report = next;
            }
            Ok(())
        }
        fn listen(&self) -> Result<Box<dyn InputStream>> {
            Err("not needed".into())
        }
    }

    fn report(route: u8, body: &[u8]) -> [u8; 64] {
        let mut report = [0u8; 64];
        report[..3].copy_from_slice(&[7, body.len() as u8, route]);
        report[3..3 + body.len()].copy_from_slice(body);
        report
    }

    fn link_with(inputs: &[[u8; 64]], route: u8) -> HidLink {
        let hid = ScriptedHid::default();
        hid.inputs.borrow_mut().extend(inputs.iter().copied());
        HidLink::new(Box::new(hid), 62, route)
    }

    #[test]
    fn replies_for_another_route_are_dropped_but_still_count_as_delivered() {
        let mix = [5, 0x5b, 6, 0, 0x83, 0x2c, 0, 0x0b, 0, 10];
        let mut link = link_with(&[report(0, &mix), report(0x80, &mix)], 0x80);
        let mut chunk = vec![1, 2, 3];
        assert!(
            link.receive(&mut chunk).unwrap(),
            "the cable report arrived"
        );
        assert!(chunk.is_empty(), "but it is not for the dongle");
        assert!(link.receive(&mut chunk).unwrap());
        assert_eq!(chunk, mix);
    }

    #[test]
    fn an_empty_mailbox_delivers_nothing() {
        let mut link = link_with(&[], 0x80);
        let mut chunk = vec![9];
        assert!(!link.receive(&mut chunk).unwrap());
        assert!(chunk.is_empty());
    }

    #[test]
    fn malformed_reports_yield_no_bytes() {
        let mut oversized = report(0x80, &[1, 2, 3]);
        oversized[1] = 62;
        let mut wrong_id = report(0x80, &[1, 2, 3]);
        wrong_id[0] = 6;
        let mut link = link_with(&[oversized, wrong_id], 0x80);
        let mut chunk = vec![];
        for _ in 0..2 {
            link.receive(&mut chunk).unwrap();
            assert!(chunk.is_empty());
        }
    }

    #[test]
    fn requests_are_wrapped_in_a_full_output_report() {
        let hid = ScriptedHid::default();
        let writes = hid.writes.clone();
        let mut link = HidLink::new(Box::new(hid), 62, 0x80);
        let frame = [5, 0x5a, 4, 0, 0x83, 0x2c, 0x0b, 0];
        link.send(&frame).unwrap();
        let sent = writes.borrow()[0].clone();
        assert_eq!(sent.len(), 62);
        assert_eq!(sent[..3], [6, 8, 0x80]);
        assert_eq!(sent[3..11], frame);
        assert!(sent[11..].iter().all(|b| *b == 0));
    }

    #[test]
    fn a_request_longer_than_the_report_is_refused_not_truncated() {
        let hid = ScriptedHid::default();
        let writes = hid.writes.clone();
        let mut link = HidLink::new(Box::new(hid), 62, 0);
        assert!(link.send(&[0u8; 60]).is_err());
        assert!(writes.borrow().is_empty());
        assert!(link.send(&[0u8; 59]).is_ok());
    }
}
