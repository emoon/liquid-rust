use error::{Result, ResultLiquidExt};

use compiler::Element;
use compiler::LiquidOptions;
use compiler::Token;
use compiler::{parse, unexpected_token_error};
use interpreter::Context;
use interpreter::Renderable;
use interpreter::Template;
use value::Value;

#[derive(Debug)]
struct Capture {
    id: String,
    template: Template,
}

impl Capture {
    fn trace(&self) -> String {
        format!("{{% capture {} %}}", self.id)
    }
}

impl Renderable for Capture {
    fn render(&self, context: &mut Context) -> Result<Option<String>> {
        let output = self.template
            .render(context)
            .trace_with(|| self.trace().into())?
            .unwrap_or_else(|| "".to_owned());

        context.set_global_val(&self.id, Value::scalar(output));
        Ok(None)
    }
}

pub fn capture_block(
    _tag_name: &str,
    arguments: &[Token],
    tokens: &[Element],
    options: &LiquidOptions,
) -> Result<Box<Renderable>> {
    let mut args = arguments.iter();
    let id = match args.next() {
        Some(&Token::Identifier(ref x)) => x.clone(),
        x @ Some(_) | x @ None => return Err(unexpected_token_error("identifier", x)),
    };

    // there should be no trailing tokens after this
    if let t @ Some(_) = args.next() {
        return Err(unexpected_token_error("`%}`", t));
    };

    let t = Template::new(
        parse(tokens, options).trace_with(|| format!("{{% capture {} %}}", &id).into())?
    );

    Ok(Box::new(Capture { id, template: t }))
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
            .insert("capture", (capture_block as compiler::FnParseBlock).into());
        options
    }

    #[test]
    fn test_capture() {
        let text = concat!(
            "{% capture attribute_name %}",
            "{{ item }}-{{ i }}-color",
            "{% endcapture %}"
        );
        let tokens = compiler::tokenize(text).unwrap();
        let options = options();
        let template = compiler::parse(&tokens, &options)
            .map(interpreter::Template::new)
            .unwrap();

        let mut ctx = Context::new();
        ctx.set_global_val("item", Value::scalar("potato"));
        ctx.set_global_val("i", Value::scalar(42f64));

        let output = template.render(&mut ctx).unwrap();
        assert_eq!(
            ctx.get_val("attribute_name"),
            Some(&Value::scalar("potato-42-color"))
        );
        assert_eq!(output, Some("".to_owned()));
    }

    #[test]
    fn trailing_tokens_are_an_error() {
        let text = concat!(
            "{% capture foo bar baz %}",
            "We should never see this",
            "{% endcapture %}"
        );
        let tokens = compiler::tokenize(text).unwrap();
        let options = options();
        let template = compiler::parse(&tokens, &options).map(interpreter::Template::new);
        assert!(template.is_err());
    }
}
