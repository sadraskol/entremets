use crate::engine::{PropertyCheck, Transaction, TransactionState, Value};
use crate::interpreter::InterpreterError::{TypeError, Unexpected};
use crate::parser::{Expression, Operator, SqlExpression, Statement};
use crate::sql_interpreter::{SqlEngineError, TransactionId};
use crate::state::{ProcessState, RcState, State};

#[derive(Debug)]
pub enum InterpreterError {
    Unexpected(String),
    TypeError(Expression, Value, String),
    SqlEngineError(SqlEngineError),
}

impl From<SqlEngineError> for InterpreterError {
    fn from(value: SqlEngineError) -> Self {
        InterpreterError::SqlEngineError(value)
    }
}

type Res<T> = Result<T, InterpreterError>;

pub struct Interpreter {
    pub idx: usize,
    checking: bool,
    state: RcState,
    next_state: State,
}

impl Interpreter {
    pub fn new(state: RcState) -> Self {
        Interpreter {
            idx: 0,
            checking: false,
            state: state.clone(),
            next_state: state.borrow().clone(),
        }
    }

    pub fn next_state(&mut self) -> State {
        std::mem::replace(&mut self.next_state, self.state.borrow().clone())
    }

    pub fn check_property(&mut self, property: &Statement) -> Res<PropertyCheck> {
        self.checking = true;
        let res = match property {
            Statement::Always(always) => {
                let value = self.interpret(always)?;
                Ok(PropertyCheck::Always(value == Value::Bool(true)))
            }
            Statement::Eventually(eventually) => {
                let value = self.interpret(eventually)?;
                Ok(PropertyCheck::Eventually(value == Value::Bool(true)))
            }
            Statement::Never(never) => {
                let value = self.interpret(never)?;
                Ok(PropertyCheck::Always(value == Value::Bool(false)))
            }
            _ => Err(Unexpected(format!("unsupported property: {property:?}"))),
        };

        self.checking = false;
        res
    }

    pub fn statement(&mut self, statement: &Statement) -> Res<usize> {
        match self.priv_statement(statement) {
            Err(InterpreterError::SqlEngineError(SqlEngineError::UnicityViolation)) => Ok(1),
            Err(InterpreterError::SqlEngineError(SqlEngineError::ForeignKeyViolation)) => Ok(1),
            Err(InterpreterError::SqlEngineError(SqlEngineError::Locked(lock))) => {
                self.next_state.processes[self.idx] = ProcessState::Locked(lock);
                Ok(0)
            }
            other => other,
        }
    }

    fn priv_statement(&mut self, statement: &Statement) -> Res<usize> {
        match statement {
            Statement::Begin(isolation, tx_name) => {
                self.next_state.txs[self.idx].name = tx_name.as_ref().map(|v| v.name.clone());
                let id = self.next_state.sql.open_transaction(*isolation);
                self.next_state.txs[self.idx].id = Some(id);
                self.next_state.txs[self.idx].state = TransactionState::Running;

                if let Some(tx) = tx_name {
                    self.next_state.locals.insert(
                        tx.name.clone(),
                        Value::Tx(Transaction(TransactionState::Running)),
                    );
                }
            }
            Statement::Commit => {
                if self.next_state.txs[self.idx].state == TransactionState::Running {
                    self.next_state
                        .sql
                        .commit(&self.next_state.txs[self.idx].id.unwrap());
                    self.next_state.txs.get_mut(self.idx).unwrap().id = None;

                    if let Some(tx) = &self.next_state.txs[self.idx].name {
                        self.next_state.locals.insert(
                            tx.clone(),
                            Value::Tx(Transaction(TransactionState::Committed)),
                        );
                    }
                    self.next_state.txs[self.idx].state = TransactionState::Committed;
                }
            }
            Statement::Abort => {
                self.next_state
                    .sql
                    .abort(&self.next_state.txs[self.idx].id.unwrap());
                self.next_state.txs[self.idx].id = None;

                if let Some(tx) = &&self.next_state.txs[self.idx].name {
                    self.next_state.locals.insert(
                        tx.clone(),
                        Value::Tx(Transaction(TransactionState::Aborted)),
                    );
                }
                self.next_state.txs[self.idx].state = TransactionState::Aborted;
            }
            Statement::Expression(expr) => {
                self.interpret(expr)?;
            }
            Statement::Latch => {
                self.next_state.processes[self.idx] = ProcessState::Latching;
            }
            Statement::If(expr, offset) => {
                let cond = self.assert_bool(expr)?;
                if !cond {
                    return Ok(offset.get());
                }
            }
            Statement::Else(offset) => {
                return Ok(offset.get());
            }
            _ => panic!("Unexpected statement in process: {statement:?}"),
        };
        Ok(1)
    }

    fn interpret(&mut self, expression: &Expression) -> Res<Value> {
        match expression {
            Expression::Sql(sql_expr) => {
                let reified = self.reify_up_variable(sql_expr)?;
                Ok(self.next_state.sql.execute(&reified, self.running_tx())?)
            }
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
            Expression::Var(variable) => Ok(self
                .state
                .borrow()
                .locals
                .get(&variable.name)
                .cloned()
                .unwrap_or(Value::Tx(Transaction(TransactionState::NotExisting)))),
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
            Expression::Member { call_site, member } => {
                let target = self.assert_transaction(call_site)?;
                match target.0 {
                    TransactionState::NotExisting => Ok(Value::Bool(false)),
                    TransactionState::Running => Ok(Value::Bool(false)),
                    TransactionState::Aborted => Ok(Value::Bool(member.name == "aborted")),
                    TransactionState::Committed => Ok(Value::Bool(member.name == "committed")),
                }
            }
            Expression::String(s) => Ok(Value::String(s.clone())),
            Expression::Scalar(expr) => Ok(Value::Scalar(Box::new(self.interpret(expr)?))),
        }
    }

    fn assert_transaction(&mut self, expr: &Expression) -> Res<Transaction> {
        let value = self.interpret(expr)?;
        if let Value::Tx(value) = value {
            Ok(value)
        } else {
            Err(TypeError(
                expr.clone(),
                value.clone(),
                "transaction".to_string(),
            ))
        }
    }

    fn assert_integer(&mut self, expr: &Expression) -> Res<i16> {
        let value = self.interpret(expr)?;
        if let Value::Integer(value) = value {
            Ok(value)
        } else if let Value::Scalar(boxed) = &value {
            if let Value::Integer(i) = *(*boxed) {
                Ok(i)
            } else {
                Err(TypeError(expr.clone(), value, "integer".to_string()))
            }
        } else {
            Err(TypeError(expr.clone(), value, "integer".to_string()))
        }
    }

    fn assert_set(&mut self, expr: &Expression) -> Res<Vec<Value>> {
        let value = self.interpret(expr)?;
        if let Value::Set(value) = value {
            Ok(value)
        } else {
            Err(TypeError(expr.clone(), value, "set".to_string()))
        }
    }

    fn assert_bool(&mut self, expr: &Expression) -> Res<bool> {
        let value = self.interpret(expr)?;
        if let Value::Bool(value) = value {
            Ok(value)
        } else if let Value::Scalar(boxed) = &value {
            if let Value::Bool(b) = *(*boxed) {
                Ok(b)
            } else {
                Err(TypeError(expr.clone(), value, "bool".to_string()))
            }
        } else {
            Err(TypeError(expr.clone(), value, "bool".to_string()))
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
            Operator::Subtract => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left - right))
            }
            Operator::Multiply => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left * right))
            }
            Operator::Divide => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left / right))
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
            Operator::Or => {
                let left = self.assert_bool(left)?;
                let right = self.assert_bool(right)?;
                Ok(Value::Bool(left || right))
            }
            Operator::Greater => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left > right))
            }
            Operator::GreaterEqual => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left >= right))
            }
            Operator::NotEqual => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(right != left))
            }
        }
    }
    fn reify_up_variable(&self, expr: &SqlExpression) -> Res<SqlExpression> {
        match expr {
            SqlExpression::Select {
                columns,
                from,
                condition,
                order_by,
                limit,
                offset,
                locking,
            } => {
                let condition = if let Some(cond) = condition {
                    Some(Box::new(self.reify_up_variable(cond)?))
                } else {
                    None
                };
                Ok(SqlExpression::Select {
                    columns: columns.clone(),
                    from: from.clone(),
                    order_by: order_by.clone(),
                    limit: *limit,
                    offset: *offset,
                    condition,
                    locking: *locking,
                })
            }
            SqlExpression::Update {
                relation,
                updates,
                condition,
            } => {
                let condition = if let Some(cond) = condition {
                    Some(Box::new(self.reify_up_variable(cond)?))
                } else {
                    None
                };
                let mut res = vec![];
                for update in updates {
                    res.push(self.reify_up_variable(update)?);
                }
                Ok(SqlExpression::Update {
                    relation: relation.clone(),
                    updates: res,
                    condition,
                })
            }
            SqlExpression::Insert {
                relation,
                columns,
                values,
            } => {
                let mut res = vec![];
                for value in values {
                    res.push(self.reify_up_variable(value)?);
                }
                Ok(SqlExpression::Insert {
                    relation: relation.clone(),
                    columns: columns.clone(),
                    values: res,
                })
            }
            SqlExpression::Binary {
                left,
                operator,
                right,
            } => Ok(SqlExpression::Binary {
                left: Box::new(self.reify_up_variable(left)?),
                operator: operator.clone(),
                right: Box::new(self.reify_up_variable(right)?),
            }),
            SqlExpression::Tuple(values) => {
                let mut res = vec![];
                for value in values {
                    res.push(self.reify_up_variable(value)?);
                }
                Ok(SqlExpression::Tuple(res))
            }
            SqlExpression::Assignment(var, expr) => Ok(SqlExpression::Assignment(
                var.clone(),
                Box::new(self.reify_up_variable(expr)?),
            )),
            SqlExpression::UpVariable(variable) => Ok(SqlExpression::Value(
                self.state
                    .borrow()
                    .locals
                    .get(&variable.name)
                    .cloned()
                    .unwrap_or(Value::Nil),
            )),
            expr => Ok(expr.clone()),
        }
    }

    fn running_tx(&self) -> Option<TransactionId> {
        if self.checking {
            None
        } else {
            self.state
                .borrow()
                .txs
                .get(self.idx)
                .and_then(|info| info.id)
        }
    }
}
