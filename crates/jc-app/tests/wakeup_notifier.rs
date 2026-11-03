//! Observable notification transport, independent of any native window/backend.
use jc_app::Wakeup;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(3);

#[test]
fn notifier_can_read_its_own_ticket_and_observes_monotonic_sequential_signals() {
    let holder = Arc::new(Mutex::new(None::<Wakeup>));
    let weak = Arc::downgrade(&holder);
    let (observed, observations) = mpsc::channel();
    let wake = Wakeup::with_notifier(move || {
        let holder = weak.upgrade().expect("test wake owner alive");
        let ticket = holder.lock().unwrap().as_ref().unwrap().ticket();
        observed.send(ticket).unwrap();
    });
    *holder.lock().unwrap() = Some(wake.clone());
    let initial = wake.ticket();
    let worker_wake = wake.clone();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        worker_wake.signal();
        worker_wake.signal();
        done.send(()).unwrap();
    });
    // A callback incorrectly invoked under the ticket mutex blocks only this
    // helper. Bound the observation before joining so a regression fails the
    // test instead of hanging the entire test process.
    assert_eq!(
        observations
            .recv_timeout(DEADLINE)
            .expect("reentrant notifier deadlocked"),
        initial.wrapping_add(1)
    );
    assert_eq!(
        observations.recv_timeout(DEADLINE).unwrap(),
        initial.wrapping_add(2)
    );
    finished.recv_timeout(DEADLINE).unwrap();
    worker.join().unwrap();
    assert_eq!(wake.ticket(), initial.wrapping_add(2));
    assert!(!wake.take_notifier_failure());
    holder.lock().unwrap().take();
}

#[test]
fn callback_panic_preserves_ticket_waiter_delivery_and_a_consumable_failure_latch() {
    let wake = Wakeup::with_notifier(|| panic!("injected notifier failure"));
    let initial = wake.ticket();
    let waiting = wake.clone();
    let (ready, started) = mpsc::channel();
    let (awoke, delivered) = mpsc::channel();
    let waiter = thread::spawn(move || {
        ready.send(()).unwrap();
        waiting.wait(initial, None);
        awoke.send(waiting.ticket()).unwrap();
    });
    started.recv_timeout(DEADLINE).unwrap();
    let signaling = wake.clone();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        signaling.signal();
        done.send(signaling.ticket()).unwrap();
    });
    assert_eq!(
        finished
            .recv_timeout(DEADLINE)
            .expect("notifier panic escaped signal"),
        initial.wrapping_add(1)
    );
    assert_eq!(
        delivered
            .recv_timeout(DEADLINE)
            .expect("notifier panic lost condvar delivery"),
        initial.wrapping_add(1)
    );
    worker.join().unwrap();
    waiter.join().unwrap();
    assert!(wake.take_notifier_failure());
    assert!(!wake.clone().take_notifier_failure());
    assert_eq!(wake.ticket(), initial.wrapping_add(1));
}

#[test]
fn clones_share_notifier_and_ticket_while_timeout_and_old_ticket_waits_do_not_signal() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let wake = Wakeup::with_notifier(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let clone = wake.clone();
    let initial = wake.ticket();
    wake.wait(initial, Some(Duration::ZERO));
    assert_eq!(clone.ticket(), initial);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let waiting = clone.clone();
    let (ready, started) = mpsc::channel();
    let (awoke, delivered) = mpsc::channel();
    let waiter = thread::spawn(move || {
        ready.send(()).unwrap();
        waiting.wait(initial, None);
        awoke.send(waiting.ticket()).unwrap();
    });
    started.recv_timeout(DEADLINE).unwrap();
    clone.signal();
    assert_eq!(
        delivered.recv_timeout(DEADLINE).unwrap(),
        initial.wrapping_add(1)
    );
    waiter.join().unwrap();
    assert_eq!(wake.ticket(), clone.ticket());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // A previous ticket must also return immediately with timeout=None. Keep
    // this on a bounded helper so an accidental lost-ticket wait cannot hang.
    let waiting = wake.clone();
    let (done, finished) = mpsc::channel();
    let waiter = thread::spawn(move || {
        waiting.wait(initial, None);
        done.send(()).unwrap();
    });
    finished.recv_timeout(DEADLINE).unwrap();
    waiter.join().unwrap();
    wake.signal();
    assert_eq!(clone.ticket(), initial.wrapping_add(2));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!clone.take_notifier_failure());
    let plain = Wakeup::default();
    plain.signal();
    assert_eq!(plain.ticket(), 1);
    assert!(!plain.take_notifier_failure());
}
