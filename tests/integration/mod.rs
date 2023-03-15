fn make_path(module: &str, name: &str) -> (String, String) {
    let mut path = std::path::PathBuf::from("");
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

fn test_file(module: &str, name: &str) {
    let (mets, expected) = make_path(module, name);
    std::fs::File::open(&mets).expect(&format!("No file {mets}"));
    let x = std::process::Command::new("cargo")
        .arg("run")
        .arg(mets)
        .output()
        .expect("failed to execute process");
    let output = String::from_utf8(x.stdout).expect("no stdout");

    let expected_output = std::fs::read_to_string(&expected).expect(&format!("no file {expected}"));
    assert!(
        output.contains(&expected_output),
        "testing scenario {}: {}",
        name,
        output
    );
}

macro_rules! entremets_test {
    ($($name:ident),*) => {
    $(
        #[test]
        fn $name() {
            use crate::integration::test_file;
            test_file(module_path!(), stringify!($name));
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

mod comparisons {
    entremets_test! {
        comparison,
        between,
        order_by
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

mod delete {
    entremets_test! {
        delete,
        delete_visibility_in_transaction,
        delete_with_unicity,
        delete_with_update_lock
    }
}

mod limit {
    entremets_test! {
        limit
    }
}

mod group {
    entremets_test! {
        group
    }
}
