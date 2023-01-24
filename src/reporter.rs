use crate::engine::Report;
use crate::parser::{format_statement, Mets};
use crate::sql_engine::SqlDatabase;

pub fn summary(mets: &Mets, report: &Report) -> String {
    let mut base = if let Some(violation) = &report.violation {
        let mut x = format!(
            "Following property was violated: {}\n",
            format_statement(&violation.property)
        );
        x.push_str("The following counter example was found:\n");

        let mut last_trace = &violation.log[0];
        x.push_str(&format!("Local State {:?}:\n", last_trace.locals));
        x.push_str("Global State:\n");
        x.push_str(&sql_summary(&last_trace.sql));

        for trace in &violation.log[1..] {
            let (index, _) = (trace.pc.iter().zip(&last_trace.pc))
                .enumerate()
                .find(|(_i, (a, b))| a != b)
                .expect("no pc changed in between states");
            x.push_str(&format!(
                "Process {}: {}\n",
                index,
                format_statement(&mets.processes[index][trace.pc[index] - 1])
            ));
            x.push_str(&format!("Local State {:?}:\n", trace.locals));
            x.push_str("Global State:\n");
            x.push_str(&sql_summary(&trace.sql));
            last_trace = trace;
        }
        x
    } else {
        "No counter example found".to_string()
    };

    base.push_str(&format!("\nStates explored: {}", report.states_explored));
    base
}
fn sql_summary(global: &SqlDatabase) -> String {
    let mut x = String::new();
    for (table, rows) in global.tables.iter() {
        if rows.is_empty() {
            x.push_str(&format!("{}: empty\n", table));
        } else {
            x.push_str(&format!("--- {} ---\n", table));

            for key in &rows[0].keys() {
                x.push_str(&format!("{},", key));
            }
            x.remove(x.len() - 1);
            x.push('\n');

            for row in rows {
                for value in &row.values() {
                    x.push_str(&format!("{:?},", value));
                }
                x.remove(x.len() - 1);
                x.push('\n');
            }
        }
    }
    x
}
