use crate::engine::Report;
use crate::parser::Mets;
use crate::sql_interpreter::SqlDatabase;

pub fn summary(mets: &Mets, report: &Report) -> String {
    let mut base = if let Some(violation) = &report.violation {
        let mut x = format!("Following property was violated: {}\n", violation.property);
        x.push_str("The following counter example was found:\n");

        let mut traces = vec![];
        let mut current = violation.state.clone();
        loop {
            traces.push(current.clone());
            let x = if let Some(x) = current.borrow().ancestors.get(0) {
                x.clone()
            } else {
                break;
            };
            current = x;
        }
        traces.reverse();

        let mut last_trace = traces[0].borrow();
        if !last_trace.locals.is_empty() {
            x.push_str(&format!("Local State {:?}\n", last_trace.locals));
        }
        x.push_str(&sql_summary(&last_trace.sql));

        for trace in &traces[1..] {
            let trace = trace.borrow();
            if let Some((index, _)) = (trace.pc.iter().zip(&last_trace.pc))
                .enumerate()
                .find(|(_i, (a, b))| a != b) {
                x.push_str(&format!(
                    "Process {}: {}\n",
                    index,
                    mets.processes[index][trace.pc[index] - 1]
                ));
            }
            if !trace.locals.is_empty() {
                x.push_str(&format!("Local State {:?}\n", trace.locals));
            }
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
        x.push_str(&format!("{table}: {{"));

        let mut x1 = rows.iter().peekable();
        while let Some(row) = x1.next() {
            x.push('(');
            let values = row.values();
            let mut enumerate = values.iter().enumerate().peekable();
            while let Some((i, _)) = enumerate.next() {
                x.push_str(&format!("{}: {}", row.keys()[i], row.values()[i]));
                if enumerate.peek().is_some() {
                    x.push_str(", ");
                }
            }
            x.push(')');

            if x1.peek().is_some() {
                x.push_str(", ");
            }
        }
        x.push_str("}\n");
    }

    x
}
