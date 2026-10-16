use super::*;

#[test]
fn action_command_poll_interval_backs_off_to_small_bound() {
    let first = ACTION_COMMAND_INITIAL_POLL_INTERVAL;
    let second = next_action_command_poll_interval(first);
    let third = next_action_command_poll_interval(second);

    assert!(first < Duration::from_millis(50));
    assert_eq!(second, Duration::from_millis(2));
    assert_eq!(third, Duration::from_millis(4));
    assert_eq!(
        next_action_command_poll_interval(Duration::from_millis(100)),
        ACTION_COMMAND_MAX_POLL_INTERVAL
    );
}

#[test]
fn process_runner_reports_exit_status_and_setup_errors() {
    let mut runner = ProcessCommandRunner;
    let shutdown = ActionShutdownToken::default();
    for (script, success) in [("exit 0", true), ("exit 7", false)] {
        let argv = ["sh", "-c", script].map(str::to_string);
        assert_eq!(
            runner.run(&argv, &shutdown).unwrap(),
            ActionCommandStatus { success },
        );
    }
    assert_eq!(
        runner.run(&[], &shutdown).unwrap_err(),
        "command action argv must not be empty",
    );
    assert!(runner
        .run(&[String::new()], &shutdown)
        .unwrap_err()
        .starts_with("failed to spawn action command"),);
}

#[cfg(target_os = "linux")]
mod unix;
