use itertools;

use error::{Error, Result, ResultLiquidExt};

use compiler::Element;
use compiler::LiquidOptions;
use compiler::Token;
use compiler::{consume_value_token, parse, split_block, unexpected_token_error, BlockSplit};
use interpreter::Argument;
use interpreter::Context;
use interpreter::Renderable;
use interpreter::Template;
use value::Value;

#[derive(Debug)]
struct CaseOption {
    args: Vec<Argument>,
    template: Template,
}

impl CaseOption {
    fn new(args: Vec<Argument>, template: Template) -> CaseOption {
        CaseOption { args, template }
    }

    fn evaluate(&self, value: &Value, context: &Context) -> Result<bool> {
        for a in &self.args {
            let v = a.evaluate(context)?;
            if v == *value {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn trace(&self) -> String {
        format!("{{% when {} %}}", itertools::join(self.args.iter(), " or "))
    }
}

#[derive(Debug)]
struct Case {
    target: Argument,
    cases: Vec<CaseOption>,
    else_block: Option<Template>,
}

impl Case {
    fn trace(&self) -> String {
        format!("{{% case {} %}}", self.target)
    }
}

impl Renderable for Case {
    fn render(&self, context: &mut Context) -> Result<Option<String>> {
        let value = self.target.evaluate(context)?;
        for case in &self.cases {
            if case.evaluate(&value, context)? {
                return case.template
                    .render(context)
                    .trace_with(|| case.trace().into())
                    .trace_with(|| self.trace().into())
                    .context_with(|| (self.target.to_string().into(), value.to_string()));
            }
        }

        if let Some(ref t) = self.else_block {
            return t.render(context)
                .trace_with(|| "{{% else %}}".to_owned().into())
                .trace_with(|| self.trace().into())
                .context_with(|| (self.target.to_string().into(), value.to_string()));
        }

        Ok(None)
    }
}

enum Conditional {
    Cond(Vec<Argument>),
    Else,
}

fn parse_condition(element: &Element) -> Result<Conditional> {
    if let Element::Tag(ref tokens, _) = *element {
        match tokens[0] {
            Token::Identifier(ref name) if name == "else" => return Ok(Conditional::Else),

            Token::Identifier(ref name) if name == "when" => {
                let mut values: Vec<Argument> = Vec::new();
                let mut args = tokens[1..].iter();

                values.push(consume_value_token(&mut args)?.to_arg()?);

                loop {
                    match args.next() {
                        Some(&Token::Or) => {}
                        Some(x) => return Err(unexpected_token_error("`or`", Some(x))),
                        None => break,
                    }

                    values.push(consume_value_token(&mut args)?.to_arg()?);
                }

                return Ok(Conditional::Cond(values));
            }

            ref x => return Err(unexpected_token_error("`else` | `when`", Some(x))),
        }
    } else {
        Err(unexpected_token_error("`else` | `when`", Some(element)))
    }
}

const SECTION_DELIMS: &[&str] = &["when", "else"];

fn parse_sections<'e>(
    case: &mut Case,
    children: &'e [Element],
    options: &LiquidOptions,
) -> Result<Option<BlockSplit<'e>>> {
    let (leading, trailing) = split_block(&children[1..], SECTION_DELIMS, options);

    match parse_condition(&children[0])? {
        Conditional::Cond(conds) => {
            let template = Template::new(parse(leading, options).trace_with(|| {
                format!("{{% when {} %}}", itertools::join(conds.iter(), " or ")).into()
            })?);
            case.cases.push(CaseOption::new(conds, template));
        }
        Conditional::Else => {
            if case.else_block.is_none() {
                let template = Template::new(
                    parse(leading, options).trace_with(|| "{{% else %}}".to_owned().into())?
                );
                case.else_block = Some(template)
            } else {
                return Err(Error::with_msg("Only one else block allowed"));
            }
        }
    }

    Ok(trailing)
}

pub fn case_block(
    _tag_name: &str,
    arguments: &[Token],
    tokens: &[Element],
    options: &LiquidOptions,
) -> Result<Box<Renderable>> {
    let mut args = arguments.iter();
    let value = consume_value_token(&mut args)?.to_arg()?;

    // fast forward to the first arm of the case block,
    let mut children = match split_block(&tokens[..], SECTION_DELIMS, options) {
        (_, Some(split)) => split.trailing,
        _ => return Err(Error::with_msg("Expected case | else")),
    };

    let mut result = Case {
        target: value,
        cases: Vec::new(),
        else_block: None,
    };

    loop {
        let trailing = parse_sections(&mut result, children, options)
            .trace_with(|| format!("{{% case {} %}}", result.target).into())?;
        match trailing {
            Some(split) => children = split.trailing,
            None => break,
        }
    }

    Ok(Box::new(result))
}

#[cfg(test)]
mod test {
    use super::*;
    use compiler;
    use interpreter;

    fn options() -> LiquidOptions {
        let mut options = LiquidOptions::default();
        options
            .blocks
            .insert("case", (case_block as compiler::FnParseBlock).into());
        options
    }

    #[test]
    fn test_case_block() {
        let text = concat!(
            "{% case x %}",
            "{% when 2 %}",
            "two",
            "{% when 3 or 4 %}",
            "three and a half",
            "{% else %}",
            "otherwise",
            "{% endcase %}"
        );
        let tokens = compiler::tokenize(text).unwrap();
        let options = options();
        let template = compiler::parse(&tokens, &options)
            .map(interpreter::Template::new)
            .unwrap();

        let mut context = Context::new();
        context.set_global_val("x", Value::scalar(2f64));
        assert_eq!(
            template.render(&mut context).unwrap(),
            Some("two".to_owned())
        );

        context.set_global_val("x", Value::scalar(3f64));
        assert_eq!(
            template.render(&mut context).unwrap(),
            Some("three and a half".to_owned())
        );

        context.set_global_val("x", Value::scalar(4f64));
        assert_eq!(
            template.render(&mut context).unwrap(),
            Some("three and a half".to_owned())
        );

        context.set_global_val("x", Value::scalar("nope"));
        assert_eq!(
            template.render(&mut context).unwrap(),
            Some("otherwise".to_owned())
        );
    }

    #[test]
    fn test_no_matches_returns_empty_string() {
        let text = concat!(
            "{% case x %}",
            "{% when 2 %}",
            "two",
            "{% when 3 or 4 %}",
            "three and a half",
            "{% endcase %}"
        );
        let tokens = compiler::tokenize(text).unwrap();
        let options = options();
        let template = compiler::parse(&tokens, &options)
            .map(interpreter::Template::new)
            .unwrap();

        let mut context = Context::new();
        context.set_global_val("x", Value::scalar("nope"));
        assert_eq!(template.render(&mut context).unwrap(), Some("".to_owned()));
    }

    #[test]
    fn multiple_else_blocks_is_an_error() {
        let text = concat!(
            "{% case x %}",
            "{% when 2 %}",
            "two",
            "{% else %}",
            "else #1",
            "{% else %}",
            "else # 2",
            "{% endcase %}"
        );
        let tokens = compiler::tokenize(text).unwrap();
        let options = options();
        let template = compiler::parse(&tokens, &options).map(interpreter::Template::new);
        assert!(template.is_err());
    }
}
