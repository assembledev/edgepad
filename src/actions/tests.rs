use super::test_support::*;
use super::*;
use crate::core::{GestureDirection, SliderDirection, Zone};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn matching_command_action_is_queued_and_waited_by_worker_runner() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Left,
            GestureDirection::Right,
            GestureActionConfig::Command {
                argv: vec![
                    "notify-send".to_string(),
                    "edgepad".to_string(),
                    "left right".to_string(),
                ],
            },
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Right));
    let argv = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("worker should run queued action");
    let stats = dispatcher.shutdown();

    assert_eq!(
        argv,
        vec![
            "notify-send".to_string(),
            "edgepad".to_string(),
            "left right".to_string()
        ]
    );
    assert_eq!(stats.matched_gestures, 1);
    assert_eq!(stats.queued_commands, 1);
    assert_eq!(stats.started_commands, 1);
    assert_eq!(stats.succeeded_commands, 1);
    assert_eq!(stats.failed_commands, 0);
}

#[test]
fn matching_slider_step_action_is_queued_and_waited_by_worker_runner() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
        Vec::new(),
        vec![slider_binding(
            Zone::Left,
            SliderDirection::Up,
            vec!["pamixer", "-d", "3"],
            SliderDirection::Down,
            vec!["pamixer", "-i", "3"],
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_slider_step(slider_step(Zone::Left, SliderDirection::Down));
    let argv = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("worker should run queued action");
    let stats = dispatcher.shutdown();

    assert_eq!(
        argv,
        vec!["pamixer".to_string(), "-i".to_string(), "3".to_string()]
    );
    assert_eq!(stats.matched_slider_steps, 1);
    assert_eq!(stats.queued_commands, 1);
    assert_eq!(stats.started_commands, 1);
    assert_eq!(stats.succeeded_commands, 1);
    assert_eq!(stats.failed_commands, 0);
}

#[test]
fn unmatched_gesture_does_not_queue_action() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Left,
            GestureDirection::Right,
            GestureActionConfig::Command {
                argv: vec!["notify-send".to_string(), "edgepad".to_string()],
            },
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Down));
    let stats = dispatcher.shutdown();

    assert!(receiver.try_recv().is_err());
    assert_eq!(stats.unmatched_gestures, 1);
    assert_eq!(stats.queued_commands, 0);
    assert_eq!(stats.started_commands, 0);
}

#[test]
fn double_tap_binding_dispatches_only_the_sequence_gesture() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Top,
            GestureDirection::DoubleTap,
            GestureActionConfig::Command {
                argv: vec!["playerctl".to_string(), "stop".to_string()],
            },
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Top, GestureDirection::DoubleTap));
    let argv = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("double-tap action should run");
    let stats = dispatcher.shutdown();

    assert_eq!(stats.matched_gestures, 1);
    assert_eq!(argv, vec!["playerctl".to_string(), "stop".to_string()]);
}

#[test]
fn log_action_is_counted_without_queueing_command() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Top,
            GestureDirection::Down,
            GestureActionConfig::Log,
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Top, GestureDirection::Down));
    let stats = dispatcher.shutdown();

    assert!(receiver.try_recv().is_err());
    assert_eq!(stats.matched_gestures, 1);
    assert_eq!(stats.log_actions, 1);
    assert_eq!(stats.queued_commands, 0);
}

#[test]
fn reconfigure_replaces_gesture_and_slider_lookups_without_restarting_worker() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Left,
            GestureDirection::Right,
            GestureActionConfig::Log,
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::success(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Down));
    dispatcher.reconfigure(
        vec![binding(
            Zone::Right,
            GestureDirection::Down,
            GestureActionConfig::Log,
        )],
        Vec::new(),
    );
    dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Down));
    let stats = dispatcher.shutdown();

    assert!(receiver.try_recv().is_err());
    assert_eq!(stats.unmatched_gestures, 1);
    assert_eq!(stats.matched_gestures, 1);
    assert_eq!(stats.log_actions, 1);
}

#[test]
fn failed_command_status_is_recorded() {
    let (sender, receiver) = mpsc::channel();
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Bottom,
            GestureDirection::Up,
            GestureActionConfig::Command {
                argv: vec!["false".to_string()],
            },
        )],
        4,
        RecordingRunner {
            sender,
            status: ActionCommandStatus::failure(),
        },
    )
    .expect("dispatcher should start");

    dispatcher.dispatch_gesture(gesture(Zone::Bottom, GestureDirection::Up));
    receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("worker should run queued action");
    let stats = dispatcher.shutdown();

    assert_eq!(stats.started_commands, 1);
    assert_eq!(stats.succeeded_commands, 0);
    assert_eq!(stats.failed_commands, 1);
}

#[path = "tests/lifecycle.rs"]
mod lifecycle;

#[path = "tests/queue.rs"]
mod queue;
