//! Protocol between the halves.

// The protocol is a simple state machine that sends and receives messages
// between the two sides.  Each side has a sequence id (SID) that is
// incremented for each message sent.  The other side must Acknowledge
// the message by sending back the same SID.
//
// There are few error cases when new messages are queued until the
// error is resolved.
// 1. If a message is received with an invalid SID, a retransmit is sent for
//    the expected SID.
//    Once the expected SID is received, the next SID is incremented and a
//    Retransmit is sent for the next SID.
//    This ends when the other side sends a Noop message.
// 2. If a message cannot be deserialized, a retransmit is sent for the
//    expected SID.
//    Once the expected SID is received, the next SID is incremented and a
//    Retransmit is sent for the next SID.
//    This ends when the other side sends a Noop message.
// 3. A Retransmit message is received. This means the other side is on error.
//    To avoid a message storm, only the expected SID is retransmitted.
//    The error state is cleared when receiving a Retransmit message for an
//    event that was not sent, or already acknowledged.
// Those cases can occur simultaneously on both sides.
// When such errors occur, no ping is sent until the error is resolved.

use crate::log::{error, warn};
#[cfg(feature = "log-protocol")]
use crate::log::{info, Debug2Format};
use crate::serde::{deserialize, serialize, Event, Message};
use crate::sid::{CircBuf, Sid};
use arraydeque::ArrayDeque;
use core::future;

/// Received or tick
#[allow(dead_code)]
pub enum ReceivedOrTick {
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

const MAX_QUEUED_EVENTS: usize = 64;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SideProtocol<W: Sized + Hardware> {
    // Name
    name: &'static str,

    /// Events sent to the other side,
    /// waiting for an ACK
    sent: CircBuf<Message>,

    /// Events queued to be sent when retransmit is complete
    queued_events: ArrayDeque<Event, MAX_QUEUED_EVENTS, arraydeque::behavior::Saturating>,

    /// Expecting sid to be received
    /// None on startup
    next_rx_sid: Option<Sid>,
    /// Next sequence id to send
    next_tx_sid: Sid,

    /// Is master
    is_master: bool,

    /// Need to send a ping
    need_ping: bool,

    /// Last message received
    last_msg: Option<Message>,

    /// Retransmit on going: this side asked for a retransmit
    retransmit_on_going: bool,
    /// Remote retransmit on going: the other side asked for a retransmit
    remote_retransmit_on_going: bool,
    /// Retransmit reverse index
    retransmit_rev_index: CircBuf<Sid>,

    /// Hardware
    pub hw: W,
}

impl<W: Sized + Hardware> SideProtocol<W> {
    /// Create a new side protocol
    pub fn new(hw: W, name: &'static str, is_master: bool) -> Self {
        Self {
            name,
            sent: CircBuf::new(),
            queued_events: ArrayDeque::new(),
            next_rx_sid: None,
            next_tx_sid: Sid::default(),
            hw,
            is_master,
            retransmit_on_going: false,
            remote_retransmit_on_going: false,
            retransmit_rev_index: CircBuf::new(),
            need_ping: true,
            last_msg: None,
        }
    }

    /// Send an event
    async fn send_event(&mut self, event: Event) {
        self.need_ping = false;
        let msg = serialize(event, self.next_tx_sid).unwrap();
        #[cfg(feature = "log-protocol")]
        info!(
            "[{}] Sending [Sid#{}] Event: {} (0x{:04x})",
            self.name,
            self.next_tx_sid,
            Debug2Format(&event),
            msg
        );
        self.hw.send(msg).await;
        self.sent.insert(self.next_tx_sid, msg);
        if let Event::Retransmit(re) = event {
            // Populate the reverse index
            self.retransmit_rev_index.insert(re, self.next_tx_sid);
        }

        self.next_tx_sid = self.next_tx_sid.next();
    }

    /// Check if we're in error mode
    fn is_on_error(&self) -> bool {
        self.retransmit_on_going || self.remote_retransmit_on_going
    }

    /// Queue an event to be sent
    pub async fn queue_event(&mut self, event: Event) {
        if self.is_on_error() {
            // If we're in error mode, queue the event instead of sending it immediately
            #[cfg(feature = "log-protocol")]
            info!(
                "[{}] Queuing event while in error mode: {}",
                self.name,
                Debug2Format(&event)
            );
            if let Err(e) = self.queued_events.push_front(event) {
                warn!("[{}] Unable to queue event: {}", self.name, e);
            }
        } else {
            // If we're not in error mode, send the event immediately
            #[cfg(feature = "log-protocol")]
            info!(
                "[{}] Sending event: {} (no queue)",
                self.name,
                Debug2Format(&event)
            );
            self.send_event(event).await;
        }
    }

    /// Send a Retransmit event
    async fn send_retransmit(&mut self, sid: Sid) {
        self.retransmit_on_going = true;
        // Mark as on error
        self.hw.set_error_state(self.is_on_error()).await;

        #[cfg(feature = "log-protocol")]
        info!("[{}] Sending Retransmit [{}]", self.name, sid);
        self.send_event(Event::Retransmit(sid)).await;
    }

    /// On invalid sequence id
    async fn on_invalid_sid(&mut self, msg: Message, expected: Sid, event: Event, sid: Sid) {
        error!(
            "[{}] Invalid sid received: expected {}, got {} for event {:?}",
            self.name, expected, sid, event
        );
        if let Some(last_msg) = self.last_msg {
            if last_msg == msg {
                warn!("[{}] Last message was the same, skip it", self.name);
                return;
            }
        }

        self.send_retransmit(expected).await;
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

        // If we were in error mode and all messages have been acknowledged,
        // we can exit error mode and send any queued events
        if self.retransmit_on_going && self.sent.is_empty() {
            #[cfg(feature = "log-protocol")]
            info!(
                "[{}] Exiting error mode on retransmit on going, sending queued events",
                self.name
            );

            self.retransmit_on_going = false;
            self.hw.set_error_state(self.is_on_error()).await;

            // Send all queued events
            while let Some(event) = self.queued_events.pop_back() {
                self.send_event(event).await;
            }
        }
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
        // Mark as on error
        self.remote_retransmit_on_going = true;
        self.hw.set_error_state(self.is_on_error()).await;
        #[cfg(feature = "log-protocol")]
        error!("[{}] Received Retransmit [{}]", self.name, sid,);

        if let Some(msg) = self.sent.get(sid) {
            #[cfg(feature = "log-protocol")]
            info!(
                "[{}] retransmitting [{}] event: {}",
                self.name,
                sid,
                Debug2Format(&deserialize(msg).unwrap().0)
            );
            self.hw.send(msg).await;
        } else {
            #[cfg(feature = "log-protocol")]
            info!("[{}] retransmitting [{}] event: Noop", self.name, sid);
            let msg = serialize(Event::Noop, sid).unwrap();
            self.remote_retransmit_on_going = false;
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
            Event::Noop => {
                #[cfg(feature = "log-protocol")]
                info!("[{}] Received Noop", self.name);
                // If we were in error mode and received a Noop, we can exit
                // error mode
                if self.retransmit_on_going {
                    #[cfg(feature = "log-protocol")]
                    info!(
                        "[{}] Exiting error mode about ongoing retransmit",
                        self.name
                    );
                    self.retransmit_on_going = false;
                    self.hw.set_error_state(self.is_on_error()).await;
                }
            }
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

        // Store the last message for duplicate detection
        self.last_msg = Some(msg);

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
                    }
                    if let Some(event_to_return) = match (self.next_rx_sid, sid) {
                        (Some(expected), got) if expected == got => {
                            let mut event_to_return = None;
                            // We received the expected message
                            if self.retransmit_on_going {
                                // If we're in retransmit mode, and got the event, consider the
                                // Retransmit to be acknowledged
                                #[cfg(feature = "log-protocol")]
                                info!(
                                    "[{}] while in retransmit mode, received event: {}, retransmit acknowledged",
                                    self.name,
                                    Debug2Format(&event)
                                );
                                if let Some(re) = self.retransmit_rev_index.take(sid) {
                                    self.sent.remove(re);
                                }
                            }
                            self.next_rx_sid = Some(expected.next());
                            if let Some(event) = self.handle_received_event(msg, event, sid).await {
                                event_to_return = Some(event);
                            }
                            #[cfg(feature = "log-protocol")]
                            info!(
                                "[{}] retransmit on going: {}",
                                self.name, self.retransmit_on_going
                            );
                            if self.retransmit_on_going {
                                // We were in error mode and received the
                                // expected message. Ask for the next
                                // message
                                if let Some(next) = self.next_rx_sid {
                                    self.send_retransmit(next).await;
                                }
                            }
                            event_to_return
                        }
                        (None, _) => {
                            // No expected message, this is the first message
                            self.next_rx_sid = Some(sid.next());
                            if let Some(event) = self.handle_received_event(msg, event, sid).await {
                                Some(event)
                            } else {
                                None
                            }
                        }
                        (Some(expected), _) => {
                            self.on_invalid_sid(msg, expected, event, sid).await;
                            None
                        }
                    } {
                        return Some(event_to_return);
                    };
                }
                Err(_) => {
                    warn!("[{}] Unable to deserialize event: 0x{:04x}", self.name, msg);
                    if let Some(next) = self.next_rx_sid {
                        self.send_retransmit(next).await;
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

    /// One exchange of messages between the two sides
    async fn communicate_once(
        right: &mut SideProtocol<MockHardware>,
        left: &mut SideProtocol<MockHardware>,
    ) {
        if let Some(msg) = left.hw.send_queue.pop_back() {
            right
                .hw
                .to_rx
                .send(ReceivedOrTick::Some(msg))
                .await
                .unwrap();
        }
        if !right.hw.rx.is_empty() {
            right.run_once().await;
        }
        if let Some(msg) = right.hw.send_queue.pop_back() {
            left.hw.to_rx.send(ReceivedOrTick::Some(msg)).await.unwrap();
        }
        if !left.hw.rx.is_empty() {
            left.run_once().await;
        }
        info!(
            "QUEUES: right rx:{} send:{}/{} left rx:{} send:{}/{}",
            right.hw.rx.len(),
            right.hw.send_queue.len(),
            right.hw.msg_sent,
            left.hw.rx.len(),
            left.hw.send_queue.len(),
            left.hw.msg_sent
        );
    }

    /// Communicate between the two sides
    async fn communicate(
        right: &mut SideProtocol<MockHardware>,
        left: &mut SideProtocol<MockHardware>,
        loop_nb: usize,
    ) {
        for _ in 0..loop_nb {
            communicate_once(right, left).await;
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

        // Both sides are synced
        right.next_rx_sid = Some(Sid::new(0));
        right.next_tx_sid = Sid::new(0);
        left.next_rx_sid = Some(Sid::new(0));
        left.next_tx_sid = Sid::new(0);

        // Send 4 SeedRng from right to left but only receive the 4th one
        right.send_event(Event::SeedRng(0)).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::SeedRng(1)).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::SeedRng(2)).await;
        right.hw.send_queue.pop_back().unwrap();
        right.send_event(Event::SeedRng(3)).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left, 10).await;
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
        communicate(&mut right, &mut left, 40).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    #[tokio::test]
    /// Test the startup of the protocol when the two sides are not synced.
    /// The right side is the master and the left side is the slave.
    async fn test_startup_unsynced() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right", true);
        let mut left = SideProtocol::new(hw_left, "left", false);

        right.next_rx_sid = Some(Sid::new(30));
        right.next_tx_sid = Sid::new(2);
        left.next_rx_sid = None;
        left.next_tx_sid = Sid::new(0);
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left, 10).await;
        info!("Right: {:?}", right.hw.msg_sent);
        info!("Left: {:?}", right.hw.msg_sent);
        assert!(is_stable(&right));
        assert!(is_stable(&left));
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        // Force a ping to be sent due to 2 consecutive ticks with no comm
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        // Let it commmunicate and stabilize
        communicate_once(&mut right, &mut left).await;
        communicate(&mut right, &mut left, 10).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    #[tokio::test]
    async fn test_retransmit_both_sides() {
        let _ = lovely_env_logger::try_init_default();
        let hw_right = MockHardware::new();
        let hw_left = MockHardware::new();
        let mut right = SideProtocol::new(hw_right, "right", true);
        let mut left = SideProtocol::new(hw_left, "left", false);

        right.next_rx_sid = Some(Sid::new(30));
        right.next_tx_sid = Sid::new(0);
        left.next_rx_sid = Some(Sid::new(25));
        left.next_tx_sid = Sid::new(10);
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left, 50).await;
        info!("Right: {:?}", right.hw.msg_sent);
        info!("Left: {:?}", right.hw.msg_sent);
        assert!(is_stable(&right));
        assert!(is_stable(&left));

        // Force a ping to be sent due to 2 consecutive ticks with no comm
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();
        right.hw.to_rx.send(ReceivedOrTick::Tick).await.unwrap();

        left.send_event(Event::Press(3, 3)).await;
        // Let it commmunicate and stabilize
        communicate(&mut right, &mut left, 50).await;
        assert!(is_stable(&right));
        assert!(is_stable(&left));
    }

    // TODO Test when a side got a corrupted message and sends a retransmit
    // that is also corrupted
}
