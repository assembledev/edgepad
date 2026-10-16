use super::*;
use std::thread;
use std::time::Instant;

fn command_binding(zone: Zone, argv: &[&str]) -> GestureBindingConfig {
    binding(
        zone,
        GestureDirection::Tap,
        GestureActionConfig::command(argv.iter().copied()).unwrap(),
    )
}

fn slider(program: &str) -> SliderBindingConfig {
    slider_binding(
        Zone::Bottom,
        SliderDirection::Left,
        vec![program],
        SliderDirection::Right,
        vec![program],
    )
}

#[test]
fn queued_gesture_and_slider_commands_keep_their_argv_across_reload() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
        vec![
            command_binding(Zone::Right, &["holding"]),
            command_binding(Zone::Left, &["old-gesture"]),
        ],
        vec![slider("old-slider")],
        4,
        GatedRunner {
            started: started_tx,
            release: release_rx,
        },
    )
    .unwrap();
    dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Tap));
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ["holding"]
    );
    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
    dispatcher.dispatch_slider_step(slider_step(Zone::Bottom, SliderDirection::Right));
    dispatcher.reconfigure(
        vec![command_binding(Zone::Left, &["new-gesture"])],
        vec![slider("new-slider")],
    );
    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
    dispatcher.dispatch_slider_step(slider_step(Zone::Bottom, SliderDirection::Right));

    for program in ["old-gesture", "old-slider", "new-gesture", "new-slider"] {
        release_tx.send(()).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            [program]
        );
    }
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while dispatcher.stats().succeeded_commands < 5 {
        assert!(
            Instant::now() < deadline,
            "all released commands should finish"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let stats = dispatcher.shutdown();
    assert_eq!(stats.queued_commands, 5);
    assert_eq!(stats.started_commands, 5);
    assert_eq!(stats.succeeded_commands, 5);
    assert_eq!(stats.dropped_commands, 0);
}

#[test]
fn full_queue_drops_new_actions_without_waiting_and_shutdown_skips_queued_work() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![command_binding(Zone::Left, &["command"])],
        1,
        GatedRunner {
            started: started_tx,
            release: release_rx,
        },
    )
    .unwrap();
    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));

    let (returned_tx, returned_rx) = mpsc::channel();
    let dispatch_thread = thread::spawn(move || {
        dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
        dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
        returned_tx.send(()).unwrap();
        dispatcher
    });
    let returned = returned_rx.recv_timeout(Duration::from_secs(1));
    if returned.is_err() {
        for _ in 0..4 {
            let _ = release_tx.send(());
        }
    }
    let dispatcher = dispatch_thread.join().unwrap();
    let stats = dispatcher.shutdown();
    assert!(returned.is_ok(), "a full action queue blocked dispatch");
    assert_eq!(stats.queued_commands, 2);
    assert_eq!(stats.dropped_commands, 2);
    assert_eq!(stats.started_commands, 1);
    assert_eq!(stats.failed_commands, 1);
    assert_eq!(stats.worker_shutdown_timeouts, 0);
    assert!(
        started_rx.try_recv().is_err(),
        "shutdown started queued work"
    );
}
