#![allow(dead_code)]

use std::collections::hash_map::HashMap;

use crate::parser::Parser;
use crate::arg;

pub struct Program<'d> {
    parser: Parser,
    args: HashMap<&'d str, arg::Arg<'d>>,
}

impl<'d> Program<'d> {
    pub fn new(args: Vec<String>) -> Program<'d> {

        Program {  
            parser: Parser::new(args),
            args: HashMap::new(),
        }
    }

    pub fn arg(mut self, arg: arg::Arg<'d>) -> Self {
        self.args.insert(arg.name, arg);
        self
    }
}
