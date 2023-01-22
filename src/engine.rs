// use std::collections::{HashMap, HashSet, VecDeque};
//
// #[derive(Hash, Eq, PartialEq, Debug, Clone)]
// enum Value {
//     Bool(bool),
//     Integer(i16),
//     Set(Vec<Row>),
// }
//
// #[derive(Hash, Eq, PartialEq, Debug, Clone)]
// struct HashableState {
//     pc: Vec<usize>,
//     global: Vec<(String, Value)>,
//     locals: Vec<Vec<(String, Value)>>,
// }
//
// #[derive(PartialEq, Debug, Clone)]
// struct Trace {
//     pc: Vec<usize>,
//     global: HashMap<String, Value>,
//     locals: Vec<HashMap<String, Value>>,
// }
//
// #[derive(PartialEq, Debug, Clone)]
// struct State {
//     pc: Vec<usize>,
//     global: HashMap<String, Value>,
//     locals: Vec<HashMap<String, Value>>,
//     log: Vec<Trace>,
// }
//
// impl State {
//     fn trace(&self) -> Trace {
//         Trace {
//             pc: self.pc.clone(),
//             global: self.global.clone(),
//             locals: self.locals.clone(),
//         }
//     }
//
//     fn hashable(&self) -> HashableState {
//         HashableState {
//             pc: self.pc.clone(),
//             global: self
//                 .global
//                 .iter()
//                 .map(|(l, r)| (l.clone(), r.clone()))
//                 .collect(),
//             locals: self
//                 .locals
//                 .iter()
//                 .map(|l| l.iter().map(|(l, r)| (l.clone(), r.clone())).collect())
//                 .collect(),
//         }
//     }
// }
//
// fn eval(state: &State, select: &Sigma) -> Value {
//     let binding = Value::Set(vec![]);
//     let from = state.global.get(&select.relation.name).unwrap_or(&binding);
//     match &select.formula {
//         Proposition::LesserEqual(left, right) => {
//             let mut res = vec![];
//             let rows = if let Value::Set(rows) = from {
//                 rows
//             } else {
//                 todo!()
//             };
//             for row in rows {
//                 let l = if let Value::Integer(l) = eval_prop_row(row, left) {
//                     l
//                 } else {
//                     todo!()
//                 };
//                 let r = if let Value::Integer(r) = eval_prop_row(row, right) {
//                     r
//                 } else {
//                     todo!()
//                 };
//                 if l <= r {
//                     res.push(row.clone());
//                 }
//             }
//             Value::Set(res)
//         }
//         _ => panic!("{:?} not yet implemented", select.formula),
//     }
// }
//
// fn eval_prop_local(state: &State, local: &HashMap<String, Value>, prop: &Proposition) -> Value {
//     match prop {
//         Proposition::LesserEqual(left, right) => {
//             let l = if let Value::Integer(l) = eval_prop_local(state, local, left) {
//                 l
//             } else {
//                 todo!()
//             };
//             let r = if let Value::Integer(r) = eval_prop_local(state, local, right) {
//                 r
//             } else {
//                 todo!()
//             };
//             Value::Bool(l <= r)
//         }
//         Proposition::Count(set) => {
//             let s = if let Value::Set(s) = eval_prop_local(state, local, set) {
//                 s
//             } else {
//                 todo!()
//             };
//             Value::Integer(i16::try_from(s.len()).expect("set is too large for count"))
//         }
//         Proposition::Var(var) => local.get(&var.name).expect("oupsy").clone(),
//         Proposition::Literal(value) => value.clone(),
//         Proposition::Sigma(sigma) => eval(state, sigma),
//         _ => panic!("{:?} is not implemented for evaluation", prop),
//     }
// }
//
// #[allow(clippy::expect_fun_call)]
// fn eval_prop_row(row: &Row, prop: &Proposition) -> Value {
//     match prop {
//         Proposition::Literal(value) => value.clone(),
//         Proposition::Var(v) => row
//             .get(&v.name)
//             .expect(&format!("couldn't find column '{}' in table...", v.name)),
//         _ => panic!("{:?} is not implemented for row evaluation", prop),
//     }
// }
//
// #[derive(PartialEq, Debug, Clone)]
// struct Violation {
//     property: Property,
//     log: Vec<Trace>,
// }
//
// #[derive(Hash, Eq, PartialEq, Debug, Clone)]
// struct Row {
//     keys: Vec<String>,
//     values: Vec<Value>,
// }
//
// impl Row {
//     pub fn get(&self, key: &String) -> Option<Value> {
//         for (i, k) in self.keys.iter().enumerate() {
//             if k == key {
//                 return Some(self.values[i].clone());
//             }
//         }
//         None
//     }
// }
//
// impl Row {
//     fn new() -> Self {
//         Row {
//             keys: vec![],
//             values: vec![],
//         }
//     }
// }
//
// impl<const N: usize> From<[(String, Value); N]> for Row {
//     fn from(arr: [(String, Value); N]) -> Self {
//         let mut res = Row::new();
//         for entry in arr {
//             res.keys.push(entry.0);
//             res.values.push(entry.1);
//         }
//         res
//     }
// }
//
// fn apply_statement(state: &State, idx: usize, client: &Client) -> State {
//     let mut new_state = state.clone();
//     match &client.statements[state.pc[idx]] {
//         Statement::Begin(_) => {
//             new_state.pc[idx] += 1;
//             new_state
//         }
//         Statement::Commit => {
//             new_state.pc[idx] += 1;
//             new_state
//         }
//         Statement::Assignment(v, expression) => {
//             new_state.pc[idx] += 1;
//             new_state.locals[idx].insert(v.name.clone(), eval(state, expression));
//             new_state
//         }
//         Statement::Insert(relation, row) => {
//             new_state.pc[idx] += 1;
//             let table = if let Value::Set(table) = new_state
//                 .global
//                 .entry(relation.name.clone())
//                 .or_insert(Value::Set(vec![]))
//             {
//                 table
//             } else {
//                 todo!()
//             };
//             table.push(row.clone());
//             new_state.global.iter_mut();
//             new_state
//         }
//     }
// }
//
// fn check_property(state: &State, property: &Property) -> Value {
//     let prop = if let Property(Proposition::Always(prop)) = property {
//         prop
//     } else {
//         todo!()
//     };
//     eval_prop_local(state, &state.global, prop)
// }
//
// struct Report {
//     violation: Option<Violation>,
// }
//
// fn model_checker(clients: &[Client], properties: &Vec<Property>) -> Report {
//     let init_state = State {
//         pc: clients.iter().map(|_| 0).collect(),
//         global: HashMap::new(),
//         locals: clients.iter().map(|_| HashMap::new()).collect(),
//         log: vec![],
//     };
//
//     let mut deq = VecDeque::from([init_state]);
//     let mut visited = HashSet::new();
//     let mut state_checked: u128 = 0;
//
//     while let Some(state) = deq.pop_front() {
//         // println!("checking for state: {:?}", state);
//         if visited.contains(&state.hashable()) {
//             continue;
//         }
//         visited.insert(state.hashable());
//
//         for property in properties {
//             if check_property(&state, property) != Value::Bool(true) {
//                 let mut log = state.log.clone();
//                 log.push(state.trace());
//                 return Report {
//                     violation: Some(Violation {
//                         property: property.clone(),
//                         log,
//                     }),
//                 };
//             }
//         }
//
//         state_checked += 1;
//         for (idx, client) in clients.iter().enumerate() {
//             if state.pc[idx] < client.statements.len() {
//                 let mut new_state = apply_statement(&state, idx, client);
//                 new_state.log.push(state.trace());
//                 deq.push_back(new_state);
//             }
//         }
//     }
//     println!("state_checked {}", state_checked);
//
//     Report { violation: None }
// }
//
// fn sql_summary(global: &HashMap<String, Value>) -> String {
//     let mut x = String::new();
//     for (table, value) in global.iter() {
//         let rows = if let Value::Set(rows) = value {
//             rows
//         } else {
//             todo!()
//         };
//         if rows.is_empty() {
//             x.push_str(&format!("{}: empty\n", table));
//         } else {
//             x.push_str(&format!("--- {} ---\n", table));
//
//             for key in &rows[0].keys {
//                 x.push_str(&format!("{},", key));
//             }
//             x.remove(x.len() - 1);
//             x.push('\n');
//
//             for row in rows {
//                 for value in &row.values {
//                     x.push_str(&format!("{:?},", value));
//                 }
//                 x.remove(x.len() - 1);
//                 x.push('\n');
//             }
//         }
//     }
//     x
// }
//
// fn summary(report: &Report) -> String {
//     if let Some(violation) = &report.violation {
//         let mut x = format!("Following property was wrong: {:?}\n", violation.property);
//         x.push_str("The following counter example was found:\n");
//
//         x.push_str("Init state: empty\n");
//         let mut last_trace = &violation.log[0];
//         for trace in &violation.log[1..] {
//             let (index, _) = (trace.pc.iter().zip(&last_trace.pc))
//                 .enumerate()
//                 .find(|(_i, (a, b))| a != b)
//                 .expect("no pc changed in between states");
//             x.push_str(&format!("Process {}: **stmt**\n", index));
//             x.push_str(&format!("Local State {:?}:\n", trace.locals[index]));
//             x.push_str("Global State:\n");
//             x.push_str(&sql_summary(&trace.global));
//             last_trace = trace;
//         }
//         x
//     } else {
//         "No counter example found".to_string()
//     }
// }
