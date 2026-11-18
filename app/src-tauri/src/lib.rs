//! 應用的組裝：註冊指令與外掛，建立狀態。資料規則都在核心（`paperreader-core`），這裡只轉接。

mod commands;
mod error;
mod ext;
mod interchange;
mod location;
mod ondevice;
mod pdfstrip;
mod state;
mod translate;
mod transport;
mod update;

use state::{AppState, Settings};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 平台轉接器（電腦上連同它用的官方對話框外掛）
    tauri_plugin_paperreader::register(tauri::Builder::default())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let settings_path = data_dir.join("settings.json");
            // 設定檔壞了也要能啟動：原檔留著、用預設值，畫面告知一次（`Settings::load_or_create`）
            let (settings, recovered) = Settings::load_or_create(&settings_path)
                .map_err(|e| format!("settings: {:?}", e.kind))?;
            app.manage(AppState::new(settings, settings_path, recovered));
            app.manage(translate::JobSlot::<translate::JobStatus>::default());
            app.manage(commands::digest::DigestSlot::default());
            app.manage(std::sync::Arc::new(ondevice::Bridge::default()));
            app.manage(std::sync::Arc::new(ondevice::Downloads::default()));
            app.manage(std::sync::Arc::new(commands::layout::LayoutState::default()));
            app.manage(std::sync::Arc::new(commands::sync::SyncProgress::default()));
            app.manage(std::sync::Arc::new(commands::import::ImportSlot::default()));
            app.manage(std::sync::Arc::new(update::UpdateSlot::default()));
            ext::setup(app);
            // 上次被中斷留下的選檔暫存
            if let Ok(cache) = app.path().app_cache_dir() {
                commands::add::clear_pick_cache(&cache);
                commands::export::clear_export_cache(&cache);
                commands::update::clear_stale(&cache);
            }
            Ok(())
        })
        // 應用自己的指令清單；外掛的指令由 `ext::dispatch` 依名稱接上
        .invoke_handler(ext::dispatch(tauri::generate_handler![
            commands::library::library_status,
            commands::library::library_create,
            commands::library::library_switch_to_shared,
            commands::library::library_move_plan,
            commands::library::library_move_to_shared,
            commands::library::library_move_progress,
            commands::library::library_move_cancel,
            commands::library::library_rescan,
            commands::library::library_check,
            commands::library::open_all_files_settings,
            commands::library::per_paper_state_get,
            commands::library::per_paper_state_put,
            commands::references::references_write,
            commands::references::link_judgment_add,
            commands::references::link_judgments_remove,
            commands::text::text_write,
            commands::text::text_states,
            commands::papers::list_papers,
            commands::papers::continue_reading,
            commands::papers::get_paper,
            commands::papers::update_bib,
            commands::papers::remove_paper,
            commands::papers::restore_paper,
            commands::papers::list_trash,
            commands::papers::remove_material,
            commands::papers::list_removed_materials,
            commands::papers::purge_trash_entry,
            commands::papers::restore_material,
            commands::papers::empty_trash,
            commands::papers::set_material_pages,
            commands::papers::fill_bib_blanks,
            commands::add::add_papers_from_picker,
            commands::add::shared_pending,
            commands::add::add_shared_papers,
            commands::add::add_translation_from_picker,
            commands::groups::list_groups,
            commands::groups::create_group,
            commands::groups::rename_group,
            commands::groups::delete_group,
            commands::groups::set_group_member,
            commands::notes::read_notes,
            commands::notes::add_highlight,
            commands::notes::set_highlight_comment,
            commands::notes::delete_highlight,
            commands::notes::set_thoughts,
            commands::notes::open_notes_external,
            commands::ink::read_ink,
            commands::ink::add_stroke,
            commands::ink::delete_stroke,
            commands::ink::clear_ink_page,
            commands::backup::export_backup_file,
            commands::backup::restore_backup_from_picker,
            commands::backup::library_join_from_picker,
            commands::sync::sync_export,
            commands::sync::sync_import_prepare,
            commands::sync::sync_import_apply,
            commands::sync::sync_import_discard,
            commands::sync::sync_resume,
            commands::sync::sync_resumed_seen,
            commands::sync::sync_progress,
            commands::sync::this_device,
            commands::import::scan_import,
            commands::import::prepare_import,
            commands::import::import_step,
            commands::import::finish_import,
            commands::import::import_progress,
            commands::import::import_cancel,
            commands::import::material_open_external,
            commands::export::export_original,
            commands::export::export_notes,
            commands::export::export_translation,
            commands::export::export_bibliography_file,
            commands::export::export_citations,
            commands::export::export_put_bytes,
            commands::export::deliver_export,
            commands::translate::translate_settings_get,
            commands::translate::translate_settings_patch,
            commands::translate::translate_service_add,
            commands::translate::translate_service_save,
            commands::translate::translate_service_reset,
            commands::translate::translate_service_delete,
            commands::translate::translate_compare,
            commands::translate::overlay_apply,
            commands::translate::glossary_get,
            commands::translate::glossary_set,
            commands::translate::glossary_import,
            commands::translate::glossary_tsv,
            commands::translate::overlay_interchange,
            commands::translate::translate_key_set,
            commands::translate::translate_key_clear,
            commands::translate::translate_test,
            commands::translate::overlay_create,
            commands::translate::overlay_read,
            commands::translate::translate_run,
            commands::translate::translate_status,
            commands::translate::translate_cancel,
            commands::translate::translate_engines,
            commands::translate::translate_prompt_save,
            commands::translate::translate_prompt_delete,
            commands::translate::prompt_preview,
            commands::translate::prompt_check,
            commands::translate::overlay_terms_set,
            commands::translate::usage_paper,
            commands::ondevice::ondevice_status,
            commands::ondevice::ondevice_download,
            commands::ondevice::ondevice_download_cancel,
            commands::ondevice::ondevice_delete,
            commands::ondevice::ondevice_next,
            commands::ondevice::ondevice_reply,
            commands::ondevice::bergamot_file,
            commands::layout::layout_status,
            commands::layout::layout_download,
            commands::layout::layout_download_cancel,
            commands::layout::layout_delete,
            commands::layout::layout_detect,
            commands::generated::system_cjk_font,
            commands::generated::translation_pdf_put,
            commands::generated::pdf_strip_text,
            commands::thumbs::thumb_get,
            commands::thumbs::thumb_put,
            commands::app::take_link,
            commands::reading::record_open,
            commands::reading::record_position,
            commands::reading::set_read,
            commands::reading::reading_progress,
            commands::reading::reading_history,
            commands::reading::material_size,
            commands::reading::read_material_range,
            commands::app::app_info,
            commands::app::open_url,
            commands::update::update_status,
            commands::update::update_configure,
            commands::update::update_check,
            commands::update::update_download,
            commands::update::update_download_cancel,
            commands::update::update_install,
            commands::update::update_install_status,
            commands::update::update_open_install_settings,
            commands::update::update_discard,
            commands::app::startup_notice,
            commands::app::get_theme,
            commands::app::set_theme,
            commands::app::get_prefs,
            commands::app::set_prefs,
            commands::app::set_keep_awake,
            commands::app::system_insets,
            commands::app::system_bars,
            commands::digest::digest_run,
            commands::digest::digest_continue,
            commands::digest::digest_status,
            commands::digest::digest_cancel,
            commands::digest::digest_read,
            commands::digest::digest_delete_point,
            commands::digest::digest_set_proofread,
            commands::digest::export_digest,
            commands::digest::digest_open_external,
        ]))
        .run(tauri::generate_context!())
        .expect("error while running the application");
}
