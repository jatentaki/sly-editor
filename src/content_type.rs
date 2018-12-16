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
use std::ops::{Index, IndexMut};
use std::iter::{Iterator, ExactSizeIterator};
use content_provider::RopeBasedContentProvider;
use std::cell::Ref;

use cursive::theme::Color;

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

//TODO(njskalski): obviously optimise
#[derive(Debug)]
pub struct RichContent {
    rope : Rope,
//    lines : Vec<RichLine>
}

impl RichContent {
    pub fn new(rope : Rope) -> Self {
        RichContent { rope }
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn get_line(&self, line_no : usize) -> Option<&RichLine> {
        None //TODO
    }
}

struct RichLinesIterator<'a> {
    content : &'a RichContent,
    line_no : usize
}

impl <'a> Iterator for RichLinesIterator<'a> {
    type Item = &'a RichLine;

    fn next(&mut self) -> Option<Self::Item> {
        let old_line_no = self.line_no;
        self.line_no += 1;
        let line  = self.content.get_line(old_line_no);
        line
    }
}

impl <'a : 'b, 'b> ExactSizeIterator for RichLinesIterator<'a> {
    fn len(&self) -> usize {
        self.content.len_lines()
    }
}

impl <'a : 'b, 'b> Index<usize> for RichLinesIterator<'a> {
    type Output = RichLine;

    //panics //TODO format docs.
    fn index<'c>(&'c self, idx : usize) -> &'c RichLine {
        self.content.get_line(idx).unwrap()
    }

}