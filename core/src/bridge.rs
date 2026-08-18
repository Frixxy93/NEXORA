//! The NEXORA Bridge API (spec §40/§41).
//!
//! A small localhost HTTP server that Maya's plug-in talks to. Per the spec's
//! communication model, Maya is the client and NEXORA Core is the authority —
//! the plug-in NEVER touches SQLite directly, it calls these endpoints. The
//! server binds `127.0.0.1` only and requires an auth token on every route
//! except `/api/status` (used for discovery).
//!
//! Endpoints:
//! - `GET  /api/status`            — liveness + counts (no token)
//! - `GET  /api/materials`         — list materials
//! - `GET  /api/materials/:id`     — one material
//! - `GET  /api/textures`          — list textures
//! - `GET  /api/textures/:id`      — one texture
//! - `GET  /api/search?q=`         — search
//! - `GET  /api/pull`              — drain the "send to Maya" outbox
//! - `POST /api/heartbeat`         — Maya reports it's alive (`{version}`)
//! - `POST /api/texture/import`    — `{path}` → import a texture
//! - `POST /api/material/capture`  — `{name, maps:[{slot,path}]}` → build a material
//! - `POST /api/scene/scan`        — stub (returns ok)
//! - `POST /api/path/repair`       — stub (returns ok)

use crate::db::Database;
use crate::maptypes::MapTypeRegistry;
use crate::material;
use crate::settings::{AppSettings, StorageMode};
use crate::texture::{self, ImportOptions};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Response, Server};

/// Default port the bridge tries first (falls back to an OS-assigned port).
pub const DEFAULT_PORT: u16 = 48757;

/// An asset queued by the desktop "Send to Maya" action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendItem {
    /// "material" or "texture".
    pub kind: String,
    pub id: String,
}

/// The shared outbox of pending sends.
pub type Outbox = Arc<Mutex<Vec<SendItem>>>;

/// Liveness state for the connected Maya (updated by heartbeats).
#[derive(Default)]
pub struct MayaLink {
    pub last_seen: Option<Instant>,
    pub version: Option<String>,
}

impl MayaLink {
    /// Considered connected if a heartbeat arrived within the last 12s.
    pub fn connected(&self) -> bool {
        self.last_seen
            .map(|t| t.elapsed().as_secs() < 12)
            .unwrap_or(false)
    }
}

/// Everything the request handler needs.
pub struct BridgeContext {
    pub db: Arc<Mutex<Database>>,
    pub thumbnail_dir: PathBuf,
    pub token: String,
    pub outbox: Outbox,
    pub maya: Arc<Mutex<MayaLink>>,
    /// Called after a capture/import so the desktop UI can refresh.
    pub on_change: Arc<dyn Fn() + Send + Sync>,
}

/// Generate a fresh 32-hex-char bridge token.
pub fn new_token() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

/// Bind the bridge (trying `desired_port`, else an OS-assigned one) and spawn the
/// serving thread. Returns the actual bound port.
pub fn start(ctx: BridgeContext, desired_port: u16) -> std::io::Result<u16> {
    let server = Server::http(("127.0.0.1", desired_port))
        .or_else(|_| Server::http(("127.0.0.1", 0)))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(desired_port);

    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            handle(request, &ctx);
        }
    });
    Ok(port)
}

fn json_response(value: serde_json::Value, code: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = value.to_string();
    let header =
        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("header");
    Response::from_string(body)
        .with_header(header)
        .with_status_code(code)
}

fn handle(mut request: tiny_http::Request, ctx: &BridgeContext) {
    // path without query string
    let raw_url = request.url().to_string();
    let (path, query) = match raw_url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (raw_url.clone(), String::new()),
    };
    let method = request.method().clone();

    // Auth: everything except /api/status needs the token.
    if path != "/api/status" {
        let ok = request
            .headers()
            .iter()
            .any(|h| h.field.as_str().as_str().eq_ignore_ascii_case("X-NEXORA-Token") && h.value.as_str() == ctx.token);
        if !ok {
            let _ = request.respond(json_response(json!({"error": "unauthorized"}), 401));
            return;
        }
    }

    // Read a JSON body for POSTs.
    let read_body = |req: &mut tiny_http::Request| -> serde_json::Value {
        let mut s = String::new();
        let _ = req.as_reader().read_to_string(&mut s);
        serde_json::from_str(&s).unwrap_or(json!({}))
    };

    let response = match (&method, path.as_str()) {
        (Method::Get, "/api/status") => route_status(ctx),
        (Method::Get, "/api/materials") => route_materials(ctx),
        (Method::Get, "/api/textures") => route_textures(ctx),
        (Method::Get, "/api/search") => route_search(ctx, &query),
        (Method::Get, "/api/pull") => route_pull(ctx),
        (Method::Get, p) if p.starts_with("/api/materials/") => {
            route_material(ctx, &p["/api/materials/".len()..])
        }
        (Method::Get, p) if p.starts_with("/api/textures/") => {
            route_texture(ctx, &p["/api/textures/".len()..])
        }
        (Method::Post, "/api/heartbeat") => {
            let body = read_body(&mut request);
            route_heartbeat(ctx, &body)
        }
        (Method::Post, "/api/texture/import") => {
            let body = read_body(&mut request);
            route_texture_import(ctx, &body)
        }
        (Method::Post, "/api/material/capture") => {
            let body = read_body(&mut request);
            route_material_capture(ctx, &body)
        }
        (Method::Post, "/api/scene/scan") | (Method::Post, "/api/path/repair") => {
            json_response(json!({"ok": true, "note": "not yet implemented"}), 200)
        }
        _ => json_response(json!({"error": "not found"}), 404),
    };
    let _ = request.respond(response);
}

// --- route handlers --------------------------------------------------------

fn import_opts(conn: &rusqlite::Connection, thumbnail_dir: &PathBuf) -> ImportOptions {
    let settings = AppSettings::load(conn).unwrap_or_default();
    ImportOptions {
        managed: settings.library.storage_mode == StorageMode::Managed,
        library_root: settings.library.location.clone().map(PathBuf::from),
        thumbnail_dir: thumbnail_dir.clone(),
        generate_preview: settings.import.auto_generate_preview,
        detect_maps: settings.import.auto_detect_maps,
    }
}

fn route_status(ctx: &BridgeContext) -> Response<std::io::Cursor<Vec<u8>>> {
    let db = ctx.db.lock().unwrap();
    let conn = db.conn();
    let materials: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets WHERE kind='material'", [], |r| r.get(0))
        .unwrap_or(0);
    let textures: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets WHERE kind='texture'", [], |r| r.get(0))
        .unwrap_or(0);
    // The user's chosen default renderer, so Maya can target it (spec §37/§51).
    let default_renderer = AppSettings::load(conn)
        .ok()
        .and_then(|s| serde_json::to_value(s.default_renderer).ok())
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "generic_pbr".to_string());
    json_response(
        json!({
            "app": "NEXORA",
            "version": crate::CORE_VERSION,
            "ok": true,
            "materials": materials,
            "textures": textures,
            "default_renderer": default_renderer,
        }),
        200,
    )
}

fn route_materials(ctx: &BridgeContext) -> Response<std::io::Cursor<Vec<u8>>> {
    let db = ctx.db.lock().unwrap();
    match material::list_materials(db.conn(), None) {
        Ok(list) => json_response(json!(list), 200),
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_material(ctx: &BridgeContext, id: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let db = ctx.db.lock().unwrap();
    match material::get_material(db.conn(), id) {
        Ok(Some(m)) => json_response(json!(m), 200),
        Ok(None) => json_response(json!({"error": "not found"}), 404),
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_textures(ctx: &BridgeContext) -> Response<std::io::Cursor<Vec<u8>>> {
    let db = ctx.db.lock().unwrap();
    match texture::list_textures(db.conn(), None) {
        Ok(list) => json_response(json!(list), 200),
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_texture(ctx: &BridgeContext, id: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let db = ctx.db.lock().unwrap();
    match texture::get_texture(db.conn(), id) {
        Ok(Some(t)) => json_response(json!(t), 200),
        Ok(None) => json_response(json!({"error": "not found"}), 404),
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_search(ctx: &BridgeContext, query: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let q = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("q="))
        .map(|v| v.replace('+', " "))
        .unwrap_or_default();
    let db = ctx.db.lock().unwrap();
    match crate::library::search(db.conn(), &q) {
        Ok(r) => json_response(json!(r), 200),
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_pull(ctx: &BridgeContext) -> Response<std::io::Cursor<Vec<u8>>> {
    let items: Vec<SendItem> = {
        let mut out = ctx.outbox.lock().unwrap();
        std::mem::take(&mut *out)
    };
    let db = ctx.db.lock().unwrap();
    let conn = db.conn();
    let resolved: Vec<serde_json::Value> = items
        .into_iter()
        .map(|it| {
            if it.kind == "material" {
                let m = material::get_material(conn, &it.id).ok().flatten();
                json!({"kind": "material", "id": it.id, "material": m})
            } else {
                let t = texture::get_texture(conn, &it.id).ok().flatten();
                json!({"kind": "texture", "id": it.id, "texture": t})
            }
        })
        .collect();
    json_response(json!(resolved), 200)
}

fn route_heartbeat(ctx: &BridgeContext, body: &serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let version = body.get("version").and_then(|v| v.as_str()).map(String::from);
    {
        let mut link = ctx.maya.lock().unwrap();
        link.last_seen = Some(Instant::now());
        link.version = version;
    }
    json_response(json!({"ok": true}), 200)
}

fn route_texture_import(ctx: &BridgeContext, body: &serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json_response(json!({"error": "missing path"}), 400),
    };
    let db = ctx.db.lock().unwrap();
    let conn = db.conn();
    let opts = import_opts(conn, &ctx.thumbnail_dir);
    let registry = MapTypeRegistry::builtin();
    let result = texture::analyze(std::path::Path::new(&path), &registry)
        .and_then(|a| texture::import_texture(conn, &a, &opts));
    match result {
        Ok(outcome) => {
            let _ = texture::rebuild_texture_sets(conn);
            drop(db);
            (ctx.on_change)();
            json_response(json!({"ok": true, "outcome": outcome}), 200)
        }
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

fn route_material_capture(ctx: &BridgeContext, body: &serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Captured Material")
        .to_string();
    let maps: Vec<(String, String)> = body
        .get("maps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let slot = m.get("slot")?.as_str()?.to_string();
                    let path = m.get("path")?.as_str()?.to_string();
                    Some((slot, path))
                })
                .collect()
        })
        .unwrap_or_default();

    if maps.is_empty() {
        return json_response(json!({"error": "no maps provided"}), 400);
    }

    let db = ctx.db.lock().unwrap();
    let conn = db.conn();
    let opts = import_opts(conn, &ctx.thumbnail_dir);
    let registry = MapTypeRegistry::builtin();
    match material::create_material_from_maps(conn, &name, &maps, &opts, &registry) {
        Ok(id) => {
            let _ = texture::rebuild_texture_sets(conn);
            drop(db);
            (ctx.on_change)();
            json_response(json!({"ok": true, "id": id}), 200)
        }
        Err(e) => json_response(json!({"error": e.to_string()}), 500),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use image::{Rgb, RgbImage};
    use std::path::Path;

    fn ctx_with_db() -> (BridgeContext, Arc<Mutex<Database>>, Outbox, Arc<Mutex<MayaLink>>) {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let outbox: Outbox = Arc::new(Mutex::new(Vec::new()));
        let maya = Arc::new(Mutex::new(MayaLink::default()));
        let ctx = BridgeContext {
            db: db.clone(),
            thumbnail_dir: std::env::temp_dir().join("nexora_bridge_test_thumbs"),
            token: "testtoken".into(),
            outbox: outbox.clone(),
            maya: maya.clone(),
            on_change: Arc::new(|| {}),
        };
        (ctx, db, outbox, maya)
    }

    fn write_png(dir: &Path, name: &str) -> String {
        std::fs::create_dir_all(dir).unwrap();
        let mut img = RgbImage::new(16, 16);
        for px in img.pixels_mut() {
            *px = Rgb([90, 90, 90]);
        }
        let p = dir.join(name);
        img.save(&p).unwrap();
        p.to_string_lossy().to_string()
    }

    #[test]
    fn status_and_auth() {
        let (ctx, _db, _o, _m) = ctx_with_db();
        let port = start(ctx, 0).unwrap();
        let base = format!("http://127.0.0.1:{port}");

        // status needs no token
        let body: serde_json::Value =
            ureq::get(&format!("{base}/api/status")).call().unwrap().into_json().unwrap();
        assert_eq!(body["app"], "NEXORA");
        // Renderer preference is exposed so Maya can target it (spec §37).
        assert_eq!(body["default_renderer"], "generic_pbr");

        // materials requires token
        let unauth = ureq::get(&format!("{base}/api/materials")).call();
        assert!(unauth.is_err()); // 401

        let ok = ureq::get(&format!("{base}/api/materials"))
            .set("X-NEXORA-Token", "testtoken")
            .call()
            .unwrap();
        assert_eq!(ok.status(), 200);
    }

    #[test]
    fn import_capture_and_pull() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, db, outbox, _m) = ctx_with_db();
        let port = start(ctx, 0).unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let tok = "testtoken";

        // Capture a material from explicit slot→path maps.
        let bc = write_png(dir.path(), "cap_basecolor.png");
        let nrm = write_png(dir.path(), "cap_normal.png");
        let resp: serde_json::Value = ureq::post(&format!("{base}/api/material/capture"))
            .set("X-NEXORA-Token", tok)
            .send_json(json!({
                "name": "Captured Concrete",
                "maps": [{"slot": "base_color", "path": bc}, {"slot": "normal", "path": nrm}]
            }))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(resp["ok"], true);
        let mat_id = resp["id"].as_str().unwrap().to_string();

        // It shows up via the API.
        let mats: serde_json::Value = ureq::get(&format!("{base}/api/materials"))
            .set("X-NEXORA-Token", tok)
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(mats.as_array().unwrap().len(), 1);

        // Queue a send and pull it (resolved with data).
        outbox.lock().unwrap().push(SendItem { kind: "material".into(), id: mat_id.clone() });
        let pulled: serde_json::Value = ureq::get(&format!("{base}/api/pull"))
            .set("X-NEXORA-Token", tok)
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        let arr = pulled.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["kind"], "material");
        assert_eq!(arr[0]["material"]["id"], mat_id);
        // Outbox drained.
        assert!(outbox.lock().unwrap().is_empty());

        // The captured material really references two textures in the DB.
        let db = db.lock().unwrap();
        assert_eq!(texture::list_textures(db.conn(), None).unwrap().len(), 2);
    }

    #[test]
    fn heartbeat_marks_connected() {
        let (ctx, _db, _o, maya) = ctx_with_db();
        let port = start(ctx, 0).unwrap();
        let base = format!("http://127.0.0.1:{port}");
        assert!(!maya.lock().unwrap().connected());
        ureq::post(&format!("{base}/api/heartbeat"))
            .set("X-NEXORA-Token", "testtoken")
            .send_json(json!({"version": "2026"}))
            .unwrap();
        let link = maya.lock().unwrap();
        assert!(link.connected());
        assert_eq!(link.version.as_deref(), Some("2026"));
    }
}
