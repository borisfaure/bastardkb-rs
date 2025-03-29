//! Protocol between the halves.

use crate::log::warn;
#[cfg(feature = "log-protocol")]
use crate::log::{info, Debug2Format};
use crate::serde::{deserialize, serialize, Event, Message};
use crate::sid::{CircBuf, Sid};
use core::future;

enum ReceivedOrTick {
    Some(Message),
    Tick,
}

/// Hardware trait
pub trait Hardware {
    /// Send a message
    fn send(&mut self, msg: Message) -> impl future::Future<Output = ()> + Send;
    /// Receive a message
    fn receive(&mut self) -> impl future::Future<Output = ReceivedOrTick> + Send;

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
    /// Retransmit requests to send to the other side
    /// The key is the sid used to retransmit
    retransmit: CircBuf<Sid>,
    /// Last message received, if from a retransmit request
    last_msg: Option<Message>,

    /// Expecting sid
    next_rx_sid: Option<Sid>,
    /// Next sequence id to send
    next_tx_sid: Sid,

    /// Errors in the received sequence ids
    rx_errors: CircBuf<()>,

    /// Is master
    is_master: bool,

    /// Need to send a ping
    need_ping: bool,

    /// Hardware
    pub hw: W,
}

impl<W: Sized + Hardware> SideProtocol<W> {
    /// Create a new side protocol
    pub fn new(hw: W, name: &'static str, is_master: bool) -> Self {
        Self {
            name,
            sent: CircBuf::new(),
            retransmit: CircBuf::new(),
            next_rx_sid: None,
            next_tx_sid: Sid::default(),
            last_msg: None,
            rx_errors: CircBuf::new(),
            hw,
            is_master,
            need_ping: true,
        }
    }

    /// Send an event
    async fn send_event(&mut self, event: Event) {
        self.need_ping = false;
        let msg = serialize(event, self.next_tx_sid).unwrap();
        #[cfg(feature = "log-protocol")]
        info!(
            "[{}] Sending [{}] Event: {} (0x{:04x})",
            self.name,
            self.next_tx_sid,
            Debug2Format(&event),
            msg
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
    async fn on_invalid_sid(&mut self, msg: Message, expected: Sid, event: Event, sid: Sid) {
        warn!(
            "[{}] Invalid sid received: expected {}, got {} for event {:?}",
            self.name, expected, sid, event
        );
        if let Some(last_msg) = self.last_msg {
            if last_msg == msg {
                warn!("[{}] Last message was the same, skip it", self.name);
                return;
            }
        }
        let end = sid.next();
        for s in expected.iter(end) {
            self.rx_errors.insert(s, ());
        }
        self.send_event(Event::Retransmit(self.next_rx_sid.unwrap()))
            .await;
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
    async fn on_retransmit(&mut self, sid: Sid) {
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
            warn!("[{}] No event to retransmit for sid {}", self.name, sid);
            let msg = serialize(Event::Noop, sid).unwrap();
            self.hw.send(msg).await;
        }
    }

    /// On Ok event
    async fn handle_received_event(
        &mut self,
        msg: Message,
        event: Event,
        sid: Sid,
    ) -> Option<Event> {
        let mut to_process = None;
        match event {
            Event::Noop => {}
            Event::Ping => {
                self.on_ping(sid).await;
            }
            Event::Retransmit(_err) => {}
            Event::Ack(ack) => {
                self.on_ack(ack).await;
            }
            _ => {
                self.acknowledge(sid).await;
                to_process = Some(event);
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
        } else if let Some(next) = self.next_rx_sid {
            // Now that we have an event to process, since there are still
            // errors, we need to ask to retransmit the next event
            if self.rx_errors.get(next).is_some() {
                self.send_event(Event::Retransmit(next)).await;
            }
        }
        to_process
    }

    pub async fn run_once(&mut self) -> Option<Event> {
        let msg = self.hw.receive().await;
        match msg {
            ReceivedOrTick::Tick => {
                #[cfg(feature = "log-protocol")]
                info!("[{}] Sending Ping", self.name);
                if self.is_master && self.need_ping {
                    self.send_event(Event::Ping).await;
                }
                self.need_ping = true;
            }
            ReceivedOrTick::Some(msg) => match deserialize(msg) {
                Ok((event, sid)) => {
                    #[cfg(feature = "log-protocol")]
                    if let Some(next) = self.next_rx_sid {
                        info!(
                            "[{}] Received [{}/{}] Event: {}",
                            self.name,
                            sid,
                            next,
                            Debug2Format(&event)
                        );
                    } else {
                        info!(
                            "[{}] Received [{}] Event: {}",
                            self.name,
                            sid,
                            Debug2Format(&event)
                        );
                    }
                    if let Event::Retransmit(to_retransmit) = event {
                        self.on_retransmit(to_retransmit).await;
                    } else {
                        match (self.next_rx_sid, sid) {
                            (Some(expected), got) if expected == got => {
                                self.next_rx_sid = Some(expected.next());
                                if let Some(event) =
                                    self.handle_received_event(msg, event, sid).await
                                {
                                    return Some(event);
                                }
                            }
                            (None, _) => {
                                self.next_rx_sid = Some(sid.next());
                                if let Some(event) =
                                    self.handle_received_event(msg, event, sid).await
                                {
                                    return Some(event);
                                }
                            }
                            (Some(expected), _) => {
                                self.on_invalid_sid(msg, expected, event, sid).await;
                            }
                        }
                    }
                }
                Err(_) => {
                    warn!("[{}] Unable to deserialize event: 0x{:04x}", self.name, msg);
                    if let Some(sid) = self.next_rx_sid {
                        self.send_event(Event::Retransmit(sid)).await;
                    }
                }
            },
        }
        None
    }

    /// Receive a message
    pub async fn receive(&mut self) -> Event {
        loop {
            if let Some(event) = self.run_once().await {
                return event;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arraydeque::ArrayDeque;
    use log::{error, info};
    use lovely_env_logger;
    use tokio::sync::mpsc;

    const MAX_MSGS: usize = 64;

    struct MockHardware {
        msg_sent: usize,
        send_queue: ArrayDeque<Message, MAX_MSGS, arraydeque::behavior::Saturating>,
        to_rx: mpsc::Sender<ReceivedOrTick>,
        rx: mpsc::Receiver<ReceivedOrTick>,
        on_error: bool,
    }
    impl Hardware for MockHardware {
        fn send(&mut self, msg: Message) -> impl future::Future<Output = ()> + Send {
            self.msg_sent += 1;
            self.send_queue.push_front(msg).unwrap();
            async {}
        }
        async fn receive(&mut self) -> ReceivedOrTick {
            loop {
                if let Some(msg) = self.rx.recv().await {
                    return msg;
                }
            }
        }
        fn set_error_state(&mut self, error: bool) -> impl future::Future<Output = ()> + Send {
            self.on_error = error;
            async {}
        }
    }
    impl MockHardware {
        fn new() -> Self {
            let (to_rx, rx) = mpsc::channel(MAX_MSGS);
            Self {
                msg_sent: 0,
                send_queue: ArrayDeque::new(),
                to_rx,
                rx,
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
            if let Some(msg) = left.hw.send_queue.pop_back() {
                right.hw.to_rx.send(msg).await.unwrap();
                right.run_once().await;
            }
            if let Some(msg) = right.hw.send_queue.pop_back() {
                left.hw.to_rx.send(msg).await.unwrap();
                left.run_once().await;
            }
            if right.hw.send_queue.is_empty() && left.hw.send_queue.is_empty() {
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
        let mut right = SideProtocol::new(hw_right, "right", true);
        let mut left = SideProtocol::new(hw_left, "left", false);

        // Send a message from right to left
        right.send_event(Event::Ping).await;
        let msg = right.hw.send_queue.pop_back().unwrap();
        left.hw.to_rx.send(ReceivedOrTick::Some(msg)).await.unwrap();
        left.run_once().await;
        let msg = left.hw.send_queue.pop_back().unwrap();
        right
            .hw
            .to_rx
            .send(ReceivedOrTick::Some(msg))
            .await
            .unwrap();
        right.run_once().await;
        assert!(right.sent.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_sid() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right", true);
        let mut left = SideProtocol::new(hw_left, "left", false);

        // Send 4 pings from right to left but only receive the 4th one
        right.send_event(Event::Ping).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::Ping).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    #[tokio::test]
    async fn test_retransmit_simple() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right", true);
        let mut left = SideProtocol::new(hw_left, "left", false);

        // Send 2 pings from right to left but corrupt the 3 next ones,
        // followed by a correct one
        right.send_event(Event::SeedRng(0)).await;
        right.send_event(Event::SeedRng(1)).await;
        right.send_event(Event::SeedRng(2)).await;
        right.send_event(Event::SeedRng(3)).await;
        right.send_event(Event::SeedRng(4)).await;
        let mut bad = [0u32, 0, 0];
        for i in 0..3 {
            let mut msg = right.hw.send_queue.pop_front().unwrap();
            msg ^= 0x1234;
            bad[i] = msg;
        }
        for i in 0..3 {
            right.hw.send_queue.push_front(bad[i]).unwrap();
        }
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    #[tokio::test]
    async fn test_retransmit_both_sides() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right");
        let mut left = SideProtocol::new(hw_left, "left");

        right.next_rx_sid = Sid::new(0);
        right.next_tx_sid = Sid::new(0);
        left.next_rx_sid = Sid::new(5);
        left.next_tx_sid = Sid::new(10);
        right.send_event(Event::Ping).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
        info!("Right: {:?}", right.hw.msg_sent);
        info!("Left: {:?}", right.hw.msg_sent);
        assert!(is_stable(&right));
        assert!(is_stable(&left));
        right.send_event(Event::Press(0, 0)).await;
        left.send_event(Event::Press(3, 3)).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left).await;
    }
}
