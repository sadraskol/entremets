macro_rules! entremets_test {
    ($($name:ident),*) => {
    $(
        #[test]
        fn $name() {
            let x = std::process::Command::new("cargo")
                .arg("run")
                .arg(&format!("tests/read_committed/{}.mets", stringify!($name)))
                .output()
                .expect("failed to execute process");
            let output = String::from_utf8(x.stdout).expect("no stdout");

            let expected_output =
                std::fs::read_to_string(&format!("tests/read_committed/{}.expected", stringify!($name)))
                    .expect("no expected result");
            assert!(
                output.contains(&expected_output),
                "testing scenario {}: {}",
                stringify!($name),
                output
            );
        }
    )*
    }
}

entremets_test! {
    aborted_reads,
    circular_information_flow,
    deadlocks,
    duplicate_creation,
    intermediate_reads,
    lost_update,
    multiple_columns_unique_contraint,
    multiple_columns_update,
    not_lost_update,
    observed_transaction_vanishes,
    optimistic_lost_update,
    optimistic_lost_update_aborted,
    predicate_many_preceders,
    unique_contraint,
    write_cycles
}
