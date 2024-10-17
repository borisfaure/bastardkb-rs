use crate::layout::LAYOUT_CHANNEL;
use embassy_futures::select::{select, Either};
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_1, PIN_29, PIO1};
use embassy_rp::pio::{self, Direction, FifoJoin, ShiftDirection, StateMachine};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use fixed::{traits::ToFixed, types::U56F8};
use keyberon::layout::Event as KBEvent;
use utils::serde::{deserialize, serialize, Event};

pub const USART_SPEED: u64 = 460800;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 64;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<CriticalSectionRawMutex, Event, NB_EVENTS> = Channel::new();

const TX: usize = 0;
const RX: usize = 1;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

pub async fn full_duplex_comm<'a>(
    mut pio_common: PioCommon<'a>,
    sm0: SmTx<'a>,
    sm1: SmRx<'a>,
    gpio_pin_1: PIN_1,
    gpio_pin_29: PIN_29,
    status_led: &mut Output<'static>,
    is_right: bool,
) {
    let (mut pin_rx, mut pin_tx) = if is_right {
        (
            pio_common.make_pio_pin(gpio_pin_29),
            pio_common.make_pio_pin(gpio_pin_1),
        )
    } else {
        (
            pio_common.make_pio_pin(gpio_pin_1),
            pio_common.make_pio_pin(gpio_pin_29),
        )
    };

    let mut tx_sm = task_tx(&mut pio_common, sm0, &mut pin_tx);
    let mut rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    loop {
        match select(SIDE_CHANNEL.receive(), rx_sm.rx().wait_pull()).await {
            Either::First(event) => {
                let b = serialize(event);
                status_led.set_low();
                tx_sm.tx().wait_push(b).await;
                status_led.set_high();
            }
            Either::Second(x) => {
                status_led.set_low();
                match deserialize(x) {
                    Ok(event) => {
                        defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
                        match event {
                            Event::Press(i, j) => {
                                LAYOUT_CHANNEL.send(KBEvent::Press(i, j)).await;
                            }
                            Event::Release(i, j) => {
                                LAYOUT_CHANNEL.send(KBEvent::Release(i, j)).await;
                            }
                        }
                    }
                    Err(_) => {
                        defmt::warn!("Invalid event received: {:?}", x);
                    }
                }
                status_led.set_high();
            }
        }
    }
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clk_sys_freq()) / (8 * USART_SPEED)).to_fixed()
}

fn task_tx<'a>(
    common: &mut PioCommon<'a>,
    mut sm_tx: SmTx<'a>,
    tx_pin: &mut PioPin<'a>,
) -> SmTx<'a> {
    let tx_prog = pio_proc::pio_file!("src/tx.pio");
    sm_tx.set_pins(Level::High, &[tx_pin]);
    sm_tx.set_pin_dirs(Direction::Out, &[tx_pin]);

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.set_out_pins(&[tx_pin]);
    cfg.set_set_pins(&[tx_pin]);
    cfg.use_program(&common.load_program(&tx_prog.program), &[]);
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.fifo_join = FifoJoin::TxOnly;
    cfg.clock_divider = pio_freq();
    sm_tx.set_config(&cfg);
    sm_tx.set_enable(true);

    sm_tx
}

fn task_rx<'a>(
    common: &mut PioCommon<'a>,
    mut sm_rx: SmRx<'a>,
    rx_pin: &mut PioPin<'a>,
) -> SmRx<'a> {
    let rx_prog = pio_proc::pio_file!("src/rx.pio");

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&rx_prog.program), &[]);

    sm_rx.set_pins(Level::High, &[rx_pin]);
    cfg.set_in_pins(&[rx_pin]);
    cfg.set_jmp_pin(rx_pin);
    sm_rx.set_pin_dirs(Direction::In, &[rx_pin]);

    cfg.clock_divider = pio_freq();
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.fifo_join = FifoJoin::RxOnly;
    sm_rx.set_config(&cfg);
    sm_rx.set_enable(true);

    sm_rx
}
