use super::*;

#[test]
fn replay_commands_reject_reversed_capability_ranges() {
    for command in ["replay", "replay-raw"] {
        for (index, field) in ["slots", "x", "y"].into_iter().enumerate() {
            let mut ranges = ["0..=4", "0..=1000", "0..=700"];
            ranges[index] = "4..=0";
            let path = unique_temp_path("edgepad-reversed-capabilities.ev");
            fs::write(
                &path,
                format!(
                    "# slots: {}\n# x: {}\n# y: {}\n",
                    ranges[0], ranges[1], ranges[2]
                ),
            )
            .expect("malformed capture should be written");

            let output = edgepad()
                .arg(command)
                .arg(&path)
                .arg("--built-in-defaults")
                .output()
                .expect("edgepad binary should run");
            fs::remove_file(&path).expect("capture should be removed");

            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{command} should report a parsing error for {field}; stderr: {stderr}"
            );
            assert!(stderr.starts_with("parse failed:"), "stderr was: {stderr}");
            assert!(
                stderr.contains(&format!("line: {}", index + 1)),
                "stderr was: {stderr}"
            );
            assert!(
                stderr.contains(&format!("{field:?}")),
                "stderr was: {stderr}"
            );
            assert!(stderr.contains("4..=0"), "stderr was: {stderr}");
        }
    }
}
