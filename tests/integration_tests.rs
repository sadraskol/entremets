const FILES: &[&str] = &[
    "aborted_reads",
    "circular_information_flow",
    "deadlocks",
    "intermediate_reads",
    "lost_update",
    "observed_transaction_vanishes",
    "predicate_many_preceders",
    "write_cycles",
];

#[test]
fn test_read_committed_scenarios() {
    for file in FILES {
        let x = std::process::Command::new("cargo")
            .arg("run")
            .arg(&format!("tests/read_committed/{}.mets", file))
            .output()
            .expect("failed to execute process");
        let output = String::from_utf8(x.stdout).expect("no stdout");

        let expected_output =
            std::fs::read_to_string(&format!("tests/read_committed/{}.expected", file))
                .expect("no expected result");
        assert!(
            output.contains(&expected_output),
            "testing scenario {}: {}",
            file,
            output
        );
    }
}
