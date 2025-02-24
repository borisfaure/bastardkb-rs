//! Protocol between the halves.

#[cfg(feature = "log-protocol")]
use crate::log::*;
use crate::serde::{deserialize, serialize, Event, Message};
use crate::sid::{CircBuf, Sid};
use core::future;

/// Hardware trait
pub trait Hardware {
    /// Send a message
    fn send(&mut self, msg: Message) -> impl future::Future<Output = ()> + Send;
    /// Receive a message
    fn receive(&mut self) -> impl future::Future<Output = Message> + Send;
    /// Wait a bit
    fn wait_a_bit(&mut self) -> impl future::Future<Output = ()> + Send;

    /// Process an event
    fn process_event(&mut self, event: Event) -> impl future::Future<Output = ()> + Send;

    /// Set error state
    fn set_error_state(&mut self, error: bool) -> impl future::Future<Output = ()> + Send;
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SideProtocol<W: Sized + Hardware> {
    // Name
    name: &'static str,

    /// Events sent to the other side
    sent: CircBuf<Message>,
    /// Retransmit requests
    /// The key is the sid used to retransmit
    retransmit: CircBuf<Sid>,
    /// Last message received, if from a retransmit request
    last_msg: Option<Message>,

    /// Expecting sid
    next_rx_sid: Sid,
    /// Next sequence id to send
    next_tx_sid: Sid,

    /// Errors in the received sequence ids
    rx_errors: CircBuf<()>,

    /// Hardware
    pub hw: W,
}

impl<W: Sized + Hardware> SideProtocol<W> {
    /// Create a new side protocol
    pub fn new(hw: W, name: &'static str) -> Self {
        Self {
            name,
            sent: CircBuf::new(),
            retransmit: CircBuf::new(),
            next_rx_sid: Sid::default(),
            next_tx_sid: Sid::default(),
            last_msg: None,
            rx_errors: CircBuf::new(),
            hw,
        }
    }

    /// Send an event
    async fn send_event(&mut self, event: Event) {
        let msg = serialize(event, self.next_tx_sid).unwrap();
        #[cfg(feature = "log-protocol")]
        info!(
            "[{}] Sending [{}] Event: {}",
            self.name,
            self.next_tx_sid,
            Debug2Format(&event)
        );
        self.hw.send(msg).await;
        self.sent.insert(self.next_tx_sid, msg);
        if let Event::Retransmit(sid) = event {
            self.retransmit.insert(self.next_tx_sid, sid);
        }
        self.next_tx_sid.next();
    }

    /// Queue an event to be sent
    pub async fn queue_event(&mut self, event: Event) {
        // TODO: really queue events when retransmits are ongoing
        self.send_event(event).await;
    }

    /// On invalid sequence id
    async fn on_invalid_sid(&mut self, msg: Message, sid: Sid) {
        #[cfg(feature = "log-protocol")]
        warn!(
            "[{}] Invalid sid received: expected {}, got {}",
            self.name, self.next_rx_sid, sid
        );
        if let Some(last_msg) = self.last_msg {
            if last_msg == msg {
                #[cfg(feature = "log-protocol")]
                warn!("[{}] Last message was the same, skip it", self.name);
                return;
            }
        }
        let mut next = sid;
        next.next();
        for s in self.next_rx_sid.iter(next) {
            self.rx_errors.insert(s, ());
        }
        self.hw.wait_a_bit().await;
        self.send_event(Event::Retransmit(self.next_rx_sid)).await;
    }

    //. Send an ACK for the given sequence id
    async fn acknowledge(&mut self, sid: Sid) {
        #[cfg(feature = "log-protocol")]
        info!("[{}] Sending ACK for sid {}", self.name, sid);
        self.send_event(Event::Ack(sid)).await;
    }

    /// Received an ACK for the given sequence id
    /// This means the other side has received this event
    async fn on_ack(&mut self, sid: Sid) {
        self.sent.remove(sid);
    }

    /// On Ping event: respond with a ack
    async fn on_ping(&mut self, sid: Sid) {
        #[cfg(feature = "log-protocol")]
        info!("[{}] Acknowledge Ping of Sid {}", self.name, sid);
        self.acknowledge(sid).await;
    }

    /// On Retransmit event
    /// The other side is asking for a retransmit
    /// Send the event again with the same sequence id
    async fn on_retansmit(&mut self, sid: Sid) {
        if let Some(msg) = self.sent.get(sid) {
            #[cfg(feature = "log-protocol")]
            info!(
                "[{}] Retransmitting [{}] Event: {}",
                self.name,
                sid,
                Debug2Format(&deserialize(msg).unwrap().0)
            );
            self.hw.send(msg).await;
        } else {
            #[cfg(feature = "log-protocol")]
            warn!("[{}] No event to retransmit for sid {}", self.name, sid);
        }
    }

    /// On Ok event
    async fn handle_received_event(&mut self, msg: Message, event: Event, sid: Sid) {
        match event {
            Event::Noop => {}
            Event::Ping => {
                self.on_ping(sid).await;
            }
            Event::Retransmit(err) => {
                self.on_retansmit(err).await;
            }
            Event::Ack(ack) => {
                self.on_ack(ack).await;
            }
            _ => {
                self.acknowledge(sid).await;
                self.hw.process_event(event).await;
            }
        }
        // If this Sequence Id had an error, we can clear it now
        if self.rx_errors.get(sid).is_some() {
            self.last_msg = Some(msg);
            self.rx_errors.remove(sid);
            /* Remove related retransmit requests */
            for idx in Sid::new(0).iter(Sid::new(0)) {
                if let Some(s) = self.retransmit.get(idx) {
                    if s == sid {
                        #[cfg(feature = "log-protocol")]
                        info!(
                            "[{}] Removing retransmit request for sid {} at {}",
                            self.name, sid, idx
                        );
                        self.sent.remove(idx);
                        self.retransmit.remove(idx);
                    }
                }
            }
        } else {
            self.last_msg = None;
        }
        if self.rx_errors.is_empty() {
            self.hw.set_error_state(false).await;
        } else {
            // Now that we have processed the event, since there are still
            // errors, we need to ask to retransmit the next event
            if self.rx_errors.get(self.next_rx_sid).is_some() {
                self.send_event(Event::Retransmit(self.next_rx_sid)).await;
            }
        }
    }

    /// Receive a message
    pub async fn on_receive(&mut self, msg: Message) {
        match deserialize(msg) {
            Ok((event, sid)) => {
                #[cfg(feature = "log-protocol")]
                info!(
                    "[{}] Received [{}/{}] Event: {}",
                    self.name,
                    sid,
                    self.next_rx_sid,
                    Debug2Format(&event)
                );
                if self.next_rx_sid != sid {
                    self.on_invalid_sid(msg, sid).await;
                } else {
                    self.next_rx_sid.next();
                    self.handle_received_event(msg, event, sid).await;
                }
            }
            Err(_) => {
                #[cfg(feature = "log-protocol")]
                warn!("[{}] Unable to deserialize event: 0x{:04x}", self.name, msg);
                self.hw.wait_a_bit().await;
                self.send_event(Event::Retransmit(self.next_rx_sid)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arraydeque::ArrayDeque;
    use log::error;
    use lovely_env_logger;

    struct MockHardware {
        msg_sent: usize,
        queue: ArrayDeque<Message, 64, arraydeque::behavior::Saturating>,
        on_error: bool,
    }
    impl Hardware for MockHardware {
        fn send(&mut self, msg: Message) -> impl future::Future<Output = ()> + Send {
            self.msg_sent += 1;
            self.queue.push_front(msg).unwrap();
            async {}
        }
        fn receive(&mut self) -> impl future::Future<Output = Message> + Send {
            async { 0x1234 }
        }
        fn wait_a_bit(&mut self) -> impl future::Future<Output = ()> + Send {
            async {}
        }
        fn process_event(&mut self, _event: Event) -> impl future::Future<Output = ()> + Send {
            async {}
        }
        fn set_error_state(&mut self, error: bool) -> impl future::Future<Output = ()> + Send {
            self.on_error = error;
            async {}
        }
    }
    impl MockHardware {
        fn new() -> Self {
            Self {
                msg_sent: 0,
                queue: ArrayDeque::new(),
                on_error: false,
            }
        }
    }

    /// Communicate between the two sides
    async fn communicate(
        right: &mut SideProtocol<MockHardware>,
        left: &mut SideProtocol<MockHardware>,
    ) {
        loop {
            if let Some(msg) = left.hw.queue.pop_back() {
                right.on_receive(msg).await;
            }
            if let Some(msg) = right.hw.queue.pop_back() {
                left.on_receive(msg).await;
            }
            if right.hw.queue.is_empty() && left.hw.queue.is_empty() {
                break;
            }
        }
    }

    /// Is side stable
    fn is_stable(side: &SideProtocol<MockHardware>) -> bool {
        for i in Sid::new(0).iter(Sid::new(0)) {
            if let Some(msg) = side.sent.get(i) {
                let (event, _) = deserialize(msg).unwrap();
                if !event.is_ack() {
                    error!("[{}/{}] Not acked: {:?}", side.name, i, event);
                    return false;
                }
            }
        }
        true
    }

    #[tokio::test]
    async fn test_protocol_synced() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right");
        let mut left = SideProtocol::new(hw_left, "left");

        // Send a message from right to left
        right.send_event(Event::Ping).await;
        let msg = right.hw.queue.pop_back().unwrap();
        left.on_receive(msg).await;
        let msg = left.hw.queue.pop_back().unwrap();
        right.on_receive(msg).await;
        assert!(right.sent.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_sid() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right");
        let mut left = SideProtocol::new(hw_left, "left");

        // Send 4 pings from right to left but only receive the 4th one
        right.send_event(Event::Ping).await;
        right.hw.queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        right.hw.queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        right.hw.queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    #[tokio::test]
    async fn test_retransmit() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right");
        let mut left = SideProtocol::new(hw_left, "left");

        // Send 2 pings from right to left but corrupt the 3 next ones,
        // followed by a correct one
        right.send_event(Event::Ping).await;
        right.send_event(Event::Ping).await;
        right.send_event(Event::Ping).await;
        right.send_event(Event::Ping).await;
        right.send_event(Event::Ping).await;
        let mut bad = [0u32, 0, 0];
        for i in 0..3 {
            let mut msg = right.hw.queue.pop_front().unwrap();
            msg ^= 0x1234;
            bad[i] = msg;
        }
        for i in 0..3 {
            right.hw.queue.push_front(bad[i]).unwrap();
        }
        right.send_event(Event::Ping).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }
}
