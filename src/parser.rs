use std::cell::Cell;
use std::fmt::Formatter;
use std::fmt::{Debug, Write};
use std::mem;
use std::num::ParseIntError;
use std::rc::Rc;
use std::str::FromStr;

use crate::engine::Value;
use crate::format::intersperse;
use crate::scanner::{Scanner, ScannerError, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Begin(IsolationLevel, Option<Variable>),
    Commit,
    Abort,
    Expression(Expression),
    Latch,

    If(Expression, Rc<Cell<usize>>),
    Else(Rc<Cell<usize>>),

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
pub enum SelectItem {
    Column(Item),
    Count(Item),
}

impl std::fmt::Display for SelectItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectItem::Column(item) => std::fmt::Display::fmt(item, f),
            SelectItem::Count(item) => f.write_fmt(format_args!("count({item})")),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Item {
    Wildcard,
    Column(String),
}

impl std::fmt::Display for Item {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Wildcard => f.write_char('*'),
            Item::Column(col) => f.write_str(col),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum SqlExpression {
    Select {
        columns: Vec<SelectItem>,
        from: Variable,
        condition: Option<Box<SqlExpression>>,
        order_by: Option<Box<SqlExpression>>,
        locking: bool,
    },
    Update {
        relation: Variable,
        updates: Vec<SqlExpression>,
        condition: Option<Box<SqlExpression>>,
    },
    Delete {
        relation: Variable,
        condition: Option<Box<SqlExpression>>,
    },
    Insert {
        relation: Variable,
        columns: Vec<Variable>,
        values: Vec<SqlExpression>,
    },
    Create {
        relation: Variable,
        columns: Vec<Variable>,
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
    String(String),
    Bool(bool),
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
    String(String),
    Set(Vec<Expression>),
    Tuple(Vec<Expression>),
}

#[derive(PartialEq, Debug, Clone)]
pub enum Operator {
    Add,
    Multiply,
    Rem,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
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
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    In,
    Between,
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
    AggregateError(SelectItem),
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

    fn match_within(&mut self, kinds: &[TokenKind]) -> Res<bool> {
        Ok(if kinds.contains(&self.current.kind) {
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

    fn matches_forward_within(&mut self, kinds: &[TokenKind]) -> Res<bool> {
        let mut clone = self.scanner.clone();
        let mut advances = 1;
        let mut current = self.current.clone();
        loop {
            let Token {
                kind: next_kind, ..
            } = current;
            if next_kind == TokenKind::Newline {
                advances += 1;
            } else if kinds.contains(&next_kind) {
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
            Err(ParserErrorKind::Unexpected(expected.to_string()))
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
            Err(ParserErrorKind::Unexpected(format!(
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
        let mut statements = vec![];
        self.statement(&mut statements)?;
        self.result.properties.push(statements.remove(0));

        Ok(())
    }

    fn statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        if self.matches(TokenKind::Let)? {
            self.assignment_statement(writer)?
        } else if self.matches(TokenKind::Transaction)? {
            self.transaction_statement(writer)?
        } else if self.matches(TokenKind::Begin)? {
            self.begin_statement(writer)?
        } else if self.matches(TokenKind::Commit)? {
            self.commit_statement(writer)?
        } else if self.matches(TokenKind::If)? {
            self.if_statement(writer)?
        } else if self.matches(TokenKind::Else)? {
            self.else_statement(writer)?
        } else if self.matches(TokenKind::Abort)? {
            self.abort_statement(writer)?
        } else if self.matches(TokenKind::Latch)? {
            self.latch_statement(writer)?
        } else if self.matches(TokenKind::Always)? {
            self.always_statement(writer)?
        } else if self.matches(TokenKind::Never)? {
            self.never_statement(writer)?
        } else if self.matches(TokenKind::Eventually)? {
            self.eventually_statement(writer)?
        } else {
            self.expression_statement(writer)?
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
            _ => Err(ParserErrorKind::Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn parse_variable(&mut self, expected: &str) -> Res<Variable> {
        self.consume(TokenKind::Identifier, expected)?;

        Ok(self.make_variable())
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
            _ => Err(ParserErrorKind::Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn commit_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Commit);
        self.manual_commit = true;
        Ok(())
    }

    fn if_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let expr = self.expression()?;
        self.consume(TokenKind::Do, "Expected do token after if condition")?;
        self.end_line()?;

        let if_offset = Rc::new(Cell::new(0));
        writer.push(Statement::If(expr, if_offset.clone()));

        while !self.matches_forward(TokenKind::Else)? {
            self.statement(writer)?;
            if_offset.set(if_offset.get() + 1);
        }

        let else_offset = Rc::new(Cell::new(0));
        writer.push(Statement::Else(else_offset.clone()));
        if_offset.set(if_offset.get() + 1);
        self.end_line()?;

        while !self.matches_forward(TokenKind::End)? {
            self.statement(writer)?;
            else_offset.set(else_offset.get() + 1);
        }

        Ok(())
    }

    fn else_statement(&mut self, _writer: &mut [Statement]) -> Unit {
        panic!()
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
        self.consume(TokenKind::LeftParen, "Expected ( to open always statement")?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightParen,
            "Expected ) to close always statement",
        )?;

        writer.push(Statement::Always(expr));
        Ok(())
    }

    fn never_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(TokenKind::LeftParen, "Expected ( to open never statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightParen, "Expected ) to close never statement")?;
        writer.push(Statement::Never(expr));
        Ok(())
    }

    fn eventually_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(
            TokenKind::LeftParen,
            "Expected ( to open eventually statement",
        )?;

        let expr = self.expression()?;

        self.skip_newlines()?;
        self.consume(
            TokenKind::RightParen,
            "Expected ) to close eventually statement",
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
                return Err(ParserErrorKind::Unexpected(format!(
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
                return Err(ParserErrorKind::Unexpected(format!(
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
        let mut expr = self.sql_comparison()?;

        if self.matches_forward_within(&[TokenKind::Equal, TokenKind::Different])? {
            let operator = match self.previous.kind {
                TokenKind::Equal => SqlOperator::Equal,
                TokenKind::Different => SqlOperator::NotEqual,
                _ => panic!(),
            };
            let right = self.sql_comparison()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_comparison(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_in()?;

        if self.matches_forward_within(&[
            TokenKind::LeftCarret,
            TokenKind::LessEqual,
            TokenKind::GreaterEqual,
            TokenKind::RightCarret,
        ])? {
            let operator = match self.previous.kind {
                TokenKind::LeftCarret => SqlOperator::Less,
                TokenKind::LessEqual => SqlOperator::LessEqual,
                TokenKind::GreaterEqual => SqlOperator::GreaterEqual,
                TokenKind::RightCarret => SqlOperator::Greater,
                _ => panic!(),
            };
            let right = self.sql_in()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_in(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_between()?;

        if self.matches_forward(TokenKind::In)? {
            let right = self.sql_between()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::In,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn sql_between(&mut self) -> Res<SqlExpression> {
        let mut expr = self.sql_factor()?;

        if self.matches_forward(TokenKind::Between)? {
            let lower = self.sql_factor()?;
            self.consume(
                TokenKind::And,
                "Expected and for upper bound of the between",
            )?;
            let upper = self.sql_factor()?;
            expr = SqlExpression::Binary {
                left: Box::new(expr),
                operator: SqlOperator::Between,
                right: Box::new(SqlExpression::Tuple(vec![lower, upper])),
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
        } else if self.matches(TokenKind::String)? {
            let s = self.previous.lexeme.clone();
            Ok(SqlExpression::String(s))
        } else if self.matches(TokenKind::Dollar)? {
            self.consume(TokenKind::Identifier, "Expect identifier after $")?;
            Ok(SqlExpression::UpVariable(self.make_variable()))
        } else if self.matches(TokenKind::Identifier)? {
            Ok(SqlExpression::Var(self.make_variable()))
        } else if self.matches(TokenKind::LeftParen)? {
            self.sql_set()
        } else {
            Err(ParserErrorKind::Unexpected(format!(
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

        if self.match_within(&[TokenKind::Equal, TokenKind::Different])? {
            let operator = match self.previous.kind {
                TokenKind::Equal => Ok(Operator::Equal),
                TokenKind::Different => Ok(Operator::NotEqual),
                _ => Err(ParserErrorKind::Unexpected(format!(
                    "unknown equality operator: {}",
                    self.previous.lexeme
                ))),
            }?;
            let right = self.comparison()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn comparison(&mut self) -> Res<Expression> {
        let mut expr = self.term()?;

        if self.match_within(&[
            TokenKind::LeftCarret,
            TokenKind::LessEqual,
            TokenKind::GreaterEqual,
            TokenKind::RightCarret,
        ])? {
            let operator = match self.previous.kind {
                TokenKind::LessEqual => Ok(Operator::LessEqual),
                TokenKind::GreaterEqual => Ok(Operator::GreaterEqual),
                TokenKind::LeftCarret => Ok(Operator::Less),
                TokenKind::RightCarret => Ok(Operator::Greater),
                _ => Err(ParserErrorKind::Unexpected(format!(
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
                member: self.make_variable(),
            };
        }

        Ok(expr)
    }

    fn primary(&mut self) -> Res<Expression> {
        if self.matches(TokenKind::Number)? {
            self.number()
        } else if self.matches(TokenKind::String)? {
            self.string()
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
            Err(ParserErrorKind::Unexpected(format!(
                "Expected expression, got a {:?}",
                self.current.kind
            )))
        }
    }

    fn variable(&mut self) -> Res<Expression> {
        Ok(Expression::Var(self.make_variable()))
    }

    fn number(&mut self) -> Res<Expression> {
        let i = i16::from_str(&self.previous.lexeme)?;
        Ok(Expression::Integer(i))
    }

    fn string(&mut self) -> Res<Expression> {
        let s = self.previous.lexeme.clone();
        Ok(Expression::String(s))
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
        self.skip_newlines()?;
        let mut members = vec![];
        if !self.check(TokenKind::RightParen) {
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
        } else if self.matches(TokenKind::Delete)? {
            self.delete()
        } else if self.matches(TokenKind::Create)? {
            self.create()
        } else {
            Err(ParserErrorKind::Unexpected(format!(
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
        while self.current.kind != TokenKind::From {
            columns.push(self.select_clause()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        if columns
            .iter()
            .any(|col| matches!(col, SelectItem::Count(_)))
        {
            if let Some(item) = columns.iter().find(|x| !matches!(x, SelectItem::Count(_))) {
                return Err(ParserErrorKind::AggregateError(item.clone()));
            }
        }

        self.consume(TokenKind::From, "Expected from clause")?;

        self.consume(TokenKind::Identifier, "Expected relation for select from")?;
        let from = self.make_variable();

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            let expr = self.sql_assignment()?;
            condition = Some(Box::new(expr));
        }

        let mut order_by = None;
        if self.matches(TokenKind::Order)? {
            self.consume(TokenKind::By, "Expected by after order in select")?;

            order_by = Some(Box::new(self.sql_factor()?));
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
            order_by,
            locking,
        })
    }

    fn select_clause(&mut self) -> Res<SelectItem> {
        if self.matches(TokenKind::Count)? {
            self.consume(TokenKind::LeftParen, "Expected ( after count")?;
            let item = self.parse_select_item()?;
            self.consume(TokenKind::RightParen, "Expected ) after count")?;
            Ok(SelectItem::Count(item))
        } else {
            Ok(SelectItem::Column(self.parse_select_item()?))
        }
    }

    fn parse_select_item(&mut self) -> Res<Item> {
        if self.matches(TokenKind::Star)? {
            Ok(Item::Wildcard)
        } else if self.matches(TokenKind::Identifier)? {
            Ok(Item::Column(self.make_variable().name))
        } else {
            Err(ParserErrorKind::Unexpected(format!(
                "Expected select clause, got a {:?} instead",
                self.current.kind
            )))
        }
    }

    fn update(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::Identifier, "expected relation for update")?;
        let relation = self.make_variable();

        self.consume(TokenKind::Set, "Expected set for update expression")?;

        let mut updates = vec![];
        loop {
            updates.push(self.sql_assignment()?);
            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            condition = Some(Box::new(self.sql_assignment()?));
        }

        Ok(SqlExpression::Update {
            relation,
            updates,
            condition,
        })
    }

    fn delete(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::From, "Expected from clause")?;
        self.consume(TokenKind::Identifier, "expect relation for update")?;
        let relation = self.make_variable();

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            condition = Some(Box::new(self.sql_assignment()?));
        }

        Ok(SqlExpression::Delete {
            relation,
            condition,
        })
    }

    fn create(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::Unique, "Expected unique after create")?;
        self.consume(TokenKind::Index, "Expected index after create unique")?;
        self.consume(TokenKind::On, "Expected on after create unique index")?;

        self.consume(
            TokenKind::Identifier,
            "Expected table object for create unique index",
        )?;
        let relation = self.make_variable();

        self.consume(
            TokenKind::LeftParen,
            "Expected column declaration after relation in insert into",
        )?;

        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable());

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }
        self.consume(
            TokenKind::RightParen,
            "Expected ) closing columns declaration",
        )?;

        Ok(SqlExpression::Create { relation, columns })
    }

    fn insert(&mut self) -> Res<SqlExpression> {
        self.consume(TokenKind::Into, "Expected into after insert")?;

        self.consume(TokenKind::Identifier, "Expected relation after insert into")?;
        let relation = self.make_variable();

        self.consume(
            TokenKind::LeftParen,
            "Expected column declaration after relation in insert into",
        )?;

        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable());

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

    fn make_variable(&mut self) -> Variable {
        Variable {
            name: self.previous.lexeme.clone(),
        }
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
                order_by,
                locking,
            } => {
                f.write_str("select ")?;

                let mut iter = columns.iter().peekable();
                while let Some(col) = iter.next() {
                    std::fmt::Display::fmt(col, f)?;
                    if iter.peek().is_some() {
                        f.write_str(", ")?;
                    }
                }

                f.write_fmt(format_args!(" from {}", from.name))?;

                if let Some(cond) = condition {
                    f.write_fmt(format_args!(" where {cond}"))?;
                }

                if let Some(order) = order_by {
                    f.write_fmt(format_args!(" order by {order}"))?;
                }

                if *locking {
                    f.write_str(" for update")?;
                }

                Ok(())
            }
            SqlExpression::Update {
                relation,
                updates,
                condition,
            } => {
                f.write_fmt(format_args!("update {} set ", relation.name))?;

                intersperse(f, updates, ",")?;

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
            SqlExpression::Delete {
                relation,
                condition,
            } => {
                f.write_fmt(format_args!("delete from {}", relation.name))?;

                if let Some(cond) = condition {
                    f.write_fmt(format_args!(" where {cond}"))?;
                }

                Ok(())
            }
            SqlExpression::Create { relation, columns } => {
                f.write_fmt(format_args!("create unique index on {}(", relation.name))?;

                intersperse(f, columns, ",")?;

                f.write_str(")")
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
                    SqlOperator::NotEqual => "<>",
                    SqlOperator::Less => "<",
                    SqlOperator::LessEqual => "<=",
                    SqlOperator::Greater => ">",
                    SqlOperator::GreaterEqual => ">=",
                    SqlOperator::Between => {
                        if let SqlExpression::Tuple(tuples) = right.as_ref() {
                            return f.write_fmt(format_args!(
                                "{left} between {} and {}",
                                tuples[0], tuples[1]
                            ));
                        } else {
                            panic!()
                        }
                    }
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
            SqlExpression::String(s) => f.write_str(s),
            SqlExpression::Bool(_) => panic!(),
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
                    Operator::NotEqual => "<>",
                    Operator::LessEqual => "<=",
                    Operator::Less => "<",
                    Operator::Included => "in",
                    Operator::And => "and",
                    Operator::Or => "or",
                    Operator::Greater => ">",
                    Operator::GreaterEqual => ">=",
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
            Expression::String(s) => f.write_str(s),
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
            Statement::Always(expr) => f.write_fmt(format_args!("always({expr})")),
            Statement::Never(expr) => f.write_fmt(format_args!("never({expr})")),
            Statement::Eventually(expr) => f.write_fmt(format_args!("eventually({expr})")),
            Statement::If(expr, _) => f.write_fmt(format_args!("if {expr} do")),
            Statement::Else(_) => f.write_str("else"),
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
                updates: vec![SqlExpression::Assignment(
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
                )],
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
