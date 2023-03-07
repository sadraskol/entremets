fn make_path(module: &str, name: &str) -> (String, String) {
    let mut path = std::path::PathBuf::from("tests");
    for part in module.split("::").collect::<Vec<_>>() {
        path.push(part);
    }
    path.push(name);
    let mut mets = path.clone();
    let mut expected = path.clone();
    mets.set_extension("mets");
    expected.set_extension("expected");
    (
        mets.to_str().unwrap().to_string(),
        expected.to_str().unwrap().to_string(),
    )
}

macro_rules! entremets_test {
    ($($name:ident),*) => {
    $(
        #[test]
        fn $name() {
            use crate::make_path;
            let (mets, expected) = make_path(module_path!(), stringify!($name));
            let x = std::process::Command::new("cargo")
                .arg("run")
                .arg(mets)
                .output()
                .expect("failed to execute process");
            let output = String::from_utf8(x.stdout).expect("no stdout");

            let expected_output =
                std::fs::read_to_string(expected)
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

mod read_committed {
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
}

mod count {
    entremets_test! {
        count_star,
        item_not_in_aggregate
    }
}

mod string {
    entremets_test! {
        string
    }
}

