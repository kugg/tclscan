#![feature(plugin)]
#![plugin(docopt_macros)]

extern crate rustc_serialize;
extern crate docopt;
extern crate tclscan;

use std::error::Error;
use std::fs;
use std::io::prelude::*;
use std::io;
use std::path::Path;
use docopt::Docopt;
use tclscan::rstcl;
use tclscan::CheckResult;

docopt!(Args derive Debug, "
Usage: tclscan check [--no-warn] ( - | <path> )
       tclscan parsestr ( - | <script-str> )
");

pub fn main() {
    let args: Args = Args::docopt().decode().unwrap_or_else(|e| e.exit());
    let take_stdin = args.cmd__;
    let script_in = match (args.cmd_check, args.cmd_parsestr, take_stdin) {
        (true, false, false) => {
            let path = Path::new(&args.arg_path);
            let path_display = path.display();
            let mut file = match fs::File::open(&path) {
                Err(err) => panic!("ERROR: Couldn't open {}: {}",
                                   path_display, Error::description(&err)),
                Ok(file) => file,
            };
            let mut file_content = String::new();
            match file.read_to_string(&mut file_content) {
                Err(err) => panic!("ERROR: Couldn't read {}: {}",
                                   path_display, Error::description(&err)),
                Ok(_) => file_content,
            }
        },
        (true, false, true) |
        (false, true, true) => {
            let mut stdin_content = String::new();
            match io::stdin().read_to_string(&mut stdin_content) {
                Err(err) => panic!("ERROR: Couldn't read stdin: {}",
                                   Error::description(&err)),
                Ok(_) => stdin_content,
            }
        },
        (false, true, false) => args.arg_script_str,
        _ => panic!("Internal error: could not load script"),
    };
    let script = &script_in;
    match (args.cmd_check, args.cmd_parsestr) {
        (true, false) => {
            let mut results = tclscan::scan_script(script);
            if args.flag_no_warn {
                results = results.into_iter().filter(|r|
                    match r { &CheckResult::Warn(_, _, _) => false,  _ => true }
                ).collect();
            }
            if results.len() > 0 {
                for check_result in results.iter() {
                    println!("{}", check_result);
                }
                println!("");
            };
        },
        (false, true) =>
            println!("{:?}", rstcl::parse_script(script)),
        _ =>
            panic!("Internal error: invalid operation"),
    }
}
