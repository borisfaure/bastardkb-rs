//! Protocol between the halves.

use crate::log::*;
use crate::serde::{deserialize, serialize, Event, Message};
use crate::sid::{CircBuf, Sid};
use core::future;

/// Hardware trait
pub trait Hardware {
    /// Send a message
    fn send(&mut self, msg: Message) -> impl future::Future<Output = ()> + Send;
    /// Wait a bit
    fn wait_a_bit(&mut self) -> impl future::Future<Output = ()> + Send;

    /// Process an event
    fn process_event(&mut self, event: Event) -> impl future::Future<Output = ()> + Send;
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SideProtocol<W: Sized + Hardware> {
    /// Events sent to the other side
    sent: CircBuf<Message>,

    /// Expecting sid
    next_rx_sid: Sid,
    /// Next sequence id to send
    next_tx_sid: Sid,

    /// Hardware
    hw: W,
}

impl<W: Sized + Hardware> SideProtocol<W> {
    /// Create a new side protocol
    pub fn new(hw: W) -> Self {
        Self {
            sent: CircBuf::new(),
            next_rx_sid: Sid::default(),
            next_tx_sid: Sid::default(),
            hw,
        }
    }

    /// Send an event
    async fn send_event(&mut self, event: Event) {
        let msg = serialize(event, self.next_tx_sid).unwrap();
        info!(
            "Sending [{}/{}] Event: {}",
            self.next_tx_sid,
            self.next_rx_sid,
            Debug2Format(&event)
        );
        self.hw.send(msg).await;
        self.sent.insert(self.next_tx_sid, msg);
        self.next_tx_sid.next();
    }

    /// Queue an event to be sent
    pub async fn queue_event(&mut self, _event: Event) {}

    /// Receive a message
    pub async fn receive(&mut self, msg: Message) {
        match deserialize(msg) {
            Ok((event, sid)) => {
                info!(
                    "Received [{}/{}] Event: {}",
                    sid,
                    self.next_rx_sid,
                    Debug2Format(&event)
                );
                //if self.next_rx_sid != sid {
                //    self.on_invalid_sid(sid).await;
                //} else {
                //    self.handle_received_event(event, sid).await;
                //}
            }
            Err(_) => {
                warn!("Unable to deserialize event: 0x{:04x}", msg);
                self.hw.wait_a_bit().await;
                self.send_event(Event::Retransmit(self.next_rx_sid)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_foo() {}
}
