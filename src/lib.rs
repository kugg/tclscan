#![feature(slicing_syntax)]
#![feature(core)]
#![feature(collections)]
#![feature(libc)]
#![feature(std_misc)]

extern crate libc;

use std::iter;
use std::fmt;
use self::CheckResult::*; // TODO: why does swapping this line with one below break?
use rstcl::TokenType;

pub mod rstcl;
#[allow(dead_code, non_upper_case_globals, non_camel_case_types, non_snake_case, raw_pointer_derive)]
mod tcl;

// http://www.tcl.tk/doc/howto/stubs.html
// Ideally would use stubs but they seem to not work

// When https://github.com/crabtw/rust-bindgen/issues/89 is fixed
//#![feature(phase)]
//#[phase(plugin)] extern crate bindgen;
//
//#[allow(dead_code, uppercase_variables, non_camel_case_types)]
//mod tcl_bindings {
//    bindgen!("./mytcl.h", match="tcl.h", link="tclstub")
//}

#[derive(PartialEq)]
pub enum CheckResult<'a> {
    Warn(&'static str, &'a str),
    Danger(&'static str, &'a str),
}
impl<'b> fmt::Display for CheckResult<'b> {
    fn fmt<'a>(&'a self, f: &mut fmt::Formatter) -> fmt::Result {
        return match self {
            &Warn(msg, line) => write!(f, "WARN: {} for\n{}", msg, line),
            &Danger(msg, line) => write!(f, "DANGER: {} for\n{}", msg, line),
        };
    }
}

#[derive(Clone)]
enum Code {
    Block,
    Expr,
    Literal,
    Normal,
}

fn check_literal<'a, 'b>(token: &'b rstcl::TclToken<'a>) -> Vec<CheckResult<'a>> {
    let token_str = token.val;
    assert!(token_str.len() > 0);
    return if token_str.char_at(0) == '{' {
        vec![]
    } else if token_str.contains_char('$') {
        vec![Danger("Expected literal, found $", token_str)]
    } else if token_str.contains_char('[') {
        vec![Danger("Expected literal, found [", token_str)]
    } else {
        vec![]
    }
}

// Does this variable only contain safe characters?
// Only used by is_safe_val
fn is_safe_var(token: &rstcl::TclToken) -> bool {
    assert!(token.ttype == TokenType::Variable);
    return false
}

// Does the return value of this function only contain safe characters?
// Only used by is_safe_val.
fn is_safe_cmd(token: &rstcl::TclToken) -> bool {
    let string = token.val;
    assert!(string.starts_with("[") && string.ends_with("]"));
    let script = &string[1..string.len()-1];
    let parses = rstcl::parse_script(script);
    // Empty script
    if parses.len() == 0 {
        return true;
    }
    let token_strs: Vec<&str> = parses[0].tokens.iter().map(|e| e.val).collect();
    return match &token_strs[] {
        ["info", "exists", ..] |
        ["catch", ..] => true,
        _ => false,
    };
}

// Check whether a value can ever cause or assist in any security flaw i.e.
// whether it may contain special characters.
// We do *not* concern ourselves with vulnerabilities in sub-commands. That
// should happen elsewhere.
fn is_safe_val(token: &rstcl::TclToken) -> bool {
    assert!(token.val.len() > 0);
    for tok in token.iter() {
        let is_safe = match tok.ttype {
            TokenType::Variable => is_safe_var(tok),
            TokenType::Command => is_safe_cmd(tok),
            _ => true,
        };
        if !is_safe {
            return false;
        }
    }
    return true;
}

/// Checks if a parsed command is insecure
///
/// ```
/// use tclscan::rstcl::parse_command as p;
/// use tclscan::check_command as c;
/// use tclscan::CheckResult::{Danger,Warn};
/// assert!(c(&p("puts x").0.tokens) == vec![]);
/// assert!(c(&p("puts [x]").0.tokens) == vec![]);
/// assert!(c(&p("puts [x\n ]").0.tokens) == vec![]);
/// assert!(c(&p("puts [x;y]").0.tokens) == vec![]);
/// assert!(c(&p("puts [x;eval $y]").0.tokens) == vec![Danger("Dangerous unquoted block", "$y")]);
/// assert!(c(&p("puts [eval $x]").0.tokens) == vec![Danger("Dangerous unquoted block", "$x")]);
/// assert!(c(&p("expr {[blah]}").0.tokens) == vec![]);
/// assert!(c(&p("expr \"[blah]\"").0.tokens) == vec![Danger("Dangerous unquoted expr", "\"[blah]\"")]);
/// assert!(c(&p("expr {\\\n0}").0.tokens) == vec![]);
/// assert!(c(&p("expr {[expr \"[blah]\"]}").0.tokens) == vec![Danger("Dangerous unquoted expr", "\"[blah]\"")]);
/// assert!(c(&p("if [info exists abc] {}").0.tokens) == vec![Warn("Unquoted expr", "[info exists abc]")]);
/// assert!(c(&p("if [abc] {}").0.tokens) == vec![Danger("Dangerous unquoted expr", "[abc]")]);
/// assert!(c(&p("a${x} blah").0.tokens) == vec![Warn("Non-literal command, cannot scan", "a${x}")]);
/// assert!(c(&p("set a []").0.tokens) == vec![]);
/// ```
pub fn check_command<'a, 'b>(tokens: &'b Vec<rstcl::TclToken<'a>>) -> Vec<CheckResult<'a>> {
    let mut results = vec![];
    // First check all subcommands which will be substituted
    for tok in tokens.iter() {
        for subtok in tok.iter().filter(|tok| tok.ttype == TokenType::Command) {
            results.extend(scan_command(subtok.val).into_iter());
        }
    }
    // The empty command (e.g. `[]`)
    if tokens.len() == 0 {
        return results;
    }
    // Now check if the command name itself isn't a literal
    if check_literal(&tokens[0]).into_iter().len() > 0 {
        results.push(Warn("Non-literal command, cannot scan", tokens[0].val));
        return results;
    }
    // Now check the command-specific interpretation of arguments etc
    let param_types = match tokens[0].val {
        // eval script
        "eval" => iter::repeat(Code::Block).take(tokens.len()-1).collect(),
        // catch script [result]? [options]?
        "catch" => {
            let mut param_types = vec![Code::Block];
            if tokens.len() == 3 || tokens.len() == 4 {
                let new_params: Vec<Code> = iter::repeat(Code::Literal).take(tokens.len()-2).collect();
                param_types.push_all(&new_params[]);
            }
            param_types
        }
        // expr [arg]+
        "expr" => tokens[1..].iter().map(|_| Code::Expr).collect(),
        // proc name args body
        "proc" => vec![Code::Literal, Code::Literal, Code::Block],
        // for init cond iter body
        "for" => vec![Code::Block, Code::Expr, Code::Block, Code::Block],
        // foreach [varname list]+ body
        "foreach" => vec![Code::Literal, Code::Normal, Code::Block],
        // while cond body
        "while" => vec![Code::Expr, Code::Block],
        // if cond body [elseif cond body]* [else body]?
        "if" => {
            let mut param_types = vec![Code::Expr, Code::Block];
            let mut i = 3;
            while i < tokens.len() {
                param_types.push_all(&match tokens[i].val {
                    "elseif" => vec![Code::Literal, Code::Expr, Code::Block],
                    "else" => vec![Code::Literal, Code::Block],
                    _ => { break; },
                }[]);
                i = param_types.len() + 1;
            }
            param_types
        },
        _ => iter::repeat(Code::Normal).take(tokens.len()-1).collect(),
    };
    if param_types.len() != tokens.len() - 1 {
        results.push(Warn("badly formed command", tokens[0].val));
        return results;
    }
    for (param_type, param) in param_types.iter().zip(tokens[1..].iter()) {
        let check_results: Vec<CheckResult<'a>> = match *param_type {
            Code::Block => check_block(param),
            Code::Expr => check_expr(param),
            Code::Literal => check_literal(param),
            Code::Normal => vec![],
        };
        results.extend(check_results.into_iter());
    }
    return results;
}

/// Scans a block (i.e. should be quoted) for danger
fn check_block<'a, 'b>(token: &'b rstcl::TclToken<'a>) -> Vec<CheckResult<'a>> {
    let block_str = token.val;
    if !(block_str.starts_with("{") && block_str.ends_with("}")) {
        return vec!(match is_safe_val(token) {
            true => Warn("Unquoted block", block_str),
            false => Danger("Dangerous unquoted block", block_str),
        });
    }
    // Block isn't inherently dangerous, let's check functions inside the block
    let script_str = &block_str[1..block_str.len()-1];
    return scan_script(script_str);
}

/// Scans an expr (i.e. should be quoted) for danger
fn check_expr<'a, 'b>(token: &'b rstcl::TclToken<'a>) -> Vec<CheckResult<'a>> {
    let mut results = vec![];
    let expr_str = token.val;
    if !(expr_str.starts_with("{") && expr_str.ends_with("}")) {
        results.push(match is_safe_val(token) {
            true => Warn("Unquoted expr", expr_str),
            false => Danger("Dangerous unquoted expr", expr_str),
        });
        return results;
    };
    // Technically this is the 'scan_expr' function
    // Expr isn't inherently dangerous, let's check functions inside the expr
    assert!(token.val.starts_with("{") && token.val.ends_with("}"));
    let expr = &token.val[1..token.val.len()-1];
    let (parse, remaining) = rstcl::parse_expr(expr);
    assert!(parse.tokens.len() == 1 && remaining == "");
    for tok in parse.tokens[0].iter().filter(|tok| tok.ttype == TokenType::Command) {
        results.extend(scan_command(tok.val).into_iter());
    }
    return results;
}

/// Scans a TokenType::Command token (contained in '[]') for danger
pub fn scan_command<'a>(string: &'a str) -> Vec<CheckResult<'a>> {
    assert!(string.starts_with("[") && string.ends_with("]"));
    let script = &string[1..string.len()-1];
    return scan_script(script);
}

/// Scans a sequence of commands for danger
pub fn scan_script<'a>(string: &'a str) -> Vec<CheckResult<'a>> {
    let mut all_results: Vec<CheckResult<'a>> = vec![];
    for parse in rstcl::parse_script(string) {
        // Skip empty parse at end of script
        if parse.tokens.len() == 0 {
            break;
        }
        let results = check_command(&parse.tokens);
        all_results.extend(results.into_iter());
    }
    return all_results;
}
