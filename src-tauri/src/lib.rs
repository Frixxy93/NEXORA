//! NEXORA desktop application entrypoint (library form).
//!
//! `run()` builds the Tauri app: resolves the per-user data directory, opens the
//! database there, starts the localhost Bridge API (for the Maya plug-in), and
//! wires up IPC commands.

mod commands;
mod state;

use nexora_core::bridge::{self, BridgeContext, MayaLink};
use nexora_core::db::Database;
use nexora_core::settings::AppSettings;
use state::{AppState, Db};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Build and run the NEXORA desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    // Auto-update stack (desktop only). The updater checks GitHub Releases and
    // applies signed bundles; the process plugin provides relaunch-after-install.
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    builder
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("resolve app data dir");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("nexora.db");

            // Thumbnails live in a rebuildable cache under app data (spec §50).
            let thumbnail_dir = data_dir.join("cache").join("thumbnails");
            std::fs::create_dir_all(&thumbnail_dir)?;

            // Open the DB behind Arc<Mutex> so the bridge + commands share it.
            let db: Db = Arc::new(Mutex::new(
                Database::open(&db_path).map_err(|e| format!("open database: {e}"))?,
            ));

            // Bridge shared state.
            let outbox = Arc::new(Mutex::new(Vec::new()));
            let maya = Arc::new(Mutex::new(MayaLink::default()));
            let token = bridge::new_token();

            // Emit to the UI when Maya captures/imports through the bridge.
            let app_handle = app.handle().clone();
            let on_change = Arc::new(move || {
                let _ = app_handle.emit("library:changed", ());
            });

            let port = bridge::start(
                BridgeContext {
                    db: db.clone(),
                    thumbnail_dir: thumbnail_dir.clone(),
                    token: token.clone(),
                    outbox: outbox.clone(),
                    maya: maya.clone(),
                    on_change,
                },
                bridge::DEFAULT_PORT,
            )
            .map_err(|e| format!("start bridge: {e}"))?;

            // Publish the port + token so the Maya plug-in can auto-connect.
            write_bridge_config(app, port, &token);

            let scan_running = Arc::new(AtomicBool::new(false));

            // Auto-scan timer: wakes each minute and, when auto-scan is enabled,
            // scans the library at the configured interval to pick up files added
            // outside NEXORA. Quiet unless it finds something new.
            {
                let app_handle = app.handle().clone();
                let db_timer = db.clone();
                let thumb_timer = thumbnail_dir.clone();
                let scan_flag = scan_running.clone();
                std::thread::spawn(move || {
                    let mut elapsed_min: u32 = 0;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(60));
                        let settings = {
                            let guard = db_timer.lock().unwrap_or_else(|p| p.into_inner());
                            match AppSettings::load(guard.conn()) {
                                Ok(s) => s,
                                Err(_) => continue,
                            }
                        };
                        if !settings.library.auto_scan {
                            elapsed_min = 0;
                            continue;
                        }
                        elapsed_min += 1;
                        if elapsed_min < settings.library.scan_frequency_minutes.max(1) {
                            continue;
                        }
                        elapsed_min = 0;
                        if scan_flag.swap(true, Ordering::SeqCst) {
                            continue; // a scan (manual or prior tick) is already running
                        }
                        commands::scan_once(&app_handle, &db_timer, thumb_timer.clone(), false);
                        scan_flag.store(false, Ordering::SeqCst);
                    }
                });
            }

            app.manage(AppState {
                db,
                thumbnail_dir,
                outbox,
                maya,
                bridge_token: token,
                bridge_port: port,
                discover_stop: Arc::new(AtomicBool::new(false)),
                discover_running: Arc::new(AtomicBool::new(false)),
                discover_progress: Arc::new(Mutex::new(Default::default())),
                scan_running,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::core_version,
            commands::get_app_settings,
            commands::save_app_settings,
            commands::init_library,
            commands::get_library_status,
            commands::get_library_stats,
            commands::get_library_health,
            commands::get_maya_status,
            commands::import_paths,
            commands::list_textures,
            commands::get_texture,
            commands::get_thumbnail,
            commands::list_texture_sets,
            commands::get_texture_set,
            commands::rebuild_texture_sets,
            commands::get_udim_info,
            commands::import_material,
            commands::create_material_from_set,
            commands::list_materials,
            commands::get_material,
            commands::search,
            commands::set_favorite,
            commands::list_favorites,
            commands::list_recent_added,
            commands::list_recent_used,
            commands::record_usage,
            commands::list_tags,
            commands::tags_for_asset,
            commands::add_tag,
            commands::remove_tag,
            commands::create_collection,
            commands::list_collections,
            commands::delete_collection,
            commands::add_to_collection,
            commands::remove_from_collection,
            commands::collection_members,
            commands::list_duplicates,
            commands::remove_asset,
            commands::send_to_maya,
            commands::get_bridge_info,
            commands::install_maya_plugin,
            commands::start_discover_sync,
            commands::stop_discover_sync,
            commands::get_discover_status,
            commands::discover_browse,
            commands::start_discover_download,
            commands::recompute_metadata,
            commands::rename_asset,
            commands::set_asset_category,
            commands::set_texture_map_type,
            commands::set_favorite_many,
            commands::add_tag_many,
            commands::add_to_collection_many,
            commands::remove_assets,
            commands::scan_library,
            commands::list_missing_files,
            commands::relink_texture,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NEXORA");
}

/// Write `~/.nexora/bridge.json` so the Maya plug-in can discover the port+token.
fn write_bridge_config(app: &tauri::App, port: u16, token: &str) {
    if let Ok(home) = app.path().home_dir() {
        let dir = home.join(".nexora");
        if std::fs::create_dir_all(&dir).is_ok() {
            let cfg = serde_json::json!({ "port": port, "token": token, "host": "127.0.0.1" });
            let _ = std::fs::write(dir.join("bridge.json"), cfg.to_string());
        }
    }
}
