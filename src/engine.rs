use crate::engine::CheckerError::{TypeError, UndefinedVariable, Unexpected};
use crate::parser::{Expression, Mets, Operator, Statement, Variable};
use crate::sql_engine::{HashableRow, Row, SqlDatabase, SqlEngineError, TransactionId};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub enum Value {
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
pub struct Trace {
    pub pc: Vec<usize>,
    pub sql: SqlDatabase,
    pub locals: HashMap<String, Value>,
}

#[derive(PartialEq, Debug, Clone)]
struct State {
    pc: Vec<usize>,
    txs: Vec<Option<TransactionId>>,
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
pub struct Violation {
    pub log: Vec<Trace>,
}

pub struct Report {
    pub violation: Option<Violation>,
}

pub enum CheckerError {
    Unexpected(String),
    TypeError(Expression, String),
    UndefinedVariable(Variable),
    SqlEngineError(SqlEngineError),
}

impl From<SqlEngineError> for CheckerError {
    fn from(value: SqlEngineError) -> Self {
        CheckerError::SqlEngineError(value)
    }
}

type Res<T> = Result<T, CheckerError>;
type Unit = Res<()>;

pub fn model_checker(mets: Mets) -> Result<Report, String> {
    match private_model_checker(mets) {
        Ok(res) => Ok(res),
        Err(err) => Err(match err {
            Unexpected(message) => format!("unexpected: {}", message),
            TypeError(expr, kind) => format!("{:?} is not an {}", expr, kind),
            UndefinedVariable(variable) => format!("{:?} is not yet in the context", variable),
            CheckerError::SqlEngineError(x) => format!("{:?}", x),
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
                interpreter.idx = idx;
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
        txs: mets.processes.iter().map(|_| None).collect(),
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
    idx: usize,
    state: State,
    next_state: State,
    sql_context: Option<SqlContext>,
}

impl Interpreter {
    fn new(state: &State) -> Self {
        Interpreter {
            idx: 0,
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
            Statement::Begin(isolation) => {
                self.next_state.txs[self.idx] =
                    Some(self.next_state.sql.open_transaction(*isolation));
            }
            Statement::Commit => self.next_state.txs[self.idx] = None,
            Statement::Abort => self.next_state.txs[self.idx] = None,
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
            } => self.interpret_update(relation, update, condition),
            Expression::Insert {
                relation,
                columns,
                values: exprs,
            } => self.interpret_insert(relation, columns, exprs),
            Expression::Assignment(variable, expr) => {
                let value = self.interpret(expr)?;
                let name = variable.name.clone();
                self.next_state.locals.insert(name, value);
                Ok(Value::Nil)
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => self.interpret_binary(left, operator, right),
            Expression::Var(variable) => self
                .state
                .locals
                .get(&variable.name)
                .cloned()
                .ok_or(UndefinedVariable(variable.clone())),
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

        self.next_state.sql.insert_in_table(
            &self.state.txs[self.idx],
            &relation.name,
            values,
            columns,
        );

        Ok(Value::Nil)
    }

    fn interpret_update(
        &mut self,
        relation: &Variable,
        update: &Expression,
        condition: &Option<Box<Expression>>,
    ) -> Res<Value> {
        assert!(self.sql_context.is_none());

        self.next_state.sql.update_in_table(
            &self.state.txs[self.idx],
            &relation.name,
            update,
            condition,
        )?;

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
                x
            } else {
                panic!("expected to be a tuple")
            };
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
                let right = self.assert_set(right)?;
                Ok(Value::Bool(right.contains(&left)))
            }
            Operator::And => {
                let left = self.assert_bool(left)?;
                let right = self.assert_bool(right)?;
                Ok(Value::Bool(left && right))
            }
        }
    }
}
