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

use app_state::*;
use buffer_state::BufferState;
use buffer_state_observer::BufferStateObserver;
use cursive;
use cursive::theme;
use cursive::theme::BaseColor::*;
use cursive::theme::Color;
use cursive::theme::PaletteColor;
use cursive::theme::{BorderStyle, Palette, Theme};
use cursive::traits::*;
use cursive::views::*;
use cursive::*;
use settings;
use settings::load_default_settings;
use settings::Settings;

use events::IEvent;
use file_dialog::{self, *};
use fuzzy_query_view::FuzzyQueryView;
use sly_text_view::SlyTextView;
use std::thread;
use utils;

use events::IChannel;
use lsp_client::LspClient;
use sly_view::SlyView;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::mpsc;
use std::sync::Arc;
use view_handle::ViewHandle;

pub struct Interface {
    state :         AppState,
    settings :      Rc<Settings>,
    channel :       (mpsc::Sender<IEvent>, mpsc::Receiver<IEvent>),
    siv :           Cursive,
    done :          bool,
    file_dialog :   Option<ViewHandle>,
    active_editor : ViewHandle,
    lsp_clients :   Vec<LspClient>, //TODO(njskalski): temporary storage to avoid removal
}

impl Interface {
    fn get_active_editor(&mut self) -> views::ViewRef<SlyTextView> {
        let id = format!("sly{}", self.active_editor.view_id());
        let editor = self.siv.find_id(&id).unwrap() as views::ViewRef<SlyTextView>;
        editor
    }

    pub fn new(mut state : AppState) -> Self {
        let mut siv = Cursive::default();
        let settings = Rc::new(load_default_settings());

        let palette = settings.get_palette();

        let theme : Theme =
            Theme { shadow : false, borders : BorderStyle::Simple, palette : palette };

        let channel = mpsc::channel();
        siv.set_theme(theme);

        let buffer_observer = state.get_first_buffer().unwrap(); // TODO(njskalski): panics. Semantics unclear.
        let sly_text_view = SlyTextView::new(settings.clone(), buffer_observer, channel.0.clone());
        let active_editor = sly_text_view.handle().clone();

        siv.add_fullscreen_layer(sly_text_view.with_id(sly_text_view.siv_uid()));

        let mut i = Interface {
            state :         state,
            settings :      settings,
            channel :       channel,
            siv :           siv,
            done :          false,
            file_dialog :   None,
            active_editor : active_editor,
            lsp_clients :   Vec::new(),
        };

        // let known_actions = vec!["show_everything_bar"];
        //TODO filter unknown actions
        for (event, action) in i.settings.get_keybindings("global") {
            let ch = i.get_event_sink();
            match action.as_str() {
                "show_file_bar" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::ShowFileBar).unwrap();
                    });
                }
                "quit" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::QuitSly).unwrap();
                    });
                }
                "show_buffer_list" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::ShowBufferList).unwrap();
                    });
                }
                "save" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::SaveCurrentBuffer).unwrap();
                    });
                }
                //                "save_as" => {
                //                    i.siv.add_global_callback(event, move |_| {
                //                        ch.send(IEvent::ShowSaveAs).unwrap();
                //                    });
                //                }
                "open_file_dialog" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::OpenFileDialog).unwrap();
                    });
                }
                "close_window" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::CloseWindow).unwrap();
                    });
                }
                "start_lsp" => {
                    i.siv.add_global_callback(event, move |_| {
                        ch.send(IEvent::EnableLSP).unwrap();
                    });
                }
                _ => {
                    debug!("unknown action {:?} bound with event global {:?}", action, event);
                }
            }
        }

        i
    }

    fn process_events(&mut self) {
        while let Ok(msg) = self.channel.1.try_recv() {
            debug!("processing event {:?}", msg);
            match msg {
                IEvent::ShowFileBar => {
                    self.show_file_bar();
                }
                IEvent::FuzzyQueryBarSelected(marker, selection) => {
                    debug!("selected {:?}", &selection);
                    self.close_file_bar();
                }
                IEvent::QuitSly => {
                    self.done = true;
                }
                IEvent::CloseWindow => {
                    self.close_floating_windows();
                }
                IEvent::BufferEditEvent(view_handle, events) => {
                    //TODO now I just send to active editor, ignoring view_handle
                    self.get_active_editor().buffer_obs().submit_edit_events_to_buffer(events);
                }
                IEvent::SaveCurrentBuffer => {
                    self.save_current_buffer();
                }
                IEvent::OpenFileDialog => {
                    self.show_open_file_dialog();
                }
                IEvent::OpenFile(file_path) => {
                    self.state.schedule_file_for_load(file_path);
                    self.close_filedialog();
                }
                IEvent::ShowBufferList => {
                    self.show_buffer_list();
                }
                IEvent::EnableLSP => {
                    self.enable_lsp();
                }
                _ => {
                    debug!("unhandled IEvent {:?}", &msg);
                }
            }
        }
    }

    /// Main program method
    pub fn main(&mut self) {
        while !self.done {
            self.process_events();
            self.siv.step();
        }
    }

    pub fn close_floating_windows(&mut self) {
        self.close_file_bar();
        self.close_filedialog();
        self.close_buffer_list();
    }

    pub fn get_event_sink(&self) -> IChannel {
        self.channel.0.clone()
    }

    // TODO(njskalski) this assertion is temporary, in use only because the interface is built
    // agile, not pre-designed.
    fn assert_no_file_view(&mut self) {
        assert!(self.siv.find_id::<FileDialog>(file_dialog::FILE_VIEW_ID).is_none());
    }

    fn show_save_as(&mut self) {
        if self.file_dialog.is_some() {
            debug!("show_save_as: not showing file_dialog, because it's already opened.");
            return;
        }

        let id = self.get_active_editor().buffer_obs().buffer_id();
        let path_op = self.get_active_editor().buffer_obs().get_path();

        let (folder_op, file_op) = match path_op {
            None => (None, None),
            Some(path) => utils::path_string_to_pair(path.to_string_lossy().to_string()), /* TODO get rid of
                                                                                           * path_string_to_pair */
        };
        self.show_file_dialog(FileDialogVariant::SaveAsFile(id, folder_op, file_op));
    }

    fn show_open_file_dialog(&mut self) {
        if self.file_dialog.is_some() {
            debug!("show_open_file_dialog: not showing file_dialog, because it's already opened.");
            return;
        }

        self.show_file_dialog(FileDialogVariant::OpenFile(None));
    }

    fn show_file_dialog(&mut self, variant : FileDialogVariant) {
        if self.file_dialog.is_some() {
            debug!("show_file_dialog: not showing file_dialog, because it's already opened.");
            return;
        }

        let is_save = variant.is_save();
        let file_view = FileDialog::new(
            self.get_event_sink(),
            variant,
            self.state.get_dir_tree(),
            &self.settings,
        );
        self.siv.add_layer(IdView::new("filedialog", file_view));
    }

    fn close_filedialog(&mut self) {
        if self.siv.focus_id("filedialog").is_ok() {
            self.siv.pop_layer();
            self.filedialog_visible = false;
        }
    }

    fn close_file_bar(&mut self) {
        if self.siv.focus_id("filebar").is_ok() {
            self.siv.pop_layer();
            self.file_bar_visible = false;
        }
    }

    fn show_file_bar(&mut self) {
        if !self.file_bar_visible {
            let ebar = FuzzyQueryView::new(
                self.state.get_file_index(),
                "filebar".to_string(),
                self.get_event_sink(),
                self.settings.clone(),
            );
            self.siv.add_layer(IdView::new("filebar", ebar));
            self.file_bar_visible = true;
        }
    }

    fn show_buffer_list(&mut self) {
        if !self.bufferlist_visible {
            let buffer_list = self.state.get_buffers();
            warn!("buffer list not imlemented yet.");
            self.bufferlist_visible = true;
        }
    }

    fn close_buffer_list(&mut self) {
        if self.siv.focus_id("bufferlist").is_ok() {
            self.siv.pop_layer();
            self.bufferlist_visible = false;
        }
    }

    fn enable_lsp(&mut self) {
        let lsp = LspClient::new(
            OsStr::new("rls"),
            self.get_event_sink(),
            Some(self.state.directories()),
        );
        self.lsp_clients.push(lsp.unwrap());
    }

    fn save_current_buffer(&mut self) {
        let path = self.get_active_editor().buffer_obs().get_path();
        if path.is_none() {
            self.show_save_as();
        } else {
            let editor = self.get_active_editor();
            let mut buffer = editor.buffer_obs().borrow_state();
            let buffer_id = buffer.id();
            self.get_event_sink().send(IEvent::SaveBufferAs(buffer_id, path.unwrap()));
        }
    }
}
