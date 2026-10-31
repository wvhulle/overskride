use std::path::PathBuf;

use relm4::{adw, adw::prelude::*, gtk, gtk::gio, prelude::*};

use crate::{
    background_application_lifecycle::{
        gsettings_stored_preferences::{AppPreferences, RUN_IN_BACKGROUND_KEY, START_AT_LOGIN_KEY},
        login_autostart_entry::managed_by_systemd,
    },
    icon_names,
    shared_page_widget_templates::{ClampedPage, PageHeader},
};

pub struct AppSettingsPage {
    preferences: AppPreferences,
    received_files_folder: PathBuf,
    folder_changed: bool,
}

#[derive(Debug)]
pub enum AppSettingsInput {
    ChooseReceivedFilesFolder,
    ReceivedFilesFolderChosen(PathBuf),
}

#[relm4::component(pub)]
impl SimpleComponent for AppSettingsPage {
    type Init = AppPreferences;
    type Input = AppSettingsInput;
    type Output = ();

    view! {
        #[template]
        ClampedPage {
            #[template_child]
            page_content {
                #[template]
                PageHeader {
                    #[template_child]
                    header_icon {
                        set_icon_name: Some("emblem-system-symbolic"),
                    },
                    #[template_child]
                    header_title {
                        set_label: "Settings",
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Received Files",

                    set_description: Some("Whether a device may send files without asking is set on its own page."),

                    adw::ActionRow {
                        set_title: "Location",
                        set_subtitle_selectable: true,
                        #[track(model.folder_changed)]
                        set_subtitle: &model.received_files_folder.to_string_lossy(),

                        add_suffix = &gtk::Button {
                            set_icon_name: icon_names::shipped::FOLDER,
                            set_valign: gtk::Align::Center,
                            set_tooltip_text: Some("Choose Folder"),
                            add_css_class: "flat",
                            connect_clicked => AppSettingsInput::ChooseReceivedFilesFolder,
                        },
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Background",
                    set_description: Some("Pairing requests and incoming files need Overskride to be running."),

                    #[name = "run_in_background_row"]
                    adw::SwitchRow {
                        set_title: "Keep Running When Closed",
                        set_subtitle: if managed_by_systemd() {
                            "Always on: started by the systemd user service"
                        } else {
                            "Closing the window hides it. Quit from the menu, the tray icon or with Ctrl+Q."
                        },
                    },

                    #[name = "start_at_login_row"]
                    adw::SwitchRow {
                        set_title: "Start at Login",
                        set_subtitle: if managed_by_systemd() {
                            "Managed by the systemd user service"
                        } else {
                            "Starts hidden in the background"
                        },
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Device List",

                    #[name = "show_unnamed_row"]
                    adw::SwitchRow {
                        set_title: "Show Unnamed Devices",
                        set_subtitle: "Also list devices that don't broadcast a name. They are rarely ready to pair.",
                    },
                },
            },
        }
    }

    fn init(
        preferences: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            received_files_folder: preferences.received_files_folder(),
            preferences,
            folder_changed: true,
        };
        let widgets = view_output!();

        let settings = model.preferences.settings();
        settings
            .bind("show-unnamed-devices", &widgets.show_unnamed_row, "active")
            .build();
        if managed_by_systemd() {
            widgets.run_in_background_row.set_active(true);
            widgets.run_in_background_row.set_sensitive(false);
            widgets.start_at_login_row.set_sensitive(false);
        } else {
            settings
                .bind(
                    RUN_IN_BACKGROUND_KEY,
                    &widgets.run_in_background_row,
                    "active",
                )
                .build();
            settings
                .bind(START_AT_LOGIN_KEY, &widgets.start_at_login_row, "active")
                .build();
            settings
                .bind(
                    RUN_IN_BACKGROUND_KEY,
                    &widgets.start_at_login_row,
                    "sensitive",
                )
                .get()
                .build();
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>) {
        self.folder_changed = false;
        match input {
            AppSettingsInput::ChooseReceivedFilesFolder => {
                let folder_dialog = gtk::FileDialog::builder()
                    .title("Choose Where Received Files Go")
                    .modal(true)
                    .initial_folder(&gio::File::for_path(&self.received_files_folder))
                    .build();
                let input_sender = sender.input_sender().clone();
                relm4::spawn_local(async move {
                    let parent_window = relm4::main_application().active_window();
                    let Ok(chosen_folder) = folder_dialog
                        .select_folder_future(parent_window.as_ref())
                        .await
                    else {
                        return;
                    };
                    if let Some(folder_path) = chosen_folder.path() {
                        input_sender.emit(AppSettingsInput::ReceivedFilesFolderChosen(folder_path));
                    }
                });
            }
            AppSettingsInput::ReceivedFilesFolderChosen(folder_path) => {
                self.preferences.set_received_files_folder(&folder_path);
                self.received_files_folder = folder_path;
                self.folder_changed = true;
            }
        }
    }
}
