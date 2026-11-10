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

fn lifecycle_slider() -> SliderBindingConfig {
    let mut binding = slider("step");
    binding.on_release = Some(command_action(vec!["release"]));
    binding.on_cancel = Some(command_action(vec!["cancel"]));
    binding
}

fn end(kind: crate::core::SliderEndKind) -> crate::core::SliderEnd {
    crate::core::SliderEnd {
        zone: Zone::Bottom,
        slot: 0,
        tracking_id: 42,
        kind,
    }
}

#[test]
fn release_keeps_its_reserved_place_when_movement_fills_the_queue() {
    use crate::core::SliderEndKind;
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
        vec![],
        vec![lifecycle_slider()],
        2,
        GatedRunner {
            started: started_tx,
            release: release_rx,
        },
    )
    .unwrap();
    let step = slider_step(Zone::Bottom, SliderDirection::Right);
    dispatcher.dispatch_slider_step(step);
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ["step"]
    );
    dispatcher.dispatch_slider_step(step);
    dispatcher.dispatch_slider_step(step);
    dispatcher.dispatch_slider_end(end(SliderEndKind::Release));
    for expected in ["step", "release"] {
        release_tx.send(()).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            [expected]
        );
    }
    release_tx.send(()).unwrap();
    let stats = dispatcher.shutdown();
    assert_eq!(stats.succeeded_commands, 3);
    assert_eq!(stats.dropped_commands, 1);
}

#[test]
fn overlapping_contact_cannot_step_or_finish_the_owners_drag() {
    use crate::core::{SliderEnd, SliderEndKind};
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
        vec![],
        vec![lifecycle_slider()],
        8,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .unwrap();
    let step = slider_step(Zone::Bottom, SliderDirection::Right);
    dispatcher.dispatch_slider_step(step);
    let other = SliderStep {
        slot: 1,
        tracking_id: 43,
        ..step
    };
    dispatcher.dispatch_slider_step(other);
    dispatcher.dispatch_slider_end(SliderEnd {
        slot: 1,
        tracking_id: 43,
        ..end(SliderEndKind::Release)
    });
    dispatcher.dispatch_slider_end(end(SliderEndKind::Cancel));
    dispatcher.dispatch_slider_end(end(SliderEndKind::Release));
    dispatcher.shutdown();
    assert_eq!(
        receiver.into_iter().collect::<Vec<_>>(),
        [vec!["step"], vec!["cancel"]]
    );
}

#[test]
fn a_drag_rejected_at_admission_cannot_run_a_terminal_command() {
    use crate::core::SliderEndKind;
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
        vec![],
        vec![lifecycle_slider()],
        1,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .unwrap();
    let step = slider_step(Zone::Bottom, SliderDirection::Right);
    dispatcher.dispatch_slider_step(step);
    dispatcher.dispatch_slider_step(step);
    dispatcher.dispatch_slider_end(end(SliderEndKind::Release));
    dispatcher.shutdown();
    assert!(receiver.into_iter().next().is_none());
}

#[test]
fn shutdown_cancels_an_admitted_drag_and_reload_keeps_its_original_hook() {
    use crate::core::SliderEndKind;
    for release in [false, true] {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
            vec![],
            vec![lifecycle_slider()],
            8,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::success(),
            },
        )
        .unwrap();
        dispatcher.dispatch_slider_step(slider_step(Zone::Bottom, SliderDirection::Right));
        let mut changed = lifecycle_slider();
        changed.on_release = Some(command_action(vec!["new-release"]));
        dispatcher.reconfigure(vec![], vec![changed]);
        if release {
            dispatcher.dispatch_slider_end(end(SliderEndKind::Release));
        }
        dispatcher.shutdown();
        let terminal = if release { "release" } else { "cancel" };
        assert_eq!(
            receiver.into_iter().collect::<Vec<_>>(),
            [vec!["step"], vec![terminal]]
        );
    }
}
