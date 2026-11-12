//! The Bluetooth channel as a [`Link`] for the sound protocol.
use super::{check_permitted, Channel, Event};
use crate::device::link::Link;
use std::time::Duration;

type Result<T> = std::result::Result<T, String>;

/// Frames over the headset's RFCOMM control channel. Unlike the HID link there is
/// no wrapper: the bytes written and read are the protocol frames themselves.
pub(in crate::device) struct BluetoothLink {
    channel: Channel,
}

impl BluetoothLink {
    pub(in crate::device) fn new(channel: Channel) -> Self {
        Self { channel }
    }
}

impl Link for BluetoothLink {
    fn send(&mut self, frame: &[u8]) -> Result<()> {
        // The channel enforces the allow-list too; checking here keeps the
        // refusal independent of that and testable without a headset.
        check_permitted(frame)?;
        self.channel.write(frame)
    }

    fn receive(&mut self, chunk: &mut Vec<u8>) -> Result<bool> {
        chunk.clear();
        loop {
            match self.channel.recv(Duration::ZERO) {
                Some(Event::Data(bytes)) => chunk.extend_from_slice(&bytes),
                Some(Event::Closed) => return Err("The Bluetooth connection closed.".into()),
                Some(Event::Opened { .. }) => {}
                None => return Ok(!chunk.is_empty()),
            }
        }
    }
}
