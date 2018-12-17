/*
Copyright 2018 Google LLC

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

// the "rich" text format is strongly work in progress, it was written primarily to check what would
// be the cost of integrating syntax highlighting from other editors. I decided to proceed with that
// approach, it's on the May 2018 roadmap.

// TODO(njskalski) secure with accessors after fixing the format.

use syntax;

use ropey::Rope;
use std::marker::Copy;
use std::borrow::Borrow;
use std::ops::{Index, IndexMut};
use std::iter::{Iterator, ExactSizeIterator};
use content_provider::RopeBasedContentProvider;
use std::cell::Ref;

use cursive::theme::Color;
use std::rc::Rc;

use syntect::highlighting::{Style, ThemeSet, Theme};
use syntect::parsing::{ParseState, SyntaxReference, SyntaxSet};
use std::cell::Cell;
use std::collections::HashMap;
use std::cell::RefCell;

const PARSING_MILESTONE : usize = 20;

#[derive(Debug)]
pub struct RichLine {
    length : usize,
    body : Vec<(Color, String)>
}

//TODO(njskalski): optimise, rethink api. maybe even drop the content.
impl RichLine {
    pub fn new(body : Vec<(Color, String)>) -> Self {
        let mut len : usize = 0;
        for piece in &body {
            len += piece.1.len()
        }

        RichLine { length : len , body }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn get_color_at(&self, idx : usize) -> Option<Color> {
        let mut cur_idx : usize = 0;

        for chunk in &self.body {
            if cur_idx + chunk.1.len() > idx {
                return Some(chunk.0)
            }
            cur_idx += chunk.1.len();
        }

        None
    }
}

#[derive(Debug)]
pub struct HighlightSettings {
//    theme : Theme,
    syntax : SyntaxReference,
    syntax_set : SyntaxSet,
    highlighter : Highlighter,
}

//TODO move const strings to settings parameters.
impl HighlightSettings {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines().clone();
        let ts = &ThemeSet::load_defaults();

        let theme = ts.themes["base16-ocean.dark"].clone();
        let syntax = syntax_set.find_syntax_by_extension("rb").unwrap().clone();
        let highlighter = Highlighter::new(theme);

        HighlightSettings { syntax, syntax_set, highlighter }
    }
}

pub struct RichContent {
    highlight_settings : Rc<HighlightSettings>,
    raw_content: Rope,
    // the key corresponds to number of next line to parse by ParseState. NOT DONE, bc I don't want negative numbers.
    parse_cache: RefCell<Vec<(usize, ParseState)>>, //TODO is it possible to remove RefCell?

    // If prefix is None, we need to parse rope from beginning. If it's Some(r, l) then
    // the previous RichContent (#r in ContentProvider history) has l lines in common.
    prefix : Option<(usize, usize)>,
    lines : Vec<Rc<RichLine>>,
}

impl RichContent {
    pub fn new(settings : Rc<HighlightSettings>, rope : Rope) -> Self {
        RichContent {
            highlight_settings : settings,
            raw_content : rope,
            prefix : None,
            //contract: sorted.
            parse_cache : RefCell::new(Vec::new()),
            //contract: max key of parse_cache < len(lines)
            lines : Vec::new()
        }
    }

    pub fn len_lines(&self) -> usize {
        self.raw_content.len_lines()
    }

    // result.0 > line_no
    pub fn get_cache(&self, line_no : usize) -> Option<(usize, ParseState)> {
        let parse_cache : Ref<Vec<(usize, ParseState)>> = self.parse_cache.borrow();

        if parse_cache.is_empty() {
            return None;
        }

        let cache : Option<(usize, ParseState)> = match parse_cache.binary_search_by(|pair| pair.0.cmp(&line_no)) {
            Ok(idx) => parse_cache.get(idx).map(|x| (x.0, x.1.clone())),
            Err(higher_index) => {
                if higher_index == 0 { None } else {
                    parse_cache.get(higher_index - 1).map(|x| (x.0, x.1.clone()))
                }
            }
        };

        cache
    }

    pub fn get_line(&self, line_no : usize) -> Option<&Rc<RichLine>> {
        if self.lines.len() > line_no {
            return Some(&self.lines[line_no]);
        }

        // see contracts
        let (line, mut parse_state) = match self.get_cache(line_no) {
            None => (0 as usize, ParseState::new(&self.highlight_settings.syntax)),
            Some(x) => x
        };

        let line_limit = std::cmp::min(line_no + PARSING_MILESTONE, self.raw_content.len_lines());

        for i in line..line_limit {
            let sth = parse_state.parse_line(
                &self.raw_content.line(line).to_string(),
                &self.highlight_settings.syntax_set
            );

            let iter = HighlightIterator::new(&mut highlight_state, &ops[..], line, &self.highlighter);

        }

        None //TODO
    }
}

struct RichLinesIterator<'a> {
    content : &'a RichContent,
    line_no : usize
}

impl <'a> Iterator for RichLinesIterator<'a> {
    type Item = &'a Rc<RichLine>;

    fn next(&mut self) -> Option<Self::Item> {
        let old_line_no = self.line_no;
        self.line_no += 1;
        let line  = self.content.get_line(old_line_no);
        line
    }
}

impl <'a> ExactSizeIterator for RichLinesIterator<'a> {
    fn len(&self) -> usize {
        self.content.len_lines()
    }
}

impl <'a> Index<usize> for RichLinesIterator<'a> {
    type Output = Rc<RichLine>;

    //panics //TODO format docs.
    fn index(&self, idx : usize) -> &Rc<RichLine> {
        self.content.get_line(idx).unwrap()
    }

}