pub struct Arg<'d> {
    pub name: &'d str,
    description: Option<&'d str>,
    help: Option<&'d str>,
    long: Option<&'d str>,
    short: Option<char>,

    flag: bool,
    takes_value: bool,
}

impl<'d> Arg<'d> {
    pub fn new(name: &str) -> Arg {
        Arg {
            name,
            description: None,
            help: None,
            long: None,
            short: None,

            flag: false,
            takes_value: false,
        }
    }

    pub fn description(mut self, des: &'d str) -> Self {
        self.description = Some(des);
        self
    }

    pub fn help(mut self, h: &'d str) -> Self {
        self.help = Some(h);
        self
    }

    pub fn long(mut self, l: &'d str) -> Self {
        self.long = Some(l);
        self
    }

    pub fn short(mut self, s: char) -> Self {
        self.short = Some(s);
        self
    }

    pub fn flag(mut self) -> Self {
        self.flag = true;
        self
    }

    pub fn takes_value(mut self, b: bool) -> Self {
        self.takes_value = b;
        self
    }  
}


