use std::collections::HashMap;

use crate::engine::Value;
use crate::parser::{IsolationLevel, Item, SelectItem, SqlExpression, SqlOperator, Variable};
use crate::sql_interpreter::SqlEngineError::{SqlTypeError, UnknownVariable};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HashableRow {
    keys: Vec<String>,
    values: Vec<Value>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Row {
    pub tuples: HashMap<String, Value>,
    rid: RowId,
}

impl Row {
    pub fn to_value(&self, columns: &[String]) -> Value {
        if columns.len() == 1 {
            self.tuples.get(&columns[0]).unwrap().clone()
        } else {
            let mut res = vec![];
            for col in columns {
                res.push(self.tuples.get(col).unwrap().clone())
            }
            Value::Tuple(res)
        }
    }

    fn hash(self) -> HashableRow {
        let (keys, values): (Vec<String>, Vec<Value>) = self.tuples.into_iter().unzip();
        HashableRow { keys, values }
    }
}

#[derive(PartialEq, Debug, Clone)]
enum Changes {
    Insert(String, Row),
    Delete(String, Row),
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum Lock {
    RowUpdate(RowId),
    RowForKeyShare(RowId),
    Unique(String, UniqueIndex, Value),
}

impl Lock {
    fn conflicts(&self, existing_lock: &Self) -> bool {
        match self {
            Lock::RowUpdate(rid) => match existing_lock {
                Lock::RowUpdate(r) => r == rid,
                Lock::RowForKeyShare(r) => r == rid,
                Lock::Unique(_, _, _) => false,
            },
            Lock::RowForKeyShare(rid) => matches!(existing_lock, Lock::RowUpdate(r) if r == rid),
            Lock::Unique(_, _, _) => false,
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct TransactionContext {
    changes: Vec<Changes>,
    pub locks: Vec<Lock>,
}

impl TransactionContext {
    fn new() -> Self {
        TransactionContext {
            changes: vec![],
            locks: vec![],
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
enum SqlContext {
    Where {
        table: String,
        row: Row,
    },
    Update {
        tx: TransactionId,
        table: String,
        row: Row,
    },
}

#[derive(PartialEq, Default, Debug, Clone)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub unique: Vec<UniqueIndex>,
}

#[derive(PartialEq, Eq, Default, Debug, Clone, Hash)]
pub struct UniqueIndex {
    columns: Vec<String>,
}

#[derive(PartialEq, Eq, Default, Debug, Clone, Hash)]
pub struct ForeignKey {
    relation: String,
    columns: Vec<String>,
    foreign_relation: String,
    foreign_columns: Vec<String>,
}

impl UniqueIndex {
    fn tuple_from(&self, row: &Row) -> Value {
        let mut tuple = vec![];
        for c in &self.columns {
            tuple.push(row.tuples.get(c).unwrap().clone())
        }
        Value::Tuple(tuple)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct SqlDatabase {
    pub cur_tx: TransactionId,
    pub tables: HashMap<String, Table>,
    pub foreign_keys: Vec<ForeignKey>,
    pub transactions: HashMap<TransactionId, TransactionContext>,
    tx: TransactionId,
    rid: RowId,
    sql_context: Option<SqlContext>,
}

impl SqlDatabase {
    pub fn hash(&self) -> Vec<(String, Vec<HashableRow>)> {
        let mut res = vec![];
        for (name, table) in &self.tables {
            res.push((
                name.clone(),
                table.rows.iter().map(|row| row.clone().hash()).collect(),
            ));
        }
        res
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Eq, Hash)]
pub struct TransactionId(pub usize);

impl TransactionId {
    fn increment(&mut self) -> TransactionId {
        self.0 += 1;
        TransactionId(self.0)
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Hash, Eq)]
pub struct RowId(usize);

impl RowId {
    fn increment(&mut self) -> RowId {
        self.0 += 1;
        RowId(self.0)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum SqlEngineError {
    Locked(Lock),
    SqlTypeError(SqlExpression, String),
    UnicityViolation,
    ForeignKeyViolation,
    UnknownVariable(String),
}

type Res<T> = Result<T, SqlEngineError>;
type Unit = Res<()>;

impl SqlDatabase {
    pub fn new() -> SqlDatabase {
        SqlDatabase {
            cur_tx: TransactionId(usize::MAX),
            tables: Default::default(),
            foreign_keys: vec![],
            transactions: Default::default(),
            tx: TransactionId(0),
            sql_context: None,
            rid: RowId(0),
        }
    }

    pub fn open_transaction(&mut self, _isolation: IsolationLevel) -> TransactionId {
        let new_tx = self.tx.increment();
        self.transactions.insert(new_tx, TransactionContext::new());

        new_tx
    }

    pub fn execute(&mut self, expr: &SqlExpression, opt_tx: Option<TransactionId>) -> Res<Value> {
        self.cur_tx = if let Some(tx) = opt_tx {
            tx
        } else {
            self.open_transaction(IsolationLevel::ReadCommitted)
        };

        let res = self.interpret(expr)?;

        if opt_tx.is_none() {
            self.commit(&self.cur_tx.clone());
        }

        Ok(res)
    }

    fn interpret(&mut self, expr: &SqlExpression) -> Res<Value> {
        match expr {
            SqlExpression::Select {
                columns,
                from,
                condition,
                order_by,
                limit,
                offset,
                locking,
            } => self.interpret_select(
                columns,
                from,
                condition.as_deref().unwrap_or(&SqlExpression::Bool(true)),
                order_by.as_deref().unwrap_or(&SqlExpression::Integer(0)),
                *limit,
                offset.unwrap_or(0),
                *locking,
            ),
            SqlExpression::Delete {
                relation,
                condition,
            } => self.interpret_delete(
                relation,
                condition.as_deref().unwrap_or(&SqlExpression::Bool(true)),
            ),
            SqlExpression::Update {
                relation,
                updates,
                condition,
            } => self.interpret_update(
                relation,
                updates,
                condition.as_deref().unwrap_or(&SqlExpression::Bool(true)),
            ),
            SqlExpression::Insert {
                relation,
                columns,
                values,
            } => self.interpret_insert(relation, columns, values),
            SqlExpression::Create { relation, columns } => {
                let table = self.tables.entry(relation.name.clone()).or_default();
                table.unique.push(UniqueIndex {
                    columns: columns.iter().map(|c| c.name.clone()).collect(),
                });
                Ok(Value::Nil)
            }
            SqlExpression::Alter {
                relation,
                columns,
                reference_relation,
                reference_columns,
                ..
            } => {
                self.foreign_keys.push(ForeignKey {
                    relation: relation.name.clone(),
                    columns: columns.iter().map(|c| c.name.clone()).collect(),
                    foreign_relation: reference_relation.name.clone(),
                    foreign_columns: reference_columns.iter().map(|c| c.name.clone()).collect(),
                });
                Ok(Value::Nil)
            }
            SqlExpression::Binary {
                left,
                operator,
                right,
            } => self.interpret_binary(left, operator, right),
            SqlExpression::Integer(i) => Ok(Value::Integer(*i)),
            SqlExpression::Tuple(values) => {
                let mut res = vec![];
                for value in values {
                    res.push(self.interpret(value)?);
                }
                Ok(Value::Tuple(res))
            }
            SqlExpression::Var(var) => {
                if let Some(SqlContext::Where { row, .. }) = &self.sql_context {
                    Ok(row.tuples.get(&var.name).unwrap().clone())
                } else {
                    Err(UnknownVariable(var.name.clone()))
                }
            }
            SqlExpression::UpVariable(_) => panic!("UpVariable should not be interpreted directly"),
            SqlExpression::Value(value) => Ok(value.clone()),
            SqlExpression::Set(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?);
                }
                Ok(Value::Set(res))
            }
            SqlExpression::Assignment(_, _) => {
                panic!()
            }
            SqlExpression::String(s) => Ok(Value::String(s.clone())),
            SqlExpression::Bool(b) => Ok(Value::Bool(*b)),
            SqlExpression::Scalar(expr) => Ok(Value::Scalar(Box::new(self.interpret(expr)?))),
        }
    }

    fn interpret_binary(
        &mut self,
        left: &SqlExpression,
        operator: &SqlOperator,
        right: &SqlExpression,
    ) -> Res<Value> {
        match operator {
            SqlOperator::And => {
                let left = self.assert_bool(left)?;
                let right = self.assert_bool(right)?;
                Ok(Value::Bool(left && right))
            }
            SqlOperator::Add => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left + right))
            }
            SqlOperator::Subtract => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left - right))
            }
            SqlOperator::Multiply => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left * right))
            }
            SqlOperator::Divide => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left / right))
            }
            SqlOperator::Rem => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left % right))
            }
            SqlOperator::Equal => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(left == right))
            }
            SqlOperator::NotEqual => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(left != right))
            }
            SqlOperator::Less => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left < right))
            }
            SqlOperator::LessEqual => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left <= right))
            }
            SqlOperator::Greater => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left > right))
            }
            SqlOperator::GreaterEqual => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left >= right))
            }
            SqlOperator::In => {
                let left = self.interpret(left)?;
                let right = self.assert_set(right)?;
                Ok(Value::Bool(right.contains(&left)))
            }
            SqlOperator::Between => {
                if let SqlExpression::Tuple(tuples) = (*right).clone() {
                    let left = self.assert_integer(left)?;
                    let lower = self.assert_integer(&tuples[0])?;
                    let upper = self.assert_integer(&tuples[1])?;
                    Ok(Value::Bool(left >= lower && left <= upper))
                } else {
                    panic!()
                }
            }
        }
    }

    fn interpret_insert(
        &mut self,
        relation: &Variable,
        columns: &[Variable],
        exprs: &[SqlExpression],
    ) -> Res<Value> {
        let mut values = vec![];
        for expr in exprs {
            values.push(self.assert_tuple(expr)?)
        }

        let table = &relation.name;
        for value in values {
            let mut new_tuples = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                new_tuples.insert(col.name.clone(), value[i].clone());
            }
            let new_row = Row {
                tuples: new_tuples,
                rid: self.rid.increment(),
            };
            self.check_unique_values(table, &new_row)?;
            let foreign_rows = self.check_foreign_key(table, &new_row)?;

            for rid in foreign_rows {
                self.request_row_lock(Lock::RowForKeyShare(rid))?;
            }

            let transaction = self.transactions.get_mut(&self.cur_tx).unwrap();

            let t = self.tables.entry(table.clone()).or_default();
            if t.columns.is_empty() {
                t.columns = columns.iter().map(|c| c.name.clone()).collect();
            }
            for unique in &t.unique {
                transaction.locks.push(Lock::Unique(
                    table.clone(),
                    unique.clone(),
                    unique.tuple_from(&new_row),
                ));
            }

            transaction
                .changes
                .push(Changes::Insert(table.to_string(), new_row));
        }
        Ok(Value::Nil)
    }

    fn interpret_delete(&mut self, relation: &Variable, condition: &SqlExpression) -> Res<Value> {
        let table = &relation.name;
        let rows = self.rows(&self.cur_tx, table);

        let mut mutated = 0;
        for row in &rows {
            self.sql_context = Some(SqlContext::Where {
                row: row.clone(),
                table: table.clone(),
            });
            if self.interpret(condition)? == Value::Bool(true) {
                self.sql_context = Some(SqlContext::Update {
                    tx: self.cur_tx,
                    row: row.clone(),
                    table: table.clone(),
                });

                let mut cascade_rows = vec![];
                for foreign_key in &self.foreign_keys {
                    if &foreign_key.foreign_relation == table {
                        let foreign_rows = self.rows(&self.cur_tx, &foreign_key.relation);
                        for cascade_row in foreign_rows {
                            let mut p = foreign_key
                                .foreign_columns
                                .iter()
                                .zip(foreign_key.columns.iter());
                            if p.all(|(col, f_col)| {
                                row.tuples.get(col).unwrap()
                                    == cascade_row.tuples.get(f_col).unwrap()
                            }) {
                                cascade_rows.push((foreign_key.relation.clone(), cascade_row));
                            }
                        }
                    }
                }

                self.request_row_lock(Lock::RowUpdate(row.rid))?;

                let transaction = self.transactions.get_mut(&self.cur_tx).unwrap();

                transaction
                    .changes
                    .push(Changes::Delete(table.clone(), row.clone()));
                for (f_table, cascade_row) in cascade_rows {
                    transaction
                        .changes
                        .push(Changes::Delete(f_table, cascade_row.clone()));
                }

                mutated += 1;
            }
            self.sql_context = None;
        }

        Ok(Value::Integer(mutated))
    }

    fn interpret_update(
        &mut self,
        relation: &Variable,
        updates: &[SqlExpression],
        condition: &SqlExpression,
    ) -> Res<Value> {
        let table = &relation.name;
        let rows = self.rows(&self.cur_tx, table);

        let mut mutated = 0;
        for row in rows {
            self.sql_context = Some(SqlContext::Where {
                row: row.clone(),
                table: table.clone(),
            });
            if self.interpret(condition)? == Value::Bool(true) {
                self.sql_context = Some(SqlContext::Update {
                    tx: self.cur_tx,
                    row: row.clone(),
                    table: table.clone(),
                });
                self.updates(updates, table, &row)?;
                mutated += 1;
            }
            self.sql_context = None;
        }

        Ok(Value::Integer(mutated))
    }

    #[allow(clippy::too_many_arguments)]
    fn interpret_select(
        &mut self,
        item_list: &[SelectItem],
        from: &Variable,
        condition: &SqlExpression,
        order_by: &SqlExpression,
        limit: Option<i16>,
        offset: i16,
        for_update: bool,
    ) -> Res<Value> {
        let rows = self.rows(&self.cur_tx, &from.name);

        let mut res = vec![];
        for row in &rows {
            self.sql_context = Some(SqlContext::Where {
                row: row.clone(),
                table: from.name.clone(),
            });
            if for_update {
                self.request_row_lock(Lock::RowUpdate(row.rid))?;
            }
            if self.interpret(condition)? == Value::Bool(true) {
                res.push(row)
            }
            self.sql_context = None;
        }

        res.sort_by(|left, right| {
            self.sql_context = Some(SqlContext::Where {
                row: (*left).clone(),
                table: from.name.clone(),
            });
            let l = self.interpret(order_by).unwrap();

            self.sql_context = Some(SqlContext::Where {
                row: (*right).clone(),
                table: from.name.clone(),
            });
            let r = self.interpret(order_by).unwrap();

            Ord::cmp(&l, &r)
        });

        res = res.into_iter().skip(offset as usize).collect();
        if let Some(l) = limit {
            res = res.into_iter().take(l as usize).collect();
        }

        if item_list
            .iter()
            .any(|col| matches!(col, SelectItem::Count(_)))
        {
            Ok(Value::Integer(res.len() as i16))
        } else {
            let mut values = vec![];
            let table = self.tables.get(&from.name).cloned().unwrap_or_default();
            let mut selected_columns = vec![];
            for col in item_list {
                match col {
                    SelectItem::Column(item) => match item {
                        Item::Wildcard => selected_columns.extend(table.columns.clone()),
                        Item::Column(col) => selected_columns.push(col.clone()),
                    },
                    SelectItem::Count(_) => panic!(),
                }
            }
            for r in res {
                values.push(r.to_value(&selected_columns));
            }

            if values.len() == 1 {
                return Ok(values[0].clone());
            }

            Ok(Value::Set(values))
        }
    }

    fn assert_integer(&mut self, expr: &SqlExpression) -> Res<i16> {
        let value = self.interpret(expr)?;
        if let Value::Integer(value) = value {
            Ok(value)
        } else if let Value::Scalar(boxed) = &value {
            if let Value::Integer(i) = *(*boxed) {
                Ok(i)
            } else {
                Err(SqlTypeError(expr.clone(), "integer".to_string()))
            }
        } else {
            Err(SqlTypeError(expr.clone(), "integer".to_string()))
        }
    }

    fn assert_bool(&mut self, expr: &SqlExpression) -> Res<bool> {
        let value = self.interpret(expr)?;
        if let Value::Bool(value) = value {
            Ok(value)
        } else if let Value::Scalar(boxed) = &value {
            if let Value::Bool(b) = *(*boxed) {
                Ok(b)
            } else {
                Err(SqlTypeError(expr.clone(), "integer".to_string()))
            }
        } else {
            Err(SqlTypeError(expr.clone(), "bool".to_string()))
        }
    }

    fn assert_tuple(&mut self, expr: &SqlExpression) -> Res<Vec<Value>> {
        if let Value::Tuple(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "tuple".to_string()))
        }
    }

    fn assert_set(&mut self, expr: &SqlExpression) -> Res<Vec<Value>> {
        if let Value::Set(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "set".to_string()))
        }
    }

    fn rows(&self, tx: &TransactionId, table_name: &String) -> Vec<Row> {
        let mut table = self.tables.get(table_name).cloned().unwrap_or_default();

        let transaction = self.transactions.get(tx).unwrap();
        for changes in &transaction.changes {
            match changes {
                Changes::Insert(insert_table, insert_row) => {
                    if insert_table == table_name {
                        table.rows.push(insert_row.clone());
                    }
                }
                Changes::Delete(delete_table, row) => {
                    if delete_table == table_name {
                        table.rows.retain(|x| x != row);
                    }
                }
            }
        }
        table.rows
    }

    pub fn commit(&mut self, tx: &TransactionId) {
        let tx = self.transactions.remove(tx).unwrap();
        for change in tx.changes {
            match change {
                Changes::Insert(table, row) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    table.rows.push(row);
                }
                Changes::Delete(table, row) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    table.rows.retain(|x| x != &row);
                }
            }
        }
    }

    pub fn abort(&mut self, tx: &TransactionId) {
        self.transactions.remove(tx).unwrap();
    }

    fn updates(&mut self, updates: &[SqlExpression], table: &String, row: &Row) -> Unit {
        let mut new_row = self.execute_assignment(row, table, &updates[0])?;
        for update in &updates[1..] {
            new_row = self.execute_assignment(&new_row, table, update)?;
        }

        self.check_unique_values(table, &new_row)?;

        let foreign_rows = self.check_foreign_key(table, &new_row)?;
        for rid in foreign_rows {
            self.request_row_lock(Lock::RowForKeyShare(rid))?;
        }
        self.request_row_lock(Lock::RowUpdate(row.rid))?;

        let transaction = self.transactions.get_mut(&self.cur_tx).unwrap();

        let t = self.tables.get(table).unwrap();
        for unique in &t.unique {
            transaction.locks.push(Lock::Unique(
                table.clone(),
                unique.clone(),
                unique.tuple_from(&new_row),
            ));
        }
        transaction
            .changes
            .push(Changes::Delete(table.clone(), row.clone()));
        transaction
            .changes
            .push(Changes::Insert(table.clone(), new_row.clone()));

        Ok(())
    }

    fn execute_assignment(&mut self, row: &Row, table: &String, expr: &SqlExpression) -> Res<Row> {
        if let SqlExpression::Assignment(name, expr) = expr {
            let value = self.interpret(expr)?;
            let t = self.tables.get(table).unwrap();
            let mut new_tuples = HashMap::new();
            for col in &t.columns {
                if col == &name.name {
                    new_tuples.insert(name.name.clone(), value.clone());
                } else {
                    new_tuples.insert(col.clone(), row.tuples.get(col).unwrap().clone());
                }
            }
            Ok(Row {
                tuples: new_tuples,
                rid: row.rid,
            })
        } else {
            panic!("{expr}")
        }
    }

    fn request_row_lock(&mut self, requested_lock: Lock) -> Unit {
        for (id, t) in &self.transactions {
            if id != &self.cur_tx && t.locks.iter().any(|l| requested_lock.conflicts(l)) {
                return Err(SqlEngineError::Locked(requested_lock));
            }
        }
        let tx = self.transactions.get_mut(&self.cur_tx).unwrap();
        tx.locks.push(requested_lock);
        Ok(())
    }

    fn check_unique_values(&self, table: &str, row: &Row) -> Unit {
        for (id, tc) in &self.transactions {
            if id == &self.cur_tx {
                continue;
            }
            for lock in &tc.locks {
                if let Lock::Unique(t, unique, value) = &lock {
                    if t == table && &unique.tuple_from(row) == value {
                        return Err(SqlEngineError::Locked(lock.clone()));
                    }
                }
            }
        }

        if let Some(t) = self.tables.get(table) {
            for unique in &t.unique {
                for existing in &t.rows {
                    if unique.tuple_from(existing) == unique.tuple_from(row) {
                        return Err(SqlEngineError::UnicityViolation);
                    }
                }
            }
        }
        Ok(())
    }

    fn check_foreign_key(&self, table: &str, row: &Row) -> Res<Vec<RowId>> {
        let mut res = vec![];
        'outer: for foreign_key in &self.foreign_keys {
            if foreign_key.relation == table {
                let foreign_rows = self.rows(&self.cur_tx, &foreign_key.foreign_relation);
                for foreign_row in foreign_rows {
                    let mut p = foreign_key
                        .columns
                        .iter()
                        .zip(foreign_key.foreign_columns.iter());
                    if p.all(|(col, f_col)| {
                        row.tuples.get(col).unwrap() == foreign_row.tuples.get(f_col).unwrap()
                    }) {
                        res.push(foreign_row.rid);
                        continue 'outer;
                    }
                }
                return Err(SqlEngineError::ForeignKeyViolation);
            }
        }
        Ok(res)
    }
}
