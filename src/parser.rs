pub mod parser {

    use crate::ast::ast::*;
    use chumsky::{error::Cheap, prelude::*, recursive::Recursive, text::ident};

    fn parse_to_i32(x: String) -> i32 {
        return x.parse::<i32>().unwrap();
    }

    fn parse_to_f32(x: String) -> f32 {
        return x.parse::<f32>().unwrap();
    }

    fn type_specifyer() -> impl Parser<char, Vec<String>, Error = Cheap<char>> {
        just(':')
            .ignore_then(
                ident().padded().map(String::from).chain::<String, _, _>(
                    just('|')
                        .ignore_then(ident().padded().map(String::from))
                        .repeated(),
                ),
            )
            .map(|x| x)
    }

    fn integer() -> impl Parser<char, i32, Error = Cheap<char>> {
        filter::<_, _, Cheap<char>>(char::is_ascii_digit)
            .repeated()
            .at_least(1)
            .collect::<String>()
            .map(parse_to_i32)
    }

    fn float() -> impl Parser<char, f32, Error = Cheap<char>> {
        (filter::<_, _, Cheap<char>>(char::is_ascii_digit)
            .repeated()
            .at_least(1)
            .collect::<String>()
            .then_ignore(just('.'))
            .then(
                filter::<_, _, Cheap<char>>(char::is_ascii_digit)
                    .repeated()
                    .at_least(1)
                    .collect::<String>(),
            ))
        .map(|x| format!("{}.{}", x.0, x.1))
        .map(parse_to_f32)
    }

    fn string() -> impl Parser<char, String, Error = Cheap<char>> {
        let escape = just('\\').ignore_then(
            just('\\')
                .or(just('/'))
                .or(just('"'))
                .or(just('b').to('\x08'))
                .or(just('f').to('\x0C'))
                .or(just('n').to('\n'))
                .or(just('r').to('\r'))
                .or(just('t').to('\t'))
                .or(just('0').to('\0')),
        );

        just('"')
            .ignore_then(filter(|c| *c != '\\' && *c != '"').or(escape).repeated())
            .then_ignore(just('"'))
            .collect::<String>()
    }

    fn symbol_parser() -> impl Parser<char, Symbol, Error = Cheap<char>> {
        string()
            .map(RawData::Str)
            .or(integer().map(RawData::Int))
            .or(float().map(RawData::Float))
            .or(seq("null".chars()).to(RawData::Null))
            .or(seq("true".chars()).to(RawData::Bool(true)))
            .or(seq("false".chars()).to(RawData::Bool(false)))
            .map(Symbol::Data)
            .or(ident().map(String::from).map(Symbol::Identifier))
    }
    fn exp_parser<'a>(
        main_parser: Recursive<'a, char, Vec<Instr>, Cheap<char>>,
    ) -> impl Parser<char, Expression, Error = Cheap<char>> + 'a {
        use Expression::*;
        recursive(|exp| {
            let func_declaration = ident()
                .padded()
                .map(String::from)
                .then(type_specifyer())
                .chain(
                    just(',')
                        .ignore_then(ident().padded().map(String::from).then(type_specifyer()))
                        .repeated(),
                )
                .or_not()
                .flatten()
                .delimited_by('(', ')')
                .then(type_specifyer().padded().or_not())
                .then(
                    (seq("=>".chars()).padded().ignore_then(
                        main_parser.clone().delimited_by('{', '}').or(exp
                            .clone()
                            .map_with_span(Instr::LoneExpression)
                            .map(|x| vec![x])),
                    ))
                    .or_not(),
                )
                .then_ignore(seq([';']).or_not())
                .map(|((args, ret), body)| {
                    Symbol::Data(RawData::Func(Function {
                        args,
                        body,
                        return_type: ret.unwrap_or_else(|| vec!["Null".to_string()]),
                    }))
                });

            let func_call = ident()
                .padded()
                .map(String::from)
                .then(
                    exp.clone()
                        .chain(just(',').ignore_then(exp.clone()).repeated())
                        .or_not()
                        .flatten()
                        .delimited_by('(', ')'),
                )
                .map(|(name, args)| FuncCall(name, args));

            let primary_exp = func_call
                .or(symbol_parser().map(Expression::Terminal))
                .or(func_declaration.map(Expression::Terminal))
                .or(exp.delimited_by('(', ')'))
                .boxed();

            let multiply_parser = primary_exp
                .clone()
                .then(one_of(['*', '/']).then(primary_exp.clone()).repeated())
                .map(|(l, t)| {
                    t.iter().fold(l, |left, (op, right)| match op {
                        '*' => Expression::Multiplication(Box::new(left), Box::new(right.clone())),
                        '/' => {
                            Expression::Division(Box::new(left.clone()), Box::new(right.clone()))
                        }
                        _ => panic!("Unexpected operator {}", op),
                    })
                })
                .boxed();
            let comparison_parser = multiply_parser
                .clone()
                .then_ignore(just('<'))
                .then(multiply_parser.clone())
                .map(|x| Expression::LessThan(Box::new(x.0), Box::new(x.1)));
            let equal_parser = comparison_parser
                .clone()
                .or(multiply_parser.clone())
                .then_ignore(seq(['=', '=']))
                .then(comparison_parser.clone().or(multiply_parser.clone()))
                .map(|x| Expression::Equal(Box::new(x.0), Box::new(x.1)));
            comparison_parser
                .or(equal_parser)
                .or(multiply_parser
                    .clone()
                    .then(one_of(['+', '-']).then(multiply_parser).repeated())
                    .map(|x| {
                        x.1.iter().fold(x.0, |left, right| match right.0 {
                            '+' => Expression::Addition(Box::new(left), Box::new(right.1.clone())),
                            '-' => {
                                Expression::Subtraction(Box::new(left), Box::new(right.1.clone()))
                            }
                            _ => panic!("Error: Unexpected operator {}", right.0),
                        })
                    }))
                .padded()
        })
    }

    pub fn type_parser() -> impl Parser<char, CustomType, Error = Cheap<char>> {
        recursive(|bf: Recursive<char, CustomType, _>| {
            ident()
                .padded()
                .map(String::from)
                .chain(
                    just('|')
                        .ignore_then(ident().padded().map(String::from))
                        .repeated(),
                )
                .map(|x| CustomType::Union(x))
                .or(bf
                    .clone()
                    .chain(just(',').ignore_then(bf.clone()).repeated())
                    .or_not()
                    .flatten()
                    .delimited_by('(', ')')
                    .then_ignore(just(':'))
                    .then(bf.clone())
                    .map(|x: (Vec<CustomType>, CustomType)| {
                        CustomType::Callible(x.0, Box::new(x.1))
                    }))
        })
    }

    pub fn parser() -> impl Parser<char, Vec<Instr>, Error = Cheap<char>> {
        use Instr::*;
        recursive(|bf: Recursive<char, Vec<Instr>, _>| {
            let exp = exp_parser(bf.clone()).boxed();
            seq("extern".chars())
                .or_not()
                .map(|x| x.is_some())
                .then(
                    seq("let".chars())
                        .map(|_| false)
                        .or(seq("const".chars()).map(|_| true))
                        .padded(),
                )
                .then(ident().map(String::from))
                .then(type_specifyer().or_not())
                .then_ignore(just('='))
                .then(exp.clone())
                .map(|x| InitAssign(x.0 .0 .0 .0, x.0 .0 .0 .1, x.0 .0 .1, x.0 .1, x.1))
                .or(ident()
                    .map(String::from)
                    .then_ignore(just('='))
                    .then(exp.clone())
                    .map_with_span(|x, r| Assign(x.0, x.1, r)))
                .or(seq("while".chars())
                    .ignore_then(exp.clone())
                    .then(bf.clone().delimited_by('{', '}'))
                    .map_with_span(|x, span| Loop(x.0, x.1, span)))
                .or(seq("if".chars())
                    .ignore_then(exp.clone())
                    .then(
                        bf.clone()
                            .delimited_by('{', '}')
                            .then_ignore(seq("else".chars()).padded())
                            .then(bf.clone().delimited_by('{', '}')),
                    )
                    .map_with_span(|x, r| IfElse(x.0, x.1 .0, x.1 .1, r)))
                .or(seq("type".chars())
                    .ignore_then(ident().padded().map(String::from))
                    .then_ignore(just('=').padded())
                    .then(type_parser())
                    .map_with_span(|x, r| TypeDeclaration(x.0, x.1, r)))
                .or(exp.map_with_span(LoneExpression).padded())
                .recover_with(nested_delimiters('{', '}', [], |_| {
                    Invalid("Syntax error".to_string())
                }))
                .recover_with(skip_then_retry_until(['}']))
                .padded()
                .repeated()
        })
        .then_ignore(end())
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use chumsky::Parser;

    use super::parser::parser;
    use crate::ast::ast::*;
    #[test]
    fn add_expression() {
        use Expression::*;
        assert_eq!(
            parser().parse("5+2").unwrap(),
            vec![Instr::LoneExpression(
                Addition(
                    Box::new(Terminal(Symbol::Data(RawData::Int(5)))),
                    Box::new(Terminal(Symbol::Data(RawData::Int(2))))
                ),
                Range { start: 0, end: 3 }
            )]
        );
    }

    #[test]
    fn add_mult_add_expression() {
        use Expression::*;
        assert_eq!(
            parser().parse("5+2*4+8").unwrap(),
            vec![Instr::LoneExpression(
                Addition(
                    Box::new(Addition(
                        Box::new(Terminal(Symbol::Data(RawData::Int(5)))),
                        Box::new(Multiplication(
                            Box::new(Terminal(Symbol::Data(RawData::Int(2)))),
                            Box::new(Terminal(Symbol::Data(RawData::Int(4))))
                        ))
                    )),
                    Box::new(Terminal(Symbol::Data(RawData::Int(8))))
                ),
                Range { start: 0, end: 7 }
            )]
        );
    }
}
