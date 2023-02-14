use std::fmt::Debug;
use std::fmt::Formatter;
use std::mem;
use std::num::ParseIntError;
use std::str::FromStr;

use crate::engine::Value;
use crate::format::intersperse;
use crate::parser::ParserErrorKind::Unexpected;
use crate::scanner::{Scanner, ScannerError, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Begin(IsolationLevel, Option<Variable>),
    Commit,
    Abort,
    Expression(Expression),
    Latch,

    Always(Expression),
    Never(Expression),
    Eventually(Expression),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsolationLevel {
    ReadCommitted,
}

impl std::fmt::Display for IsolationLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            IsolationLevel::ReadCommitted => f.write_str("read committed"),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Variable {
    pub name: String,
}

impl std::fmt::Display for Variable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum SqlExpression {
    Select {
        columns: Vec<Variable>,
        from: Variable,
        condition: Option<Box<SqlExpression>>,
        locking: bool,
    },
    Update {
        relation: Variable,
        update: Box<SqlExpression>,
        condition: Option<Box<SqlExpression>>,
    },
    Insert {
        relation: Variable,
        columns: Vec<Variable>,
        values: Vec<SqlExpression>,
    },
    Binary {
        left: Box<SqlExpression>,
        operator: SqlOperator,
        right: Box<SqlExpression>,
    },
    Tuple(Vec<SqlExpression>),
    Assignment(Variable, Box<SqlExpression>),
    Set(Vec<SqlExpression>),
    Var(Variable),
    Integer(i16),
    UpVariable(Variable),
    // UpVariables are translated to value
    Value(Value),
}

#[derive(PartialEq, Debug, Clone)]
pub enum Expression {
    Sql(SqlExpression),
    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
    },
    Member {
        call_site: Box<Expression>,
        member: Variable,
    },
    Assignment(Variable, Box<Expression>),
    Var(Variable),
    Integer(i16),
    Set(Vec<Expression>),
    Tuple(Vec<Expression>),
}

#[derive(PartialEq, Debug, Clone)]
pub enum Operator {
    Add,
    Multiply,
    Rem,
    Equal,
    LessEqual,
    Less,
    Included,
    And,
    Or,
}

#[derive(PartialEq, Debug, Clone)]
pub enum SqlOperator {
    Add,
    Multiply,
    Rem,
    Equal,
    In,
    And,
}

pub struct Parser {
    scanner: Scanner,
    manual_commit: bool,
    previous: Token,
    current: Token,
    result: Mets,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParserErrorKind {
    ParseInt(ParseIntError),
    Scanner(ScannerError),
    Unexpected(String),
}

impl From<ScannerError> for ParserErrorKind {
    fn from(value: ScannerError) -> Self {
        ParserErrorKind::Scanner(value)
    }
}

impl From<ParseIntError> for ParserErrorKind {
    fn from(value: ParseIntError) -> Self {
        ParserErrorKind::ParseInt(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParserError {
    pub current: Token,
    pub kind: ParserErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mets {
    pub init: Vec<Statement>,
    pub processes: Vec<Vec<Statement>>,
    pub properties: Vec<Statement>,
}

pub type Res<T> = Result<T, ParserErrorKind>;
type Unit = Res<()>;

impl Parser {
    pub fn new(source: String) -> Self {
        Parser {
            scanner: Scanner::new(source),
            manual_commit: false,
            previous: Token::uninitialized(),
            current: Token::uninitialized(),
            result: Mets {
                init: vec![],
                processes: vec![],
                properties: vec![],
            },
        }
    }

    pub fn compile(mut self) -> Result<Mets, Box<ParserError>> {
        match self.private_compile() {
            Ok(_) => Ok(self.result),
            Err(kind) => Err(Box::new(ParserError {
                current: self.current,
                kind,
            })),
        }
    }

    fn private_compile(&mut self) -> Unit {
        self.advance()?;
        self.skip_newlines()?;
        while !self.matches(TokenKind::Eof)? {
            self.declaration()?;
        }
        self.consume(TokenKind::Eof, "Expected end of expression")
    }

    fn advance(&mut self) -> Unit {
        mem::swap(&mut self.previous, &mut self.current);
        self.current = self.scanner.scan_token()?;

        Ok(())
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.current.kind == kind
    }

    fn matches(&mut self, kind: TokenKind) -> Res<bool> {
        Ok(if self.current.kind == kind {
            self.advance()?;
            true
        } else {
            false
        })
    }

    fn matches_forward(&mut self, kind: TokenKind) -> Res<bool> {
        let mut clone = self.scanner.clone();
        let mut advances = 1;
        let mut current = self.current.clone();
        loop {
            let Token {
                kind: next_kind, ..
            } = current;
            if next_kind == TokenKind::Newline {
                advances += 1;
            } else if next_kind == kind {
                for _ in 0..advances {
                    self.advance()?;
                }
                return Ok(true);
            } else {
                return Ok(false);
            }
            current = clone.scan_token()?;
        }
    }

    fn consume(&mut self, kind: TokenKind, expected: &str) -> Unit {
        if self.current.kind == kind {
            self.advance()
        } else {
            Err(Unexpected(expected.to_string()))
        }
    }

    fn declaration(&mut self) -> Unit {
        if self.matches(TokenKind::Process)? {
            self.process_declaration()
        } else if self.matches(TokenKind::Init)? {
            self.init_declaration()
        } else if self.matches(TokenKind::Property)? {
            self.property_declaration()
        } else {
            Err(Unexpected(format!(
                "Expected either process, init or property. Parsed {:?} instead",
                self.current.kind
            )))
        }
    }

    fn init_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expected do after init declaration")?;
        self.consume(
            TokenKind::Newline,
            "Expected newline after init declaration",
        )?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            self.statement(&mut statements)?;
        }
        self.result.init = statements;

        self.consume(
            TokenKind::End,
            "Expected end at the end of init declaration",
        )?;

        self.end_line()
    }

    fn process_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expected do after process declaration")?;
        self.consume(
            TokenKind::Newline,
            "Expected newline after process declaration",
        )?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            self.statement(&mut statements)?;
        }
        self.result.processes.push(statements);

        self.consume(
            TokenKind::End,
            "Expected end at the end of process declaration",
        )?;

        self.end_line()
    }

    fn end_line(&mut self) -> Unit {
        if self.current.kind != TokenKind::Eof {
            self.consume(TokenKind::Newline, "Expected newline after declaration")?;
        }
        self.skip_newlines()
    }

    fn property_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Equal, "Expected = after property declaration")?;

        let mut statements = vec![];
        self.statement(&mut statements)?;
        self.result.properties.push(statements.remove(0));

        Ok(())
    }

    fn statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        if self.matches(TokenKind::Let)? {
            self.assignment_statement(writer)?;
        } else if self.matches(TokenKind::Transaction)? {
            self.transaction_statement(writer)?;
        } else if self.matches(TokenKind::Begin)? {
            self.begin_statement(writer)?;
        } else if self.matches(TokenKind::Commit)? {
            self.commit_statement(writer)?;
        } else if self.matches(TokenKind::Abort)? {
            self.abort_statement(writer)?;
        } else if self.matches(TokenKind::Latch)? {
            self.latch_statement(writer)?;
        } else if self.matches(TokenKind::Always)? {
            self.always_statement(writer)?;
        } else if self.matches(TokenKind::Never)? {
            self.never_statement(writer)?;
        } else if self.matches(TokenKind::Eventually)? {
            self.eventually_statement(writer)?;
        } else {
            self.expression_statement(writer)?;
        };

        self.end_line()
    }

    fn assignment_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let expr = self.assignment()?;

        writer.push(Statement::Expression(expr));
        Ok(())
    }

    fn transaction_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let tx_name = self.parse_variable("Expected transaction name")?;
        self.consume(
            TokenKind::Identifier,
            "Expected isolation level after transaction name",
        )?;

        match self.previous.lexeme.as_str() {
            "read_committed" => {
                self.consume(TokenKind::Do, "Expected block after transaction statement")?;
                self.end_line()?;

                writer.push(Statement::Begin(
                    IsolationLevel::ReadCommitted,
                    Some(tx_name),
                ));
                self.manual_commit = false;

                while self.current.kind != TokenKind::End {
                    self.statement(writer)?;
                }

                self.consume(TokenKind::End, "Expected to close transaction block")?;

                if !self.manual_commit {
                    writer.push(Statement::Commit);
                }
                Ok(())
            }
            _ => Err(Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn parse_variable(&mut self, expected: &str) -> Res<Variable> {
        self.consume(TokenKind::Identifier, expected)?;

        self.make_variable()
    }

    fn begin_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(
            TokenKind::Identifier,
            "Expected isolation level after begin",
        )?;

        match self.previous.lexeme.as_str() {
            "read_committed" => {
                writer.push(Statement::Begin(IsolationLevel::ReadCommitted, None));
                Ok(())
            }
            _ => Err(Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn commit_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Commit);
        self.manual_commit = true;
        Ok(())
    }

    fn abort_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Abort);
        self.manual_commit = true;
        Ok(())
    }

    fn latch_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Latch);
        Ok(())
    }

    fn always_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(
            TokenKind::LeftBracket,
            "Expected [ to open always statement",
        )?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightBracket,
            "Expected ] to close always statement",
        )?;

        writer.push(Statement::Always(expr));
        Ok(())
    }

    fn never_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(TokenKind::LeftBracket, "Expected [ to open never statement")?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightBracket,
            "Expected ] to close never statement",
        )?;
        writer.push(Statement::Never(expr));
        Ok(())
    }

    fn eventually_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(
            TokenKind::LeftCarret,
            "Expected < to open eventually statement",
        )?;

        let expr = self.expression()?;

        self.skip_newlines()?;
        self.consume(
            TokenKind::RightCarret,
            "Expected > to close eventually statement",
        )?;
        writer.push(Statement::Eventually(expr));
        Ok(())
    }

    fn expression_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let expr = self.expression()?;
        writer.push(Statement::Expression(expr));
        Ok(())
    }

    fn expression(&mut self) -> Res<Expression> {
        self.assignment()
    }

    fn assignment(&mut self) -> Res<Expression> {
        let mut expr = self.or()?;

        if self.matches(TokenKind::ColonEqual)? {
            let name = if let Expression::Var(name) = expr {
                name
            } else {
                return Err(Unexpected(format!(
                    "Expected variable before := assignment at {:?}",
                    self.previous
                )));
            };
            let value = self.assignment()?;
            expr = Expression::Assignment(name, Box::new(value));
        }

        Ok(expr)
    }

    fn or(&mut self) -> Res<Expression> {
        let mut expr = self.and()?;

        if self.matches_forward(TokenKind::Or)? {
            let right = self.or()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Or,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn and(&mut self) -> Res<Expression> {
        let mut expr = self.included()?;

        if self.matches_forward(TokenKind::And)? {
            let right = self.and()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::And,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_assignment(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_and()?;

        if self.matches(TokenKind::ColonEqual)? {
            let name = if let SqlExpression::Var(name) = expr {
                name
            } else {
                return Err(Unexpected(format!(
                    "Expected variable before := assignment at {:?}",
                    self.previous
                )));
            };
            let value = self.sql_assignment()?;
            expr = SqlExpression::Assignment(name, Box::new(value));
        }

        Ok(expr)
    }

    fn sql_and(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_equality()?;

        if self.matches_forward(TokenKind::And)? {
            let right = self.sql_and()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::And,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_equality(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_in()?;

        if self.matches_forward(TokenKind::Equal)? {
            let right = self.sql_in()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::Equal,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_in(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_factor()?;

        if self.matches_forward(TokenKind::In)? {
            let right = self.sql_factor()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::In,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_factor(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_add()?;

        if self.matches_forward(TokenKind::Star)? {
            let right = self.sql_add()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::Multiply,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_add(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_rem()?;

        if self.matches_forward(TokenKind::Plus)? {
            let right = self.sql_rem()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::Add,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_rem(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_primary()?;

        if self.matches_forward(TokenKind::Percent)? {
            let right = self.sql_primary()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::Rem,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_primary(&mut self) -> Res<SqlExpression> {
        if self.matches(TokenKind::Number)? {
            let i = i16::from_str(&self.previous.lexeme)?;
            Ok(SqlExpression::Integer(i))
        } else if self.matches(TokenKind::Dollar)? {
            self.consume(TokenKind::Identifier, "Expect identifier after $")?;
            Ok(SqlExpression::UpVariable(self.make_variable()?))
        } else if self.matches(TokenKind::Identifier)? {
            Ok(SqlExpression::Var(self.make_variable()?))
        } else if self.matches(TokenKind::LeftParen)? {
            self.sql_set()
        } else {
            Err(Unexpected(format!(
                "Expected sql expression, got a {:?}",
                self.current.kind
            )))
        }
    }

    fn sql_set(&mut self) -> Res<SqlExpression> {
        self.skip_newlines()?;
        let mut members = vec![];
        if !self.check(TokenKind::RightParen) {
            loop {
                let member = self.sql_assignment()?;
                members.push(member);

                if !self.matches(TokenKind::Comma)? {
                    break;
                }
                self.skip_newlines()?;
            }
            self.skip_newlines()?;
        }
        self.consume(TokenKind::RightParen, "Expected ) to close a sql set")?;

        Ok(SqlExpression::Set(members))
    }

    fn included(&mut self) -> Res<Expression> {
        let mut expr = self.equality()?;

        if self.matches(TokenKind::In)? {
            let right = self.equality()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Included,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn equality(&mut self) -> Res<Expression> {
        let mut expr = self.comparison()?;

        if self.matches(TokenKind::Equal)? {
            let right = self.comparison()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Equal,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn comparison(&mut self) -> Res<Expression> {
        let mut expr = self.term()?;

        if self.matches(TokenKind::LeftCarret)? || self.matches(TokenKind::LessEqual)? {
            let operator = match self.previous.lexeme.as_str() {
                "<" => Ok(Operator::Less),
                "<=" => Ok(Operator::LessEqual),
                _ => Err(Unexpected(format!(
                    "unknown comparison operator: {}",
                    self.previous.lexeme
                ))),
            }?;
            let right = self.term()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn term(&mut self) -> Res<Expression> {
        let mut expr = self.factor()?;

        if self.matches(TokenKind::Plus)? {
            let right = self.factor()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Add,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn factor(&mut self) -> Res<Expression> {
        let mut expr = self.remainder()?;

        if self.matches(TokenKind::Star)? {
            let right = self.remainder()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Multiply,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn remainder(&mut self) -> Res<Expression> {
        let mut expr = self.unary()?;

        if self.matches(TokenKind::Percent)? {
            let right = self.unary()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Rem,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn unary(&mut self) -> Res<Expression> {
        if self.matches_forward(TokenKind::Or)? || self.matches_forward(TokenKind::And)? {
            self.expression()
        } else {
            self.member()
        }
    }

    fn member(&mut self) -> Res<Expression> {
        let mut expr = self.primary()?;

        if self.matches(TokenKind::Dot)? {
            self.consume(
                TokenKind::Identifier,
                "Expected identifier after \".\" for member expression",
            )?;
            expr = Expression::Member {
                call_site: Box::new(expr),
                member: self.make_variable()?,
            };
        }

        Ok(expr)
    }

    fn primary(&mut self) -> Res<Expression> {
        if self.matches(TokenKind::Number)? {
            self.number()
        } else if self.matches(TokenKind::LeftBrace)? {
            self.set()
        } else if self.matches(TokenKind::LeftParen)? {
            self.tuple()
        } else if self.matches(TokenKind::Identifier)? {
            self.variable()
        } else if self.matches(TokenKind::Backtick)? {
            self.sql_expression()
        } else if self.matches(TokenKind::Newline)? {
            self.expression()
        } else {
            Err(Unexpected(format!(
                "Expected expression, got a {:?}",
                self.current.kind
            )))
        }
    }

    fn variable(&mut self) -> Res<Expression> {
        Ok(Expression::Var(self.make_variable()?))
    }

    fn number(&mut self) -> Res<Expression> {
        let i = i16::from_str(&self.previous.lexeme)?;
        Ok(Expression::Integer(i))
    }

    fn set(&mut self) -> Res<Expression> {
        self.skip_newlines()?;
        let mut members = vec![];
        if !self.check(TokenKind::RightBrace) {
            loop {
                let member = self.expression()?;
                members.push(member);

                if !self.matches(TokenKind::Comma)? {
                    break;
                }
                self.skip_newlines()?;
            }
            self.skip_newlines()?;
        }
        self.consume(
            TokenKind::RightBrace,
            "Expected } to close a set expression",
        )?;

        Ok(Expression::Set(members))
    }

    fn sql_tuple(&mut self) -> Res<SqlExpression> {
        let mut members = vec![];
        loop {
            let member = self.sql_assignment()?;
            members.push(member);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::RightParen, "Expected closing ) for tuple")?;

        Ok(SqlExpression::Tuple(members))
    }

    fn tuple(&mut self) -> Res<Expression> {
        let mut members = vec![];
        loop {
            let member = self.expression()?;
            members.push(member);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::RightParen, "Expected closing ) for tuple")?;

        Ok(Expression::Tuple(members))
    }

    fn sql_expression(&mut self) -> Res<Expression> {
        let sql = if self.matches(TokenKind::Select)? {
            self.select()
        } else if self.matches(TokenKind::Insert)? {
            self.insert()
        } else if self.matches(TokenKind::Update)? {
            self.update()
        } else {
            Err(Unexpected(format!(
                "Expected sql expression, got a {:?}",
                self.current.kind
            )))
        }?;

        self.consume(
            TokenKind::Backtick,
            &format!("Expected ` to end sql expression '{sql}'"),
        )?;

        Ok(Expression::Sql(sql))
    }

    fn select(&mut self) -> Res<SqlExpression> {
        let mut locking = false;
        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::From, "Expected from clause")?;

        self.consume(TokenKind::Identifier, "Expected relation for select from")?;
        let from = self.make_variable()?;

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            let expr = self.sql_assignment()?;
            condition = Some(Box::new(expr));
        }

        if self.matches(TokenKind::For)? {
            self.consume(
                TokenKind::Update,
                "Expected update after lock condition in select",
            )?;
            locking = true
        }

        Ok(SqlExpression::Select {
            columns,
            from,
            condition,
            locking,
        })
    }

    fn update(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::Identifier, "expect relation for update")?;
        let relation = self.make_variable()?;

        self.consume(TokenKind::Set, "Expected set for update expression")?;

        let update = Box::new(self.sql_assignment()?);

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            condition = Some(Box::new(self.sql_assignment()?));
        }

        Ok(SqlExpression::Update {
            relation,
            update,
            condition,
        })
    }

    fn insert(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::Into, "Expected into after insert")?;

        self.consume(TokenKind::Identifier, "Expected relation after insert into")?;
        let relation = self.make_variable()?;

        self.consume(
            TokenKind::LeftParen,
            "Expected column declaration after relation in insert into",
        )?;

        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(
            TokenKind::RightParen,
            "Expected ) closing columns declaration",
        )?;
        self.consume(
            TokenKind::Values,
            "Expected values after relation declaration",
        )?;

        let mut values = vec![];
        while self.matches(TokenKind::LeftParen)? {
            values.push(self.sql_tuple()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        Ok(SqlExpression::Insert {
            relation,
            columns,
            values,
        })
    }

    fn make_variable(&mut self) -> Res<Variable> {
        Ok(Variable {
            name: self.previous.lexeme.clone(),
        })
    }

    fn skip_newlines(&mut self) -> Unit {
        while self.current.kind == TokenKind::Newline {
            self.advance()?;
        }

        Ok(())
    }
}

impl std::fmt::Display for SqlExpression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlExpression::Select {
                columns,
                from,
                condition,
                locking,
            } => {
                f.write_str("select ")?;

                let mut iter = columns.iter().peekable();
                while let Some(col) = iter.next() {
                    std::fmt::Display::fmt(&col.name, f)?;
                    if iter.peek().is_some() {
                        f.write_str(", ")?;
                    }
                }

                f.write_fmt(format_args!(" from {}", from.name))?;

                if let Some(cond) = condition {
                    f.write_fmt(format_args!(" where {cond}"))?;
                }

                if *locking {
                    f.write_str(" for update")?;
                }

                Ok(())
            }
            SqlExpression::Update {
                relation,
                update,
                condition,
            } => {
                f.write_fmt(format_args!("update {} set {}", relation.name, update))?;

                if let Some(cond) = condition {
                    f.write_fmt(format_args!(" where {cond}"))?;
                }

                Ok(())
            }
            SqlExpression::Insert {
                relation,
                columns,
                values,
            } => {
                f.write_fmt(format_args!("insert {} (", relation.name))?;

                intersperse(f, columns, ",")?;

                f.write_str(") values ")?;

                intersperse(f, values, ",")?;

                Ok(())
            }
            SqlExpression::Binary {
                left,
                operator,
                right,
            } => {
                let op = match operator {
                    SqlOperator::Add => "+",
                    SqlOperator::Multiply => "*",
                    SqlOperator::Rem => "%",
                    SqlOperator::Equal => "=",
                    SqlOperator::And => "and",
                    SqlOperator::In => "in",
                };
                f.write_fmt(format_args!("{left} {op} {right}"))
            }
            SqlExpression::Assignment(var, expr) => {
                f.write_fmt(format_args!("{} := {expr}", var.name))
            }
            SqlExpression::Integer(i) => std::fmt::Display::fmt(&i, f),
            SqlExpression::Tuple(values) => {
                f.write_str("(")?;

                intersperse(f, values, ",")?;

                f.write_str(")")
            }
            SqlExpression::Var(v) => std::fmt::Display::fmt(&v.name, f),
            SqlExpression::UpVariable(v) => f.write_fmt(format_args!("${}", v.name)),
            SqlExpression::Value(_) => panic!("no value formatting"),
            SqlExpression::Set(members) => {
                f.write_str("(")?;

                intersperse(f, members, ",")?;

                f.write_str(")")
            }
        }
    }
}

impl std::fmt::Display for Expression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expression::Sql(sql) => std::fmt::Display::fmt(&sql, f),
            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let op = match operator {
                    Operator::Add => "+",
                    Operator::Multiply => "*",
                    Operator::Rem => "%",
                    Operator::Equal => "=",
                    Operator::LessEqual => "<=",
                    Operator::Less => "<",
                    Operator::Included => "in",
                    Operator::And => "and",
                    Operator::Or => "or",
                };
                f.write_fmt(format_args!("{left} {op} {right}"))
            }
            Expression::Assignment(var, value) => {
                f.write_fmt(format_args!("{} := {}", var.name, value))
            }
            Expression::Var(var) => std::fmt::Display::fmt(&var.name, f),
            Expression::Integer(i) => std::fmt::Display::fmt(&i, f),
            Expression::Set(values) => {
                f.write_str("{")?;
                intersperse(f, values, ",")?;
                f.write_str("}")
            }
            Expression::Tuple(values) => {
                f.write_str("(")?;
                intersperse(f, values, ",")?;
                f.write_str(")")
            }
            Expression::Member { call_site, member } => {
                f.write_fmt(format_args!("{}.{}", call_site, member.name))
            }
        }
    }
}

impl std::fmt::Display for Statement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Statement::Begin(level, Some(tx_name)) => {
                f.write_fmt(format_args!("begin {level} ({})", tx_name.name))
            }
            Statement::Begin(level, None) => f.write_fmt(format_args!("begin {level}")),
            Statement::Commit => f.write_str("commit"),
            Statement::Abort => f.write_str("abort"),
            Statement::Expression(expr) => std::fmt::Display::fmt(&expr, f),
            Statement::Latch => f.write_str("latch"),
            Statement::Always(expr) => f.write_fmt(format_args!("always[{expr}]")),
            Statement::Never(expr) => f.write_fmt(format_args!("never[{expr}]")),
            Statement::Eventually(expr) => f.write_fmt(format_args!("eventually<{expr}>")),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::parser::{Expression, Parser, SqlExpression, SqlOperator, Statement, Variable};

    #[test]
    fn parse_sql_query() {
        let mut parser = Parser::new(
            "`update users set age := $t1_age + 1 where id = 1 and age = $t1_age`\n".to_string(),
        );
        parser.advance().unwrap();

        let mut statements = vec![];
        parser.statement(&mut statements).unwrap();
        assert_eq!(
            Statement::Expression(Expression::Sql(SqlExpression::Update {
                relation: Variable {
                    name: "users".to_string()
                },
                update: Box::new(SqlExpression::Assignment(
                    Variable {
                        name: "age".to_string()
                    },
                    Box::new(SqlExpression::Binary {
                        left: Box::new(SqlExpression::UpVariable(Variable {
                            name: "t1_age".to_string()
                        })),
                        operator: SqlOperator::Add,
                        right: Box::new(SqlExpression::Integer(1)),
                    }),
                )),
                condition: Some(Box::new(SqlExpression::Binary {
                    left: Box::new(SqlExpression::Binary {
                        left: Box::new(SqlExpression::Var(Variable {
                            name: "id".to_string()
                        })),
                        operator: SqlOperator::Equal,
                        right: Box::new(SqlExpression::Integer(1)),
                    }),
                    operator: SqlOperator::And,
                    right: Box::new(SqlExpression::Binary {
                        left: Box::new(SqlExpression::Var(Variable {
                            name: "age".to_string()
                        })),
                        operator: SqlOperator::Equal,
                        right: Box::new(SqlExpression::UpVariable(Variable {
                            name: "t1_age".to_string()
                        })),
                    }),
                })),
            })),
            statements[0]
        );
    }
}
