use winnow::{
    combinator::cut_err,
    error::{
        ContextError, ErrMode, StrContext as Ctx,
        StrContextValue::{Description, StringLiteral},
    },
    token::{tag, take_while},
    PResult, Parser,
};

type Input<'i> = &'i str;

fn balance(src: &str) -> Option<&str> {
    let mut balance = 0usize;
    for (i, c) in src.bytes().enumerate() {
        match c {
            b'{' => balance += 1,
            b'}' => balance -= 1,
            _ => {}
        }
        if balance == 0 {
            return Some(&src[1..i]);
        }
    }
    None
}

fn ws<'i>(input: &mut Input<'i>) -> PResult<&'i str> {
    take_while(0.., |c: char| c.is_whitespace()).parse_next(input)
}

fn lit<'i>(lit: &'static str) -> impl FnMut(&mut Input<'i>) -> PResult<&'i str> {
    move |input| {
        (ws, tag(lit), ws)
            .map(|(_, lit, _)| lit)
            .context(Ctx::Expected(StringLiteral(lit)))
            .parse_next(input)
    }
}

fn limited<'i>(input: &mut Input<'i>) -> PResult<&'i str> {
    if let Some(src) = balance(input) {
        *input = &input[src.len() + 2..]; // 2 is '{' + '}'
        Ok(src)
    } else {
        *input = "";
        Err(ErrMode::Backtrack(ContextError::new()))
    }
}

fn extract_impl<'i>(input: &mut Input<'i>) -> PResult<&'i str> {
    (
        lit("where"),
        cut_err((
            lit("cargo"),
            lit(":"),
            limited.context(Ctx::Expected(Description("`{ .. }` block"))),
        )),
    )
        .map(|(_, (_, _, output))| output)
        .parse_next(input)
}

pub fn extract_manifest(mut src: &str) -> Result<(&str, &str), String> {
    match extract_impl(&mut src) {
        Err(ErrMode::Cut(err)) => Err(err.to_string()), // error when parsing manifest
        Err(_) => Ok(("", src)),                        // can't find manifest section
        Ok(ok) => Ok((ok, src)),
    }
}

// fixme: add `winnom` inner tests
#[test]
fn simple_extract() {
    assert_eq!(extract_manifest("where cargo: {empty} TAIL"), Ok(("empty", " TAIL")))
}
