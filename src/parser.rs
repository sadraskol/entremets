use crate::parser::Expression::{Insert, Update};
use crate::parser::ParserError::Unexpected;
use crate::scanner::{Scanner, ScannerError, Token, TokenKind};
use std::mem;
use std::num::ParseIntError;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Begin(IsolationLevel),
    Commit,
    Abort,
    Expression(Expression),
    Latch,

    Always(Expression),
    Never(Expression),
    Eventually(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IsolationLevel {
    ReadCommitted,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Variable {
    pub name: String,
    token: Token,
}

#[derive(PartialEq, Debug, Clone)]
pub enum Expression {
    Select {
        columns: Vec<Variable>,
        from: Variable,
        condition: Option<Box<Expression>>,
    },
    Update {
        relation: Variable,
        update: Box<Expression>,
        condition: Option<Box<Expression>>,
    },
    Insert {
        relation: Variable,
        columns: Vec<Variable>,
        values: Vec<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
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
}

pub struct Parser {
    scanner: Scanner,
    previous: Token,
    current: Token,
    result: Mets,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParserError {
    ParseInt(ParseIntError),
    Scanner(ScannerError),
    Uninitialized,
    Unexpected(String),
}

impl From<ScannerError> for ParserError {
    fn from(value: ScannerError) -> Self {
        ParserError::Scanner(value)
    }
}

impl From<ParseIntError> for ParserError {
    fn from(value: ParseIntError) -> Self {
        ParserError::ParseInt(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mets {
    pub init: Vec<Statement>,
    pub processes: Vec<Vec<Statement>>,
    pub properties: Vec<Statement>,
}

pub type Res<T> = Result<T, ParserError>;
type Unit = Res<()>;

impl Parser {
    pub fn new(source: String) -> Self {
        Parser {
            scanner: Scanner::new(source),
            previous: Token::uninitialized(),
            current: Token::uninitialized(),
            result: Mets {
                init: vec![],
                processes: vec![],
                properties: vec![],
            },
        }
    }

    pub fn compile(mut self) -> Result<Mets, String> {
        match self.private_compile() {
            Ok(_) => Ok(self.result),
            Err(err) => Err(format!(
                "Error at position {:?}: {:?}
                previous: {:?}
                current: {:?}",
                self.current.position, err, self.previous, self.current
            )),
        }
    }

    fn private_compile(&mut self) -> Unit {
        self.advance()?;
        self.skip_newlines()?;
        while !self.matches(TokenKind::Eof)? {
            self.declaration()?;
        }
        self.consume(TokenKind::Eof, "Expect end of expression")
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
        self.consume(TokenKind::Do, "Expect do after init declaration")?;
        self.consume(TokenKind::Newline, "Expect newline after init declaration")?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            statements.push(self.statement()?);
        }
        self.result.init = statements;

        self.consume(TokenKind::End, "Expect end at the end of init declaration")?;

        self.end_line()
    }

    fn process_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expect do after process declaration")?;
        self.consume(
            TokenKind::Newline,
            "Expect newline after process declaration",
        )?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            statements.push(self.statement()?);
        }
        self.result.processes.push(statements);

        self.consume(
            TokenKind::End,
            "Expect end at the end of process declaration",
        )?;

        self.end_line()
    }

    fn end_line(&mut self) -> Unit {
        if !self.matches(TokenKind::Eof)? {
            self.consume(TokenKind::Newline, "Expect newline after declaration")?;
        }
        self.skip_newlines()
    }

    fn property_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Equal, "Expect = after property declaration")?;

        let statement = self.statement()?;
        self.result.properties.push(statement);

        Ok(())
    }

    fn statement(&mut self) -> Res<Statement> {
        let res = if self.matches(TokenKind::Let)? {
            self.assignment_statement()
        } else if self.matches(TokenKind::Begin)? {
            self.begin_statement()
        } else if self.matches(TokenKind::Commit)? {
            self.commit_statement()
        } else if self.matches(TokenKind::Abort)? {
            self.abort_statement()
        } else if self.matches(TokenKind::Latch)? {
            self.latch_statement()
        } else if self.matches(TokenKind::Always)? {
            self.always_statement()
        } else if self.matches(TokenKind::Never)? {
            self.never_statement()
        } else if self.matches(TokenKind::Eventually)? {
            self.eventually_statement()
        } else {
            self.expression_statement()
        };

        self.end_line()?;

        res
    }

    fn assignment_statement(&mut self) -> Res<Statement> {
        let expr = self.assignment()?;

        Ok(Statement::Expression(expr))
    }

    fn parse_variable(&mut self, expected: &str) -> Res<Variable> {
        self.consume(TokenKind::Identifier, expected)?;

        self.make_variable()
    }

    fn begin_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::Identifier, "Expect isolation level after begin")?;

        match self.previous.lexeme.as_str() {
            "read_committed" => Ok(Statement::Begin(IsolationLevel::ReadCommitted)),
            _ => Err(Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn commit_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Commit)
    }

    fn abort_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Abort)
    }

    fn latch_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Latch)
    }

    fn always_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::LeftBracket, "Expect [ to open always statement")?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightBracket,
            "Expect ] to close always statement",
        )?;
        Ok(Statement::Always(expr))
    }

    fn never_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::LeftBracket, "Expect [ to open never statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightBracket, "Expect ] to close never statement")?;
        Ok(Statement::Never(expr))
    }

    fn eventually_statement(&mut self) -> Res<Statement> {
        self.consume(
            TokenKind::LeftCarret,
            "Expect < to open eventually statement",
        )?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightCarret,
            "Expect > to close eventually statement",
        )?;
        Ok(Statement::Eventually(expr))
    }

    fn expression_statement(&mut self) -> Res<Statement> {
        let expr = self.expression()?;
        Ok(Statement::Expression(expr))
    }

    fn expression(&mut self) -> Res<Expression> {
        self.assignment()
    }

    fn assignment(&mut self) -> Res<Expression> {
        let mut expr = self.and()?;

        if self.matches(TokenKind::ColonEqual)? {
            let name = if let Expression::Var(name) = expr {
                name
            } else {
                todo!()
            };
            let value = self.assignment()?;
            expr = Expression::Assignment(name, Box::new(value));
        }

        Ok(expr)
    }

    fn and(&mut self) -> Res<Expression> {
        let mut expr = self.equality()?;

        while self.matches(TokenKind::And)? {
            let right = self.equality()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::And,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn equality(&mut self) -> Res<Expression> {
        let mut expr = self.comparison()?;

        while self.matches(TokenKind::Equal)? {
            let right = self.comparison()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Equal,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn comparison(&mut self) -> Res<Expression> {
        let mut expr = self.term()?;

        while self.matches(TokenKind::LeftCarret)? || self.matches(TokenKind::LessEqual)? {
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
            }
        }

        Ok(expr)
    }

    fn term(&mut self) -> Res<Expression> {
        let mut expr = self.factor()?;

        while self.matches(TokenKind::Plus)? {
            let right = self.factor()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Add,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn factor(&mut self) -> Res<Expression> {
        let mut expr = self.remainder()?;

        while self.matches(TokenKind::Star)? {
            let right = self.remainder()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Multiply,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn remainder(&mut self) -> Res<Expression> {
        let mut expr = self.included()?;

        while self.matches(TokenKind::Percent)? {
            let right = self.included()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Rem,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn included(&mut self) -> Res<Expression> {
        let mut expr = self.is()?;

        while self.matches(TokenKind::In)? {
            let right = self.is()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Included,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn is(&mut self) -> Res<Expression> {
        let mut expr = self.primary()?;

        while self.matches(TokenKind::Is)? {
            let right = self.primary()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Equal,
                right: Box::new(right),
            }
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
        } else if self.matches(TokenKind::Select)? {
            self.select()
        } else if self.matches(TokenKind::Update)? {
            self.update()
        } else if self.matches(TokenKind::Insert)? {
            self.insert()
        } else {
            Err(Unexpected(format!(
                "Expected expression, got: {:?}",
                self.current
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
        self.consume(TokenKind::RightBrace, "Expect } to close a set expression")?;

        Ok(Expression::Set(members))
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

        self.consume(TokenKind::RightParen, "Expect closing ) for tuple")?;

        Ok(Expression::Tuple(members))
    }

    fn select(&mut self) -> Res<Expression> {
        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::From, "Expect from clause")?;

        self.consume(TokenKind::Identifier, "Expect relation for select from")?;
        let from = self.make_variable()?;

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            let expr = self.expression()?;
            condition = Some(Box::new(expr));
        }

        Ok(Expression::Select {
            columns,
            from,
            condition,
        })
    }

    fn make_variable(&mut self) -> Res<Variable> {
        Ok(Variable {
            name: self.previous.lexeme.clone(),
            token: self.previous.clone(),
        })
    }

    fn update(&mut self) -> Res<Expression> {
        self.consume(TokenKind::Identifier, "expect relation for update")?;
        let relation = self.make_variable()?;

        self.consume(TokenKind::Set, "Expect set for update expression")?;

        let update = Box::new(self.expression()?);

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            condition = Some(Box::new(self.expression()?));
        }

        Ok(Update {
            relation,
            update,
            condition,
        })
    }

    fn insert(&mut self) -> Res<Expression> {
        self.consume(TokenKind::Into, "Expect into after insert")?;

        self.consume(TokenKind::Identifier, "Expect relation after insert into")?;
        let relation = self.make_variable()?;

        self.consume(
            TokenKind::LeftParen,
            "Expect column declaration after relation in insert into",
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
            "Expect ) closing columns declaration",
        )?;
        self.consume(
            TokenKind::Values,
            "Expect values after relation declaration",
        )?;

        let mut values = vec![];
        while self.matches(TokenKind::LeftParen)? {
            values.push(self.tuple()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        Ok(Insert {
            relation,
            columns,
            values,
        })
    }

    fn skip_newlines(&mut self) -> Unit {
        while self.current.kind == TokenKind::Newline {
            self.advance()?;
        }

        Ok(())
    }
}
