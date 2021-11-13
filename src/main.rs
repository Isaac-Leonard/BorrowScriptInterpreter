mod shared;
mod parser;
mod evaluater;
use chumsky::Parser;
use parser::parser::*;
use std::{
    cell::RefCell,
    env, fs,
    io::{self, BufRead},
    rc::Rc,
};
use shared::*;
use evaluater::evaluater::execute;
fn create_type_error(
    op_name: &str,
    touple: (Result<LangType, Vec<String>>, Result<LangType, Vec<String>>),
) -> Result<LangType, Vec<String>> {
    // Check if both branches failed, if so merge the errors and return them.
    // Otherwise check each branch for errors and return it
    // Otherwise the successful types don't match at this point so create a new error and return it
    match touple {
        // TODO: Maybe change these to linkedlists for less copying and mutibility
        (Err(mut l), Err(mut r)) => {
            l.append(&mut r);
            Err(l)
        }
        // TODO: I feel like this should be optemisible, directly return each branch instead of unwrapping them perhaps?
        (Err(l), _) => Err(l),
        (_, Err(r)) => Err(r),
        (l, r) => Err(vec![format!(
            "The '{}' operator cannot be used on differing types: '{:?} and '{:?}'",
            op_name, l, r
        )]),
    }
}

fn get_exp_type(
    exp: &Expression,
    variables: &mut Vec<TypeDescriptor>,
    types: &mut Vec<TypeDescriptor>,
    global: bool,
) -> Result<LangType, Vec<String>> {
    use Expression::*;
    use LangType::*;
    match exp {
        Terminal(sym) => match sym {
            Symbol::Data(RawData::Func(Function {
                args,
                body,
                call: _,
                return_type,
            }))
            | Symbol::Data(RawData::ActiveFunc(ActiveFunction {
                args,
                body,
                stack: _,
                call: _,
                return_type,
            })) => {
                let arg_types: Result<Vec<TypeDescriptor>, Vec<String>> = args
                    .iter()
                    .map(|x| {
                        let shape = consolidate_type(&x.1, types).map_err(|x| {
                            vec![format!("Attempted to use undeclared types {:?}", x)]
                        });
                        if let Err(e) = shape {
                            return Err(e);
                        }
                        let shape = shape.unwrap();
                        Ok(TypeDescriptor {
                            name: x.0.clone(),
                            shape,
                        })
                    })
                    .collect();
                if let Err(e) = arg_types {
                    return Err(e);
                };
                let arg_types = arg_types.unwrap();
                println!("arg_types: {:?}", arg_types);
                let mut variables_with_args = variables.clone();
                variables_with_args.append(&mut arg_types.clone());
                let ret = consolidate_type(return_type, types);
                if ret.is_err() {
                    ret
                } else {
                    let final_errors = type_check(body, &mut variables_with_args, types, false);
                    if final_errors.len() > 0 {
                        Err(final_errors)
                    } else {
                        // Replace this when we implement defined return types on functions
                        Ok(LangType::Func(
                            arg_types.iter().map(|x| x.shape.clone()).collect(),
                            Box::new(ret.unwrap()),
                        ))
                    }
                }
            }
            Symbol::Data(data) => get_type(data, types),
            Symbol::Identifier(name) => variables
                .iter()
                .find(|x| x.name == *name)
                .map(|x| x.shape.clone())
                .ok_or(vec![format!(
                    "Cannot access undeclared variable '{}'",
                    name
                )]),
        },
        Addition(x, y) => match (
            get_exp_type(x, variables, types, global),
            get_exp_type(y, variables, types, global),
        ) {
            (Ok(x), Ok(y)) => match (x, y) {
                (Int, Int) => Ok(Int),
                (Str, Str) | (Str, Int) | (Int, Str) | (Null, Null) => Ok(Str),
                // Let the rest go to the error manager
                (x, y) => create_type_error("+", (Ok(x), Ok(y))),
            },
            invalid => create_type_error("+", invalid),
        },
        Subtraction(x, y) | Multiplication(x, y) | Division(x, y) => {
            match (
                get_exp_type(x, variables, types, global),
                get_exp_type(y, variables, types, global),
            ) {
                (Ok(LangType::Int), Ok(LangType::Int)) => Ok(LangType::Int),
                // Kinda cheating here with the op_name param
                invalid => create_type_error("+' or '*' or '/", invalid),
            }
        }
        LessThan(x, y) => match (
            get_exp_type(x, variables, types, global),
            get_exp_type(y, variables, types, global),
        ) {
            (Ok(Int), Ok(Int)) => Ok(Bool),
            // More cheeting here
            invalid => create_type_error("<' or '=>' or '>=' or '>", invalid),
        },
        Equal(l, r) => match (
            get_exp_type(l, variables, types, global),
            get_exp_type(r, variables, types, global),
        ) {
            (Ok(x), Ok(y)) => match (x, y) {
                (Int, Int) | (Str, Str) | (Bool, Bool) => Ok(Bool),
                (l, r) => create_type_error("==", (Ok(l), Ok(r))),
            },
            invalid => create_type_error("==", invalid),
        },
        FuncCall(name, args) => {
            let func_type = variables
                .iter()
                .find(|x| x.name == *name)
                .map(|x| x.shape.clone());
            if let None = func_type {
                return Err(vec![format!(
                    "Attempted to access undeclared variable '{}'",
                    name
                )]);
            }
            let func = func_type.unwrap();
            if let Func(expected_args, ret) = func {
                if args.len() != expected_args.len() {
                    return Err(vec![format!(
                        "Function '{}' called with {} args but it expects {}",
                        name,
                        args.len(),
                        expected_args.len(),
                    )]);
                }
                let mismatched_args=                args.iter().zip(expected_args).filter_map(|x| {
                    let passed_type = get_exp_type(x.0, variables, types, global);
		    if let Err(error)=passed_type{return Some(error.clone());}
                    if !types_match(&passed_type.clone().unwrap(), &x.1) {Some(vec![format!("Attempted to pass type {:?} as argument to {} but {} expected {:?} at that position", passed_type, name,name, x.1)])}else{None}
                }).flatten().map(|x|x.clone()).collect::<Vec<_>>();
                if mismatched_args.len() != 0 {
                    Err(mismatched_args)
                } else {
                    Ok(*ret.clone())
                }
            } else {
                Err(vec![format!(
                    "Attempted to call '{}' as a function but '{}' has type '{:?}'",
                    name, name, func
                )])
            }
        }
    }
}


fn match_type_against_union(sup: Vec<LangType>, sub: LangType) -> bool {
    sup.iter().find(|x| types_match(x, &sub)).is_some()
}

fn types_match(a: &LangType, b: &LangType) -> bool {
    use LangType::*;
    match (a, b) {
        (Int, Int) => true,
        (Bool, Bool) => true,
        (Str, Str) => true,
        (Null, Null) => true,
        (Union(sup), sub) | (sub, Union(sup)) => match_type_against_union(sup.clone(), sub.clone()),
        _ => false,
    }
}

fn type_check_statement(
    stat: &Instr,
    variables: &mut Vec<TypeDescriptor>,
    types: &mut Vec<TypeDescriptor>,
    global: bool,
) -> Vec<String> {
    use Instr::*;
    return match stat {
        Assign(name, exp) => {
            let assigned_type = get_exp_type(exp, variables, types, global);
            if let Ok(shape) = assigned_type {
                variables.push(TypeDescriptor {
                    name: name.clone(),
                    shape: shape.clone(),
                });
            } else if let Err(e) = assigned_type {
                return e;
            };
            return Vec::new();
        }
        LoneExpression(exp) => match get_exp_type(exp, variables, types, global) {
            Err(e) => return e,
            _ => return Vec::new(),
        },
        Loop(check, body) => {
            if global {
                vec!["Cannot use loops outside of a function".to_string()]
            } else {
                let check_type = get_exp_type(check, variables, types, global);
                if let Err(e) = check_type {
                    return e;
                } else if let Ok(LangType::Bool) = check_type {
                    // Dont check for errors just return Err here because if the returned Vec is empty then there'll be no effect but if its not they'll be automatically added
                    return type_check(body, variables, types, global);
                } else {
                    return vec![
			format!(                        "The check expression in a while loop must result in a boolean value, not {:?}"
							 ,check_type.unwrap())
                    ];
                }
            }
        }
        Invalid(msg) => vec![msg.clone()],
    };
}

fn type_check(
    ast: &[Instr],
    variables: &mut Vec<TypeDescriptor>,
    types: &mut Vec<TypeDescriptor>,
    global: bool,
) -> Vec<String> {
    println!("");
    variables.iter().for_each(|x| println!("{:?}", x));
    println!("");
    let mut errors = Vec::new();
    for sym in ast {
        // typecheck each statement and unwrap the error result automatically so we can push it to the main list, otherwise do nothing
        errors.append(&mut type_check_statement(&sym, variables, types, global))
    }
    return errors;
}

fn main() {
    let lang_print = ActiveFunction {
        args: vec![("str".into(), vec!["string".into()])],
        body: Vec::new(),
        call: |args, params, _, _, _| {
            if args.len() != params.len() {
                panic!("Function called with invalid parameters {:?}", params)
            }
            println!("{:?}", params[0]);
            return RawData::Null;
        },
        stack: ScopeRef(Rc::new(RefCell::new(Scope {
            variables: Vec::new(),
            parent: None,
            types: Vec::new(),
        }))),
        return_type: vec!["null".to_string()],
    };
    let lang_input = ActiveFunction {
        args: Vec::new(),
        body: Vec::new(),
        call: |args, params, _, _, _| {
            if args.len() != params.len() {
                panic!("Function called with invalid parameters {:?}", params)
            }
            return RawData::Str(io::stdin().lock().lines().next().unwrap().unwrap());
        },
        stack: ScopeRef(Rc::new(RefCell::new(Scope {
            variables: Vec::new(),
            parent: None,
            types: Vec::new(),
        }))),
        return_type: vec!["string".to_string()],
    };

    let src = fs::read_to_string(env::args().nth(1).expect("Expected file argument"))
        .expect("Failed to read file");

    // let src = "[!]+";
    let variables = ScopeRef(Rc::new(RefCell::new(Scope {
        parent: None,
        variables: vec![
            Variable {
                name: "print".to_string(),
                value: RawData::ActiveFunc(lang_print),
                data_type: LangType::Func(
                    vec![LangType::Union(vec![
                        LangType::Str,
                        LangType::Int,
                        LangType::Bool,
                        LangType::Null,
                    ])],
                    Box::new(LangType::Null),
                ),
            },
            Variable {
                name: "getInput".to_string(),
                value: RawData::ActiveFunc(lang_input),
                data_type: LangType::Func(Vec::new(), Box::new(LangType::Str)),
            },
        ],
        types: Vec::new(),
    })));
    let mut types = vec![
        TypeDescriptor {
            name: "int".to_string(),
            shape: LangType::Int,
        },
        TypeDescriptor {
            name: "string".to_string(),
            shape: LangType::Str,
        },
        TypeDescriptor {
            name: "null".to_string(),
            shape: LangType::Null,
        },
    ];

    let parsed = parser().parse(src.trim());
    println!("{:?}", parsed);
    match parsed {
        Ok(ast) => {
            println!("Parsing succeeded");
            let errors = type_check(
                &ast,
                &mut variables
                    .0
                    .as_ref()
                    .borrow()
                    .variables
                    .iter()
                    .map(|x| TypeDescriptor {
                        name: x.name.clone(),
                        shape: x.data_type.clone(),
                    })
                    .collect(),
                &mut types,
                true,
            );
            if errors.len() > 0 {
                errors.iter().for_each(|x| println!("{}", x))
            } else {
                execute(&ast, variables, &types, true);
            }
        }
        Err(errs) => errs.into_iter().for_each(|e| println!("{:?}", e)),
    }
}
 
