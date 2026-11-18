//! A bounded parser for the documented JQL subset. Filtering runs only on visible Jira projections.
use crate::{Result, invalid};
use chrono::{DateTime, Days, Months, NaiveDate, Utc};
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Op(String),
    Open,
    Close,
    Comma,
}

fn error(message: &str) -> String {
    invalid(&format!("Error in the JQL Query: {message}"))
}
fn lex(query: &str) -> Result<Vec<Token>> {
    if query.len() > 8192 {
        return Err(error("query exceeds 8192 bytes"));
    }
    let mut input = query.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = input.next() {
        let token = match ch {
            c if c.is_whitespace() => continue,
            '(' => Token::Open,
            ')' => Token::Close,
            ',' => Token::Comma,
            '\'' | '"' => {
                let mut value = String::new();
                loop {
                    match input.next() {
                        None => return Err(error("unterminated quoted value")),
                        Some(c) if c == ch => break,
                        Some('\\') => value.push(
                            input
                                .next()
                                .ok_or_else(|| error("incomplete string escape"))?,
                        ),
                        Some(c) => value.push(c),
                    }
                }
                Token::Quoted(value)
            }
            '=' | '!' | '>' | '<' | '~' => {
                let mut op = String::from(ch);
                if matches!(ch, '<' | '>' | '!') && input.peek() == Some(&'=')
                    || ch == '!' && input.peek() == Some(&'~')
                {
                    op.push(input.next().unwrap());
                }
                if op == "!" {
                    return Err(error("expected != or !~"));
                }
                Token::Op(op)
            }
            c => {
                let mut word = String::from(c);
                while input
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && !"(),=!><~'\"".contains(*c))
                {
                    word.push(input.next().unwrap());
                }
                Token::Word(word)
            }
        };
        tokens.push(token);
        if tokens.len() > 256 {
            return Err(error("query exceeds 256 tokens"));
        }
    }
    Ok(tokens)
}

const FIELDS: &[&str] = &[
    "project",
    "issuetype",
    "status",
    "statuscategory",
    "resolution",
    "priority",
    "assignee",
    "reporter",
    "creator",
    "labels",
    "summary",
    "description",
    "text",
    "created",
    "updated",
    "duedate",
    "key",
    "id",
];
const SORT: &[&str] = &[
    "key", "created", "updated", "priority", "status", "assignee", "summary", "duedate",
];
struct Parser<'a> {
    tokens: Vec<Token>,
    index: usize,
    actor: &'a str,
}
enum Expr {
    All,
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Empty(String, bool),
    Compare(String, String, Vec<String>),
}
pub(super) struct Query {
    condition: Expr,
    order: Vec<(String, bool)>,
}

pub(super) fn parse(query: &str, actor: &str) -> Result<Query> {
    let mut p = Parser {
        tokens: lex(query)?,
        index: 0,
        actor,
    };
    let condition = if p.tokens.is_empty() || p.at("ORDER") {
        Expr::All
    } else {
        p.or(0)?
    };
    let mut order = Vec::new();
    if p.word("ORDER") {
        p.require("BY")?;
        loop {
            let field = p.name()?.to_ascii_lowercase();
            if !SORT.contains(&field.as_str()) {
                return Err(error(&format!("unsupported sort field {field}")));
            }
            let desc = p.word("DESC");
            if !desc {
                p.word("ASC");
            }
            order.push((field, desc));
            if !p.take(&Token::Comma) {
                break;
            }
        }
    }
    if p.index != p.tokens.len() {
        return Err(error("unexpected trailing token"));
    }
    if order.is_empty() {
        order.push(("updated".into(), true));
    }
    Ok(Query { condition, order })
}
impl Parser<'_> {
    fn at(&self, k: &str) -> bool {
        matches!(self.tokens.get(self.index),Some(Token::Word(w)) if w.eq_ignore_ascii_case(k))
    }
    fn word(&mut self, k: &str) -> bool {
        if self.at(k) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, k: &str) -> Result<()> {
        if self.word(k) {
            Ok(())
        } else {
            Err(error(&format!("expected {k}")))
        }
    }
    fn take(&mut self, t: &Token) -> bool {
        if self.tokens.get(self.index) == Some(t) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn name(&mut self) -> Result<String> {
        match self.tokens.get(self.index).cloned() {
            Some(Token::Word(w) | Token::Quoted(w)) => {
                self.index += 1;
                Ok(w)
            }
            _ => Err(error("expected a field name or value")),
        }
    }
    fn value(&mut self, field: &str) -> Result<String> {
        let function = self.at("currentUser");
        let mut value = self.name()?;
        if self.take(&Token::Open) {
            if !function || !self.take(&Token::Close) {
                return Err(error("unknown function or unexpected arguments"));
            }
            value = self.actor.into();
        }
        if matches!(field, "created" | "updated" | "duedate") && date(&value).is_none() {
            return Err(error("invalid date or relative date offset"));
        }
        Ok(value)
    }
    fn or(&mut self, depth: usize) -> Result<Expr> {
        let mut left = self.and(depth)?;
        while self.word("OR") {
            left = Expr::Or(Box::new(left), Box::new(self.and(depth)?));
        }
        Ok(left)
    }
    fn and(&mut self, depth: usize) -> Result<Expr> {
        let mut left = self.unary(depth)?;
        while self.word("AND") {
            left = Expr::And(Box::new(left), Box::new(self.unary(depth)?));
        }
        Ok(left)
    }
    fn unary(&mut self, depth: usize) -> Result<Expr> {
        if depth > 24 {
            return Err(error("query is too deeply nested"));
        }
        if self.word("NOT") {
            return Ok(Expr::Not(Box::new(self.unary(depth + 1)?)));
        }
        if self.take(&Token::Open) {
            let inner = self.or(depth + 1)?;
            if !self.take(&Token::Close) {
                return Err(error("expected closing parenthesis"));
            }
            return Ok(inner);
        }
        let field = self.name()?.to_ascii_lowercase();
        if !FIELDS.contains(&field.as_str()) {
            return Err(error(&format!("unknown field {field}")));
        }
        if self.word("IS") {
            let not = self.word("NOT");
            if !self.word("EMPTY") && !self.word("NULL") {
                return Err(error("expected EMPTY or NULL"));
            }
            return Ok(Expr::Empty(field, not));
        }
        let not = self.word("NOT");
        if self.word("IN") {
            if !self.take(&Token::Open) {
                return Err(error("expected an IN list"));
            }
            let mut values = vec![self.value(&field)?];
            while self.take(&Token::Comma) {
                values.push(self.value(&field)?);
            }
            if !self.take(&Token::Close) {
                return Err(error("expected closing parenthesis"));
            }
            return Ok(Expr::Compare(
                field,
                if not { "!=" } else { "=" }.into(),
                values,
            ));
        }
        if not {
            return Err(error("expected IN after NOT"));
        }
        let Some(Token::Op(op)) = self.tokens.get(self.index).cloned() else {
            return Err(error("expected a comparison operator"));
        };
        self.index += 1;
        let value = self.value(&field)?;
        Ok(Expr::Compare(field, op, vec![value]))
    }
}

fn date(value: &str) -> Option<(i64, i64)> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z"))
    {
        let time = timestamp.timestamp_millis();
        return Some((time, time));
    }
    let absolute = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    let day = if let Some(day) = absolute {
        day
    } else {
        if !value.is_ascii() || value.len() < 3 {
            return None;
        }
        let (amount, unit) = value.split_at(value.len() - 1);
        if !amount.starts_with(['-', '+']) {
            return None;
        }
        let offset = amount.parse::<i32>().ok()?;
        let magnitude = offset.unsigned_abs();
        let today = Utc::now().date_naive();
        match unit {
            "d" | "w" => {
                let days = Days::new(u64::from(magnitude) * if unit == "w" { 7 } else { 1 });
                if offset < 0 {
                    today.checked_sub_days(days)?
                } else {
                    today.checked_add_days(days)?
                }
            }
            "M" | "y" => {
                let months =
                    Months::new(magnitude.checked_mul(if unit == "y" { 12 } else { 1 })?);
                if offset < 0 {
                    today.checked_sub_months(months)?
                } else {
                    today.checked_add_months(months)?
                }
            }
            _ => return None,
        }
    };
    Some((
        day.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis(),
        day.and_hms_milli_opt(23, 59, 59, 999)?
            .and_utc()
            .timestamp_millis(),
    ))
}
fn collect(v: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|k| v.get(k)?.as_str().map(str::to_owned))
        .collect()
}
fn plain_description(f: &Value) -> String {
    super::fields::plain(&f["description"]).unwrap_or_default()
}
fn values(i: &Value, field: &str) -> Vec<String> {
    let f = &i["fields"];
    match field {
        "project" => collect(&f["project"], &["key", "id", "name"]),
        "status" | "priority" | "issuetype" => collect(&f[field], &["name", "id"]),
        "statuscategory" => collect(&f["status"]["statusCategory"], &["key", "name"]),
        "assignee" | "reporter" | "creator" => collect(&f[field], &["accountId", "displayName"]),
        "labels" => f[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        "description" => vec![plain_description(f)],
        "text" => vec![
            f["summary"].as_str().unwrap_or("").into(),
            plain_description(f),
        ],
        "key" | "id" => collect(i, &[field]),
        "resolution" => collect(&f[field], &["name", "id"]),
        _ => collect(f, &[field]),
    }
}
fn priority(v: &str) -> Option<u8> {
    super::fields::PRIORITIES
        .iter()
        .position(|(id, name, _)| v == *id || v.eq_ignore_ascii_case(name))
        .map(|i| 5 - i as u8)
}
fn order(a: &str, b: &str, field: &str) -> Ordering {
    if field == "priority"
        && let (Some(a), Some(b)) = (priority(a), priority(b))
    {
        return a.cmp(&b);
    }
    if field == "key"
        && let (Some((p, a)), Some((q, b))) = (a.rsplit_once('-'), b.rsplit_once('-'))
        && let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>())
    {
        return p
            .to_uppercase()
            .cmp(&q.to_uppercase())
            .then_with(|| a.cmp(&b));
    }
    if field == "id"
        && let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>())
    {
        return a.cmp(&b);
    }
    a.to_lowercase().cmp(&b.to_lowercase())
}
fn compare(a: &str, b: &str, op: &str, field: &str) -> bool {
    if matches!(field, "created" | "updated" | "duedate") {
        let Some((actual, _)) = date(a) else {
            return false;
        };
        let Some((from, to)) = date(b) else {
            return false;
        };
        return match op {
            "=" | "!=" => (from..=to).contains(&actual),
            "<" => actual < from,
            "<=" => actual <= to,
            ">" => actual > to,
            ">=" => actual >= from,
            _ => false,
        };
    }
    let cmp = order(a, b, field);
    match op {
        "=" | "!=" => cmp.is_eq(),
        "~" | "!~" => a.to_lowercase().contains(&b.to_lowercase()),
        "<" => cmp.is_lt(),
        "<=" => !cmp.is_gt(),
        ">" => cmp.is_gt(),
        ">=" => !cmp.is_lt(),
        _ => false,
    }
}
impl Expr {
    fn evaluate(&self, item: &Value) -> bool {
        match self {
            Self::All => true,
            Self::And(a, b) => a.evaluate(item) && b.evaluate(item),
            Self::Or(a, b) => a.evaluate(item) || b.evaluate(item),
            Self::Not(inner) => !inner.evaluate(item),
            Self::Empty(f, not) => values(item, f).iter().all(|s| s.is_empty()) != *not,
            Self::Compare(f, op, wanted) => {
                let mut actual = values(item, f);
                if f == "resolution" && actual.is_empty() {
                    actual.push("Unresolved".into());
                }
                if actual.is_empty() {
                    return false;
                }
                let positive = actual
                    .iter()
                    .any(|a| wanted.iter().any(|b| compare(a, b, op, f)));
                if matches!(op.as_str(), "!=" | "!~") {
                    !positive
                } else {
                    positive
                }
            }
        }
    }
}
impl Query {
    pub(super) fn matches(&self, item: &Value) -> bool {
        self.condition.evaluate(item)
    }
    pub(super) fn compare(&self, a: &Value, b: &Value) -> Ordering {
        for (field, desc) in &self.order {
            let a = values(a, field);
            let b = values(b, field);
            let cmp = match (a.first(), b.first()) {
                (Some(a), Some(b)) => order(a, b, field),
                _ => a.len().cmp(&b.len()),
            };
            if !cmp.is_eq() {
                return if *desc { cmp.reverse() } else { cmp };
            }
        }
        order(
            a["key"].as_str().unwrap_or(""),
            b["key"].as_str().unwrap_or(""),
            "key",
        )
        .then_with(|| {
            a["fields"]["project"]["id"]
                .as_str()
                .cmp(&b["fields"]["project"]["id"].as_str())
        })
    }
}
