// 發行版在 Windows 上不開主控台視窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    paperreader_app_lib::run()
}
