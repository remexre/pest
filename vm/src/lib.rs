// pest. The Elegant Parser
// Copyright (c) 2018 Dragoș Tiselice
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

extern crate pest;
extern crate pest_meta;

use std::char;
use std::collections::HashMap;

use pest::{Atomicity, Error, ParserState, Position};
use pest::iterators::Pairs;

use pest_meta::ast::{Expr, Rule, RuleType};

mod macros;

pub struct Vm {
    rules: HashMap<String, Rule>
}

impl Vm {
    pub fn new(rules: Vec<Rule>) -> Vm {
        let rules = rules.into_iter().map(|r| (r.name.clone(), r)).collect();
        Vm { rules }
    }

    pub fn parse<'a, 'i>(
        &'a self,
        rule: &'a str,
        input: &'i str
    ) -> Result<Pairs<'i, &str>, Error<'i, &str>> {
        pest::state(input, |mut state, pos| {
            self.parse_rule(rule, pos, &mut state)
        })
    }

    fn parse_rule<'a, 'i>(
        &'a self,
        rule: &'a str,
        pos: Position<'i>,
        state: &mut ParserState<'i, &'a str>
    ) -> Result<Position<'i>, Position<'i>> {
        match rule {
            "any" => return pos.skip(1),
            "eoi" => return state.rule("eoi", pos, |_, pos| pos.at_end()),
            "soi" => return pos.at_start(),
            "peek" => {
                return {
                    let string = state
                        .stack
                        .last()
                        .expect("peek was called on empty stack")
                        .as_str();
                    pos.match_string(string)
                }
            }
            "pop" => {
                return {
                    let pos = {
                        let string = state
                            .stack
                            .last()
                            .expect("pop was called on empty stack")
                            .as_str();

                        pos.match_string(string)
                    };

                    if pos.is_ok() {
                        state.stack.pop().unwrap();
                    }

                    pos
                }
            }
            _ => ()
        };

        if let Some(rule) = self.rules.get(rule) {
            if &rule.name == "whitespace" || &rule.name == "comment" {
                match rule.ty {
                    RuleType::Normal => state.rule(&rule.name, pos, |state, pos| {
                        state.atomic(Atomicity::Atomic, move |state| {
                            self.parse_expr(&rule.expr, pos, state)
                        })
                    }),
                    RuleType::Silent => state.atomic(Atomicity::Atomic, move |state| {
                        self.parse_expr(&rule.expr, pos, state)
                    }),
                    RuleType::Atomic => state.rule(&rule.name, pos, |state, pos| {
                        state.atomic(Atomicity::Atomic, move |state| {
                            self.parse_expr(&rule.expr, pos, state)
                        })
                    }),
                    RuleType::CompoundAtomic => {
                        state.atomic(Atomicity::CompoundAtomic, move |state| {
                            state.rule(&rule.name, pos, |state, pos| {
                                self.parse_expr(&rule.expr, pos, state)
                            })
                        })
                    }
                    RuleType::NonAtomic => state.atomic(Atomicity::Atomic, move |state| {
                        state.rule(&rule.name, pos, |state, pos| {
                            self.parse_expr(&rule.expr, pos, state)
                        })
                    })
                }
            } else {
                match rule.ty {
                    RuleType::Normal => state.rule(&rule.name, pos, |state, pos| {
                        self.parse_expr(&rule.expr, pos, state)
                    }),
                    RuleType::Silent => self.parse_expr(&rule.expr, pos, state),
                    RuleType::Atomic => state.rule(&rule.name, pos, |state, pos| {
                        state.atomic(Atomicity::Atomic, move |state| {
                            self.parse_expr(&rule.expr, pos, state)
                        })
                    }),
                    RuleType::CompoundAtomic => {
                        state.atomic(Atomicity::CompoundAtomic, move |state| {
                            state.rule(&rule.name, pos, |state, pos| {
                                self.parse_expr(&rule.expr, pos, state)
                            })
                        })
                    }
                    RuleType::NonAtomic => state.atomic(Atomicity::NonAtomic, move |state| {
                        state.rule(&rule.name, pos, |state, pos| {
                            self.parse_expr(&rule.expr, pos, state)
                        })
                    })
                }
            }
        } else {
            panic!("undefined rule {}", rule);
        }
    }

    fn parse_expr<'a, 'i>(
        &'a self,
        expr: &'a Expr,
        pos: Position<'i>,
        state: &mut ParserState<'i, &'a str>
    ) -> Result<Position<'i>, Position<'i>> {
        match *expr {
            Expr::Str(ref string) => {
                pos.match_string(&unescape(string).expect("incorrect string literal"))
            }
            Expr::Insens(ref string) => {
                pos.match_insensitive(&unescape(string).expect("incorrect string literal"))
            }
            Expr::Range(ref start, ref end) => {
                let start = unescape(start)
                    .expect("incorrect char literal")
                    .chars()
                    .next()
                    .expect("empty char literal");
                let end = unescape(end)
                    .expect("incorrect char literal")
                    .chars()
                    .next()
                    .expect("empty char literal");

                pos.match_range(start..end)
            }
            Expr::Ident(ref name) => self.parse_rule(name, pos, state),
            Expr::PosPred(ref expr) => state.lookahead(true, move |state| {
                pos.lookahead(true, |pos| self.parse_expr(&*expr, pos, state))
            }),
            Expr::NegPred(ref expr) => state.lookahead(false, move |state| {
                pos.lookahead(false, |pos| self.parse_expr(&*expr, pos, state))
            }),
            Expr::Seq(ref lhs, ref rhs) => state.sequence(move |state| {
                pos.sequence(|pos| {
                    self.parse_expr(&*lhs, pos, state)
                        .and_then(|pos| self.skip(pos, state))
                        .and_then(|pos| self.parse_expr(&*rhs, pos, state))
                })
            }),
            Expr::Choice(ref lhs, ref rhs) => self.parse_expr(&*lhs, pos, state)
                .or_else(|pos| self.parse_expr(&*rhs, pos, state)),
            Expr::Opt(ref expr) => pos.optional(|pos| self.parse_expr(&*expr, pos, state)),
            Expr::Rep(ref expr) => self.repeat(expr, None, None, pos, state),
            Expr::RepOnce(ref expr) => self.repeat(expr, Some(1), None, pos, state),
            Expr::RepExact(ref expr, num) => self.repeat(expr, Some(num), Some(num), pos, state),
            Expr::RepMin(ref expr, min) => self.repeat(expr, Some(min), None, pos, state),
            Expr::RepMax(ref expr, max) => self.repeat(expr, None, Some(max), pos, state),
            Expr::RepMinMax(ref expr, min, max) => {
                self.repeat(expr, Some(min), Some(max), pos, state)
            }
            Expr::Push(ref expr) => {
                let start = pos.clone();

                match self.parse_expr(&*expr, pos, state) {
                    Ok(end) => {
                        state.stack.push(start.span(&end));
                        Ok(end)
                    }
                    Err(pos) => Err(pos)
                }
            }
            Expr::Skip(ref strings) => strings[1..].iter().fold(
                pos.clone().skip_until(&strings[0]),
                |result, string| match (result, pos.clone().skip_until(string)) {
                    (Ok(lhs), Ok(rhs)) => {
                        if rhs.pos() < lhs.pos() {
                            Ok(rhs)
                        } else {
                            Ok(lhs)
                        }
                    }
                    (Ok(lhs), Err(_)) => Ok(lhs),
                    (Err(_), Ok(rhs)) => Ok(rhs),
                    (Err(lhs), Err(_)) => Err(lhs)
                }
            )
        }
    }

    fn repeat<'a, 'i>(
        &'a self,
        expr: &'a Expr,
        min: Option<u32>,
        max: Option<u32>,
        pos: Position<'i>,
        state: &mut ParserState<'i, &'a str>
    ) -> Result<Position<'i>, Position<'i>> {
        state.sequence(move |state| {
            pos.sequence(|pos| {
                let mut result = match min {
                    Some(min) if min > 0 => {
                        let mut result = self.parse_expr(&*expr, pos, state);

                        for _ in 2..min + 1 {
                            result = result.and_then(|pos| {
                                self.skip(pos, state)
                                    .and_then(|pos| self.parse_expr(&*expr, pos, state))
                            });
                        }

                        result
                    }
                    _ => pos.optional(|pos| self.parse_expr(&*expr, pos, state))
                };

                let mut times = 1;

                loop {
                    if let Some(max) = max {
                        if times >= max {
                            return result;
                        }
                    }

                    let current = result.clone().and_then(|pos| {
                        self.skip(pos, state)
                            .and_then(|pos| self.parse_expr(&*expr, pos, state))
                    });
                    times += 1;

                    if current.is_err() {
                        return result;
                    }

                    result = current;
                }
            })
        })
    }

    fn skip<'a, 'i>(
        &'a self,
        pos: Position<'i>,
        state: &mut ParserState<'i, &'a str>
    ) -> Result<Position<'i>, Position<'i>> {
        match (
            self.rules.contains_key("whitespace"),
            self.rules.contains_key("comment")
        ) {
            (false, false) => Ok(pos),
            (true, false) => if state.atomicity == Atomicity::NonAtomic {
                pos.repeat(|pos| self.parse_rule("whitespace", pos, state))
            } else {
                Ok(pos)
            },
            (false, true) => if state.atomicity == Atomicity::NonAtomic {
                pos.repeat(|pos| self.parse_rule("comment", pos, state))
            } else {
                Ok(pos)
            },
            (true, true) => if state.atomicity == Atomicity::NonAtomic {
                state.sequence(move |state| {
                    pos.sequence(|pos| {
                        pos.repeat(|pos| self.parse_rule("whitespace", pos, state))
                            .and_then(|pos| {
                                pos.repeat(|pos| {
                                    state.sequence(move |state| {
                                        pos.sequence(|pos| {
                                            self.parse_rule("comment", pos, state).and_then(|pos| {
                                                pos.repeat(|pos| {
                                                    self.parse_rule("whitespace", pos, state)
                                                })
                                            })
                                        })
                                    })
                                })
                            })
                    })
                })
            } else {
                Ok(pos)
            }
        }
    }
}

fn unescape(string: &str) -> Option<String> {
    let mut result = String::new();
    let mut chars = string.chars();

    loop {
        match chars.next() {
            Some('\\') => match chars.next()? {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                'r' => result.push('\r'),
                'n' => result.push('\n'),
                't' => result.push('\t'),
                '0' => result.push('\0'),
                '\'' => result.push('\''),
                'x' => {
                    let string: String = chars.clone().take(2).collect();

                    if string.len() != 2 {
                        return None;
                    }

                    for _ in 0..string.len() {
                        chars.next()?;
                    }

                    let value = u8::from_str_radix(&string, 16).ok()?;

                    result.push(char::from(value));
                }
                'u' => {
                    if chars.next()? != '{' {
                        return None;
                    }

                    let string: String = chars.clone().take_while(|c| *c != '}').collect();

                    if string.len() < 2 || 6 < string.len() {
                        return None;
                    }

                    for _ in 0..string.len() + 1 {
                        chars.next()?;
                    }

                    let value = u32::from_str_radix(&string, 16).ok()?;

                    result.push(char::from_u32(value)?);
                }
                _ => return None
            },
            Some(c) => result.push(c),
            None => return Some(result)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_all() {
        let string = r"a\nb\x55c\u{111}d";

        assert_eq!(unescape(string), Some("a\nb\x55c\u{111}d".to_owned()));
    }

    #[test]
    fn unescape_empty_escape() {
        let string = r"\";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_wrong_escape() {
        let string = r"\w";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_wrong_byte() {
        let string = r"\xfg";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_short_byte() {
        let string = r"\xf";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_no_open_brace_unicode() {
        let string = r"\u11";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_no_close_brace_unicode() {
        let string = r"\u{11";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_short_unicode() {
        let string = r"\u{1}";

        assert_eq!(unescape(string), None);
    }

    #[test]
    fn unescape_long_unicode() {
        let string = r"\u{1111111}";

        assert_eq!(unescape(string), None);
    }
}
