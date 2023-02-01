use crate::engine::{ProcessState, PropertyCheck, State, Value};
use crate::interpreter::InterpreterError::{TypeError, Unexpected};
use crate::parser::{Expression, Operator, Statement, Variable};
use crate::sql_engine::SqlEngineError;

#[derive(Debug)]
pub enum InterpreterError {
    Unexpected(String),
    TypeError(Expression, String),
    SqlEngineError(SqlEngineError),
}

impl From<SqlEngineError> for InterpreterError {
    fn from(value: SqlEngineError) -> Self {
        InterpreterError::SqlEngineError(value)
    }
}

type Res<T> = Result<T, InterpreterError>;
type Unit = Res<()>;

pub struct Interpreter {
    pub idx: usize,
    state: State,
    next_state: State,
}

impl Interpreter {
    pub fn new(state: &State) -> Self {
        Interpreter {
            idx: 0,
            state: state.clone(),
            next_state: state.clone(),
        }
    }

    pub fn reset(&mut self) -> State {
        std::mem::replace(&mut self.next_state, self.state.clone())
    }

    pub fn check_property(&mut self, property: &Statement) -> Res<PropertyCheck> {
        match property {
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
        }
    }

    pub fn statement(&mut self, statement: &Statement) -> Unit {
        match statement {
            Statement::Begin(isolation, _tx_name) => {
                self.next_state.txs[self.idx] =
                    Some(self.next_state.sql.open_transaction(*isolation));
            }
            Statement::Commit => {
                self.next_state
                    .sql
                    .commit(&self.next_state.txs[self.idx].unwrap());
                self.next_state.txs[self.idx] = None
            }
            Statement::Abort => {
                self.next_state
                    .sql
                    .abort(&self.next_state.txs[self.idx].unwrap());
                self.next_state.txs[self.idx] = None
            }
            Statement::Expression(expr) => {
                self.interpret(expr)?;
            }
            Statement::Latch => {
                self.next_state.state[self.idx] = ProcessState::Waiting;
            }
            _ => panic!("Unexpected statement in process: {statement:?}"),
        };
        Ok(())
    }

    fn interpret(&mut self, expression: &Expression) -> Res<Value> {
        match expression {
            Expression::Select {
                columns,
                from,
                condition,
                locking,
            } => self.interpret_select(columns, from, condition, *locking),
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
            Expression::Var(variable) => Ok(self
                .state
                .locals
                .get(&variable.name)
                .cloned()
                .unwrap_or(Value::Nil)),
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
            Expression::Value(_) => panic!(),
        }
    }

    fn interpret_insert(
        &mut self,
        relation: &Variable,
        columns: &[Variable],
        exprs: &[Expression],
    ) -> Res<Value> {
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
        let update_sql = self.sql_update_expression(update)?;
        self.next_state.sql.update_in_table(
            &self.state.txs[self.idx],
            &relation.name,
            &update_sql,
            condition,
        )?;

        Ok(Value::Nil)
    }

    fn interpret_select(
        &mut self,
        columns: &[Variable],
        from: &Variable,
        condition: &Option<Box<Expression>>,
        locking: bool,
    ) -> Res<Value> {
        let res = self.next_state.sql.select_in_table(
            &self.state.txs[self.idx],
            columns,
            &from.name,
            condition,
            locking,
        )?;

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
            Operator::Is => {
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

    fn sql_translate(&mut self, expr: &mut Expression) -> Unit {
        match expr {
            Expression::Assignment(_x, y) => {
                self.sql_translate(y)?;
            }
            Expression::Var(variable) => {
                let y = self
                    .state
                    .locals
                    .get(&variable.name)
                    .cloned()
                    .unwrap_or(Value::Nil);
                *expr = Expression::Value(y);
            }
            Expression::Binary { left, right, .. } => {
                self.sql_translate(left)?;
                self.sql_translate(right)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn sql_update_expression(&mut self, expr: &Expression) -> Res<Expression> {
        let mut sql_expr = expr.clone();
        self.sql_translate(&mut sql_expr)?;
        Ok(sql_expr.clone())
    }
}
