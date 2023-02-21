use crate::engine::{Report, Violation};
use crate::parser::Mets;
use crate::sql_interpreter::SqlDatabase;

pub fn summary(mets: &Mets, report: &Report) -> String {
    let mut base = if let Some(violation) = &report.violation {
        let mut x = String::new();
        let state = match violation {
            Violation::PropertyViolation { property, state } => {
                x.push_str(&format!("Following property was violated: {property}\n"));
                x.push_str("The following counter example was found:\n");
                state.clone()
            }
            Violation::Deadlock { cycle, state } => {
                x.push_str("System ran into a deadlock:\n");
                for p in cycle {
                    let borrowed_state = state.borrow();
                    let tid = &borrowed_state.txs[*p].id.unwrap();
                    let context = borrowed_state.sql.transactions.get(tid).unwrap();

                    x.push_str(&format!(
                        "Process {p} holds lock on {:?} and waits for {:?}\n",
                        context.locks, borrowed_state.processes[*p]
                    ));
                }
                state.clone()
            }
        };

        let mut traces = vec![];
        let mut current = state;
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
                .find(|(_i, (a, b))| a != b)
            {
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
    for (table_name, table) in global.tables.iter() {
        x.push_str(&format!("{table_name}: {{"));

        let mut x1 = table.rows.iter().peekable();
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
