use {
    ariadne::{Color, Label, Report, ReportKind, Source},
    std::{cell::Cell, fmt::Formatter},
};

use chumsky::prelude::*;

fn parser<'a>() -> impl Parser<'a, &'a str, (Option<&'a str>, &'a str), extra::Err<Rich<'a, char>>>
{
    let atom = |kw| text::keyword(kw).padded().labelled(kw).as_context();
    let op = |c| just(c).padded();

    let balance = Cell::new(0isize);
    let block = any()
        .and_is(end().not())
        .filter(move |c| {
            match c {
                '{' => balance.set(balance.get() + 1),
                '}' => balance.set(balance.get() - 1),
                _ => {}
            };
            balance.get() >= 0
        })
        .repeated()
        .to_slice()
        .delimited_by(just('{'), just('}'))
        .labelled("block {..}")
        .as_context();

    let tail = any().repeated().to_slice();
    let def = group((atom("where"), atom("cargo"), op(':')));

    let not = atom("where").not();
    choice((not.map(|_| None).then(tail), def.ignore_then(block).map(Some).then(tail)))
}

#[derive(Debug)]
pub struct Error {
    errors: Vec<Rich<'static, char>>,
    src: String,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        debug_assert!(!self.errors.is_empty(), "`Error` without `errors` doesn't make sense");

        let mut buf = String::with_capacity(128);
        for e in &self.errors {
            Report::build(ReportKind::Error, (), e.span().start)
                .with_message(e.to_string())
                .with_label(
                    Label::new(e.span().into_range())
                        .with_message(e.reason().to_string())
                        .with_color(Color::Red),
                )
                .with_labels(e.contexts().map(|(label, span)| {
                    Label::new(span.into_range())
                        .with_message(format!("while parsing `{}`", label))
                        .with_color(Color::Yellow)
                }))
                .finish()
                // Safety: report provide valid utf-8`
                .write(Source::from(&self.src), unsafe { buf.as_mut_vec() })
                .map_err(|_| fmt::Error)?;
        }

        write!(f, "{buf}")
    }
}

use {anyhow::Result, std::fmt, toml::Table};

pub fn extract_manifest(src: &str) -> Result<(Option<Table>, &str)> {
    fn error(errors: Vec<Rich<'_, char>>, src: &str) -> Error {
        Error { errors: errors.into_iter().map(Rich::into_owned).collect(), src: src.to_owned() }
    }

    match parser().parse(src).into_result() {
        Ok((table, src)) => Ok((table.map(str::parse).transpose()?, src)),
        Err(errors) => Err(error(errors, src).into()),
    }
}

#[test]
fn simple_extract() {
    assert_eq!(extract_manifest("where cargo: {  } TAIL").unwrap(), (Some(Table::new()), " TAIL"));
    assert_eq!(extract_manifest("TAIL").unwrap(), (None, "TAIL"));

    assert!(extract_manifest("where").is_err());
    assert!(extract_manifest("where cargo: {").is_err());
}
