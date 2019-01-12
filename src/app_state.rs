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

// this data corresponds to application state.
// a lot of it is internal, and it is not designed to be fully dumped.
// I am however putting some of obviously serializable part states in *S structs.

// Some design decisions:
// - history of file is not to be saved. This is not a versioning system.
// - if editor is closed it asks whether to save changes. If told not to, the changes are lost. there will be no
//   office-like "unsaved versions"
// - plugin states are to be lost in first versions
// - I am heading to MVP.

use ignore::gitignore;
use serde_json;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use buffer_state::BufferOpenMode;
use buffer_state::BufferState;
use buffer_state::BufferStateS;
use buffer_state_observer::BufferStateObserver;
use fuzzy_index::FuzzyIndex;
use fuzzy_index_trait::FuzzyIndexTrait;
use fuzzy_view_item::file_list_to_items;

use content_provider;
use content_provider::RopeBasedContentProvider;
use cursive;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::error;
use std::io;
use std::io::Write;
use std::rc::Rc;

use buffer_state::CreationPolicy;
use core::borrow::Borrow;
use lazy_dir_tree::LazyTreeNode;
use std::cell::Cell;
use std::path::PathBuf;
use view_handle::ViewHandle;

pub struct AppState {
    // TODO not sure if there is any reason to distinguish between the two.
    loaded_buffers :  Vec<Rc<RefCell<BufferState>>>,
    buffers_to_load : Vec<Rc<RefCell<BufferState>>>,

    index : Arc<RefCell<FuzzyIndex>>, /* because searches are mutating the cache TODO this can be
                                       * solved with "interior mutability", as other caches in this
                                       * app */
    dir_and_files_tree :     Rc<LazyTreeNode>,
    get_first_buffer_guard : Cell<bool>,
    directories :            Vec<PathBuf>, /* it's a straigthforward copy of arguments used
                                            * to guess "workspace" parameter for languageserver */
}

impl AppState {
    //    //TODO this interface is temporary.
    //    pub fn get_buffer_for_screen(&self, view_handle : &ViewHandle) -> Option<Rc<RefCell<BufferState>>> {
    //        self.loaded_buffers.get(view_handle).map(|x| x.clone())
    //    }

    /// Returns list of buffers. Rather stable.
    pub fn get_buffers(&self) -> Vec<BufferStateObserver> {
        self.loaded_buffers.iter().map(|b| BufferStateObserver::new(b.clone())).collect()
    }

    /// Returns file index. Rather stable.
    pub fn get_file_index(&self) -> Arc<RefCell<FuzzyIndexTrait>> {
        self.index.clone()
    }

    pub fn get_dir_tree(&self) -> Rc<LazyTreeNode> {
        self.dir_and_files_tree.clone()
    }

    pub fn schedule_file_for_load(&mut self, file_path : PathBuf) -> Result<(), io::Error> {
        let buffer_state = BufferState::open(file_path, CreationPolicy::Can)?;
        self.buffers_to_load.push(buffer_state);
        Ok(())
    }

    /// This method is called while constructing interface, to determine content of first edit view.
    pub fn get_first_buffer(&mut self) -> BufferStateObserver {
        if self.get_first_buffer_guard.get() {
            error!("secondary call to app_state::get_first_buffer!");
        }
        self.get_first_buffer_guard.set(true);

        self.loaded_buffers.append(&mut self.buffers_to_load);

        if self.buffers_to_load.is_empty() {
            /// if there is no buffer to load, we create an unnamed one.
            self.loaded_buffers.push(BufferState::new());
        }

        BufferStateObserver::new(self.loaded_buffers.first().unwrap().clone())
    }

    pub fn new(directories : Vec<PathBuf>, files : Vec<PathBuf>) -> Self {
        let mut buffers : Vec<Rc<RefCell<BufferState>>> = Vec::new();
        for file in &files {
            match BufferState::open(file.clone(), CreationPolicy::Must) {
                Ok(buffer_state) => buffers.push(buffer_state),
                Err(e) => error!("{}", e),
            }
        }

        let mut files_to_index : Vec<PathBuf> = files.to_owned();
        for dir in &directories {
            build_file_index(&mut files_to_index, dir, false, None);
        }

        let file_index_items = file_list_to_items(&files_to_index);

        AppState {
            buffers_to_load :        buffers,
            loaded_buffers :         Vec::new(),
            index :                  Arc::new(RefCell::new(FuzzyIndex::new(file_index_items))),
            dir_and_files_tree :     Rc::new(LazyTreeNode::new(directories.clone(), files)),
            get_first_buffer_guard : Cell::new(false),
            directories :            directories,
        }
    }

    fn empty() -> Self {
        Self::new(Vec::new(), Vec::new())
    }

    pub fn directories(&self) -> &Vec<PathBuf> {
        &self.directories
    }
}

/// this method takes into account .git and other directives set in .gitignore. However it only takes into account most
/// recent .gitignore
fn build_file_index(
    mut index : &mut Vec<PathBuf>,
    dir : &Path,
    enable_gitignore : bool,
    gi_op : Option<&gitignore::Gitignore>,
) {
    match fs::read_dir(dir) {
        Ok(read_dir) => {
            let gitignore_op : Option<gitignore::Gitignore> = if enable_gitignore {
                let pathbuf = dir.join(Path::new("/.gitignore"));
                let gitignore_path = pathbuf.as_path();
                if gitignore_path.exists() && gitignore_path.is_file() {
                    let (gi, error_op) = gitignore::Gitignore::new(&gitignore_path);
                    if let Some(error) = error_op {
                        info!("Error while parsing gitignore file {:?} : {:}", gitignore_path, error);
                    }
                    Some(gi)
                } else {
                    None
                }
            } else {
                None
            };

            for entry_res in read_dir {
                match entry_res {
                    Ok(entry) => {
                        let path_buf = entry.path();
                        let path = path_buf.as_path();

                        if enable_gitignore {
                            if path.ends_with(Path::new(".git")) {
                                return;
                            }
                        }

                        if path.is_file() {
                            if let Some(ref gitignore) = &gitignore_op {
                                if gitignore.matched(path, false).is_ignore() {
                                    continue;
                                };
                            };
                            index.push(path.to_path_buf()); //TODO(njskalski): move instead of copy.
                        } else {
                            if let Some(ref gitignore) = &gitignore_op {
                                if gitignore.matched(path, true).is_ignore() {
                                    continue;
                                };
                            };

                            let most_recent_gitignore =
                                if gitignore_op.is_some() { gitignore_op.as_ref() } else { gi_op };
                            build_file_index(&mut index, &path, enable_gitignore, most_recent_gitignore);
                        }
                    }
                    Err(e) => error!("error listing directory \"{:?}\": {:?}. Skipping.", dir, e),
                } //match
            } //for
        }
        Err(e) => warn!("unable to open dir \"{:?}\".", dir),
    }
}
