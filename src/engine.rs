use crate::engine::CheckerError::{TypeError, Unexpected};
use crate::parser::{Expression, Mets, Operator, Statement, Variable};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
enum Value {
    Nil,
    Bool(bool),
    Integer(i16),
    Set(Vec<Value>),
    Tuple(Vec<Value>),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct HashableState {
    pc: Vec<usize>,
    global: Vec<(String, Vec<HashableRow>)>,
    locals: Vec<(String, Value)>,
}

#[derive(PartialEq, Debug, Clone)]
struct Trace {
    pc: Vec<usize>,
    sql: SqlDatabase,
    locals: HashMap<String, Value>,
}

#[derive(PartialEq, Debug, Clone)]
struct State {
    pc: Vec<usize>,
    sql: SqlDatabase,
    locals: HashMap<String, Value>,
    log: Vec<Trace>,
}

impl State {
    fn trace(&self) -> Trace {
        Trace {
            pc: self.pc.clone(),
            sql: self.sql.clone(),
            locals: self.locals.clone(),
        }
    }

    fn hashable(&self) -> HashableState {
        HashableState {
            pc: self.pc.clone(),
            global: self.sql.hashable(),
            locals: self
                .locals
                .iter()
                .map(|(l, r)| (l.clone(), r.clone()))
                .collect(),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
struct Violation {
    log: Vec<Trace>,
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct HashableRow {
    keys: Vec<String>,
    values: Vec<Value>,
}

pub struct Report {
    violation: Option<Violation>,
}

pub enum CheckerError {
    Unexpected(String),
    TypeError(Expression, String),
}

type Res<T> = Result<T, CheckerError>;
type Unit = Res<()>;

pub fn model_checker(mets: Mets) -> Result<Report, String> {
    match private_model_checker(mets) {
        Ok(res) => Ok(res),
        Err(err) => Err(match err {
            Unexpected(message) => format!("unexpected: {}", message),
            TypeError(expr, kind) => format!("{:?} is not an {}", expr, kind),
        }),
    }
}

fn private_model_checker(mets: Mets) -> Res<Report> {
    let init_state = init_state(&mets)?;

    let mut deq = VecDeque::from([init_state]);
    let mut visited = HashSet::new();

    while let Some(state) = deq.pop_front() {
        if visited.contains(&state.hashable()) {
            continue;
        }
        visited.insert(state.hashable());

        let mut interpreter = Interpreter::new(&state);

        for property in &mets.properties {
            if !interpreter.check_property(property)? {
                let mut log = state.log.clone();
                log.push(state.trace());
                return Ok(Report {
                    violation: Some(Violation { log }),
                });
            }
        }

        for (idx, code) in mets.processes.iter().enumerate() {
            if state.pc[idx] < code.len() {
                interpreter.statement(&code[state.pc[idx]])?;
                let mut new_state = interpreter.reset();
                new_state.pc[idx] += 1;
                new_state.log.push(state.trace());
                deq.push_back(new_state);
            }
        }
    }

    Ok(Report { violation: None })
}

fn init_state(mets: &Mets) -> Res<State> {
    let init_state = State {
        pc: mets.processes.iter().map(|_| 0).collect(),
        sql: SqlDatabase::new(),
        locals: HashMap::new(),
        log: vec![],
    };
    let mut interpreter = Interpreter::new(&init_state);
    for statement in &mets.init {
        interpreter.statement(statement)?;
    }
    Ok(interpreter.reset())
}

enum SqlContext {
    Where { table: String, row: Row },
    Update { table: String, row: Row },
}

struct Interpreter {
    state: State,
    next_state: State,
    sql_context: Option<SqlContext>,
}

impl Interpreter {
    fn new(state: &State) -> Self {
        Interpreter {
            state: state.clone(),
            next_state: state.clone(),
            sql_context: None,
        }
    }

    fn reset(&mut self) -> State {
        std::mem::replace(&mut self.next_state, self.state.clone())
    }

    fn check_property(&mut self, property: &Statement) -> Res<bool> {
        match property {
            Statement::Always(always) => {
                let value = self.interpret(always)?;
                Ok(value == Value::Bool(true))
            }
            _ => Err(Unexpected(format!("unsupported property: {:?}", property))),
        }
    }

    fn statement(&mut self, statement: &Statement) -> Unit {
        match statement {
            Statement::Begin(_) => {}
            Statement::Commit => {}
            Statement::Abort => {}
            Statement::Expression(expr) => {
                self.interpret(expr)?;
            }
            Statement::Latch => {}
            _ => panic!("Unexpected statement in process: {:?}", statement),
        };
        Ok(())
    }

    fn interpret(&mut self, expression: &Expression) -> Res<Value> {
        match expression {
            Expression::Select {
                columns,
                from,
                condition,
            } => self.interpret_select(columns, from, condition),
            Expression::Update {
                relation,
                update,
                condition,
            } => self.interpret_update(&relation, update, condition),
            Expression::Insert {
                relation,
                columns,
                values: exprs,
            } => self.interpret_insert(relation, columns, exprs),
            Expression::Assignment(variable, expr) => {
                let value = self.interpret(expr)?;
                self.assign(variable.name.clone(), value);
                Ok(Value::Nil)
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => self.interpret_binary(left, operator, right),
            Expression::Var(variable) => Ok(self.get(&variable.name)),
            Expression::Integer(i) => Ok(Value::Integer(*i)),
            Expression::Set(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?)
                }
                Ok(Value::Set(res))
            }
            Expression::Tuple(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?)
                }
                Ok(Value::Tuple(res))
            }
        }
    }

    fn interpret_insert(
        &mut self,
        relation: &Variable,
        columns: &[Variable],
        exprs: &[Expression],
    ) -> Res<Value> {
        assert!(self.sql_context.is_none());

        let mut values = vec![];
        for expr in exprs {
            values.push(self.assert_tuple(expr)?)
        }

        let table = self
            .next_state
            .sql
            .tables
            .entry(relation.name.clone())
            .or_default();

        for value in values {
            let mut new_row = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                new_row.insert(col.name.clone(), value[i].clone());
            }
            table.push(Row(new_row))
        }

        Ok(Value::Nil)
    }

    fn interpret_update(
        &mut self,
        relation: &&Variable,
        update: &Expression,
        condition: &Option<Box<Expression>>,
    ) -> Res<Value> {
        assert!(self.sql_context.is_none());
        let default_rows = vec![];
        let rows = self
            .state
            .sql
            .tables
            .get(&relation.name)
            .unwrap_or(&default_rows)
            .clone();

        for row in &rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: relation.name.clone(),
                });
                if self.interpret(cond)? == Value::Bool(true) {
                    self.sql_context = Some(SqlContext::Update {
                        row: row.clone(),
                        table: relation.name.clone(),
                    });
                    self.interpret(update)?;
                }
            } else {
                self.sql_context = Some(SqlContext::Update {
                    row: row.clone(),
                    table: relation.name.clone(),
                });
                self.interpret(update)?;
            }
            self.sql_context = None;
        }
        Ok(Value::Nil)
    }

    fn interpret_select(
        &mut self,
        columns: &[Variable],
        from: &Variable,
        condition: &Option<Box<Expression>>,
    ) -> Res<Value> {
        assert!(self.sql_context.is_none());
        let columns: Vec<_> = columns.iter().map(|v| v.name.clone()).collect();
        let default_rows = vec![];
        let rows = self
            .state
            .sql
            .tables
            .get(&from.name)
            .unwrap_or(&default_rows)
            .clone();

        let mut res = vec![];
        for row in &rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: from.name.clone(),
                });
                if self.interpret(cond)? == Value::Bool(true) {
                    res.push(row.to_value(&columns))
                }
                self.sql_context = None;
            } else {
                res.push(row.to_value(&columns))
            }
        }

        if res.len() == 1 {
            let row = if let Value::Tuple(x) = &res[0] {
                Ok(x)
            } else {
                panic!("expected to be a tuple")
            }?;
            if row.len() == 1 {
                return Ok(row[0].clone());
            }
        }

        Ok(Value::Set(res))
    }

    fn assert_integer(&mut self, expr: &Expression) -> Res<i16> {
        if let Value::Integer(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(TypeError(expr.clone(), "integer".to_string()))
        }
    }

    fn assert_set(&mut self, expr: &Expression) -> Res<Vec<Value>> {
        if let Value::Set(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(TypeError(expr.clone(), "set".to_string()))
        }
    }

    fn assert_bool(&mut self, expr: &Expression) -> Res<bool> {
        if let Value::Bool(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(TypeError(expr.clone(), "bool".to_string()))
        }
    }

    fn assert_tuple(&mut self, expr: &Expression) -> Res<Vec<Value>> {
        if let Value::Tuple(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(TypeError(expr.clone(), "tuple".to_string()))
        }
    }

    fn interpret_binary(
        &mut self,
        left: &Expression,
        operator: &Operator,
        right: &Expression,
    ) -> Res<Value> {
        match operator {
            Operator::Add => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left + right))
            }
            Operator::Multiply => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left * right))
            }
            Operator::Rem => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left % right))
            }
            Operator::Equal => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;

                Ok(Value::Bool(left == right))
            }
            Operator::LessEqual => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left <= right))
            }
            Operator::Less => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left < right))
            }
            Operator::Included => {
                let left = self.interpret(left)?;
                let right = if let Value::Set(right) = self.interpret(right)? {
                    right
                } else {
                    todo!()
                };
                Ok(Value::Bool(right.contains(&left)))
            }
            Operator::And => {
                let left = if let Value::Bool(left) = self.interpret(left)? {
                    left
                } else {
                    todo!()
                };
                let right = if let Value::Bool(right) = self.interpret(right)? {
                    right
                } else {
                    todo!()
                };
                Ok(Value::Bool(left && right))
            }
        }
    }

    fn assign(&mut self, name: String, value: Value) {
        if let Some(sql_context) = &self.sql_context {
            match sql_context {
                SqlContext::Where { .. } => {
                    self.next_state.locals.insert(name, value);
                }
                SqlContext::Update { table, row } => {
                    let rows = self.next_state.sql.tables.get_mut(table).unwrap();
                    for r in rows {
                        if r == row {
                            r.0.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
        } else {
            self.next_state.locals.insert(name, value);
        }
    }

    fn get(&self, name: &String) -> Value {
        if let Some(sql_context) = &self.sql_context {
            match sql_context {
                SqlContext::Where { row, .. } => row.0.get(name).unwrap(),
                SqlContext::Update { .. } => self.state.locals.get(name).unwrap(),
            }
        } else {
            self.state.locals.get(name).unwrap()
        }
        .clone()
    }
}

pub fn summary(report: &Report) -> String {
    if let Some(violation) = &report.violation {
        let mut x = "Following property was violated:\n".to_string();
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
            x.push_str(&format!("Process {}: **stmt**\n", index));
            x.push_str(&format!("Local State {:?}:\n", trace.locals));
            x.push_str("Global State:\n");
            x.push_str(&sql_summary(&trace.sql));
            last_trace = trace;
        }
        x
    } else {
        "No counter example found".to_string()
    }
}

#[derive(PartialEq, Debug, Clone)]
struct Row(HashMap<String, Value>);

impl Row {
    fn to_value(&self, columns: &[String]) -> Value {
        let mut res = vec![];
        for col in columns {
            res.push(self.0.get(col).unwrap().clone())
        }
        Value::Tuple(res)
    }

    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.0.values().cloned().collect()
    }

    fn hashable(self) -> HashableRow {
        let (keys, values): (Vec<String>, Vec<Value>) = self.0.into_iter().unzip();
        HashableRow { keys, values }
    }
}

#[derive(PartialEq, Debug, Clone)]
struct SqlDatabase {
    tables: HashMap<String, Vec<Row>>,
}

impl SqlDatabase {
    fn hashable(&self) -> Vec<(String, Vec<HashableRow>)> {
        let mut res = vec![];
        for (name, rows) in &self.tables {
            res.push((
                name.clone(),
                rows.iter().map(|row| row.clone().hashable()).collect(),
            ));
        }
        res
    }
}

impl SqlDatabase {
    fn new() -> SqlDatabase {
        SqlDatabase {
            tables: Default::default(),
        }
    }
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
