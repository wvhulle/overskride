/* main.rs
 *
 * Copyright 2023 kaii
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

mod application;
mod window;
#[path = "bluetooth/message.rs"] mod message;
#[path = "bluetooth/bluetooth_settings.rs"] mod bluetooth_settings;
#[path = "bluetooth/device.rs"] mod device;
#[path = "bluetooth/agent.rs"] mod agent;
#[path = "bluetooth/services.rs"] mod services;
#[path = "bluetooth/audio_profiles.rs"] mod audio_profiles;
#[path = "bluetooth/battery.rs"] mod battery;
#[path = "bluetooth/pairing_stages.rs"] mod pairing_stages;
#[path = "bluetooth/radios.rs"] mod radios;
#[path = "bluetooth/input_devices.rs"] mod input_devices;
#[path = "obex/obex.rs"] mod obex;
#[path = "obex/obex_utils.rs"] mod obex_utils;
#[path = "widgets/connected_switch_row.rs"] mod connected_switch_row;
#[path = "bluetooth/bluetooth_device.rs"] mod bluetooth_device;
#[path = "widgets/receiving_popover.rs"] mod receiving_popover;
#[path = "widgets/receiving_row.rs"] mod receiving_row;
#[path = "widgets/startup_error_message.rs"] mod startup_error_message;
#[path = "widgets/selectable_row.rs"] mod selectable_row;
#[path = "widgets/battery_indicator.rs"] mod battery_indicator;
#[path = "widgets/pairing_timeline.rs"] mod pairing_timeline;
mod Singletons;

use self::application::OverskrideApplication;
use self::window::OverskrideWindow;

use gtk::{gio, glib};
use gtk::prelude::*;
use gtk::gdk::Display;

fn main() -> glib::ExitCode {
    gio::resources_register_include!("overskride.gresource")
        .expect("cannot register the embedded overskride.gresource");

    // Create a new GtkApplication. The application manages our main loop,
    // application windows, integration with the window manager/compositor, and
    // desktop features such as file opening and single-instance applications.
    let app = OverskrideApplication::new("io.github.kaii_lb.Overskride", &gio::ApplicationFlags::empty());

	app.connect_startup(|_| {
		load_css()
	});

    // Run the application. This function will block until the application
    // exits. Upon return, we have our exit code to return to the shell. This
    // is the code you see when you do `echo $?` after running a command in a
    // terminal.
    app.run()
}

fn load_css() {
	let provider = gtk::CssProvider::new();
	provider.load_from_resource("/io/github/kaii_lb/Overskride/gtk/style.css");

	gtk::style_context_add_provider_for_display(
		&Display::default().expect("could not connect to a display"),
		&provider,
		gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
	);
}
