use serde::Deserialize;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;

use crate::{AppState, handlers_auth::SessionUser};

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// owner-check generico: Some(id) se la riga esiste e appartiene all'utente
async fn owns(pool: &sqlx::SqlitePool, user_id: &str, table: &str, id: &str) -> bool {
    let q = format!("SELECT 1 FROM {table} WHERE id = ? AND user_id = ? LIMIT 1");
    let r: Option<i64> = sqlx::query_scalar(&q)
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .unwrap();
    r.is_some()
}

#[derive(Deserialize)]
pub struct ListReq {
    pub folder_id: Option<String>,
}

#[derive(Deserialize)]
pub struct FolderReq {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct FolderPatch {
    pub name: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct FilePatch {
    pub name: Option<String>,
    pub folder_id: Option<String>,
}

pub async fn list(
    State(s): State<AppState>,
    user: SessionUser,
    Query(q): Query<ListReq>,
) -> Response {
    // cartella richiesta: owner check prima di qualsiasi lettura (404 se non tuo)
    if let Some(fid) = &q.folder_id
        && !owns(&s.pool, &user.user_id, "folders", fid).await
    {
        return err(StatusCode::NOT_FOUND, "cartella sconosciuta");
    }
    let folders: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, name FROM folders WHERE user_id = ? AND parent_id IS ? ORDER BY name",
    )
    .bind(&user.user_id)
    .bind(q.folder_id.as_deref())
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let files: Vec<(String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, name, size, mime, created_at FROM files
         WHERE user_id = ? AND folder_id IS ? AND deleted_at IS NULL ORDER BY name",
    )
    .bind(&user.user_id)
    .bind(q.folder_id.as_deref())
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size), 0) FROM files WHERE user_id = ? AND deleted_at IS NULL",
    )
    .bind(&user.user_id)
    .fetch_one(&s.pool)
    .await
    .unwrap();
    let free = fs4::available_space(&*s.data_dir).unwrap_or(u64::MAX);
    let folders: Vec<serde_json::Value> = folders
        .into_iter()
        .map(|(id, name)| serde_json::json!({"id": id, "name": name}))
        .collect();
    let files: Vec<serde_json::Value> = files
        .into_iter()
        .map(|(id, name, size, mime, created)| {
            serde_json::json!({
                "id": id, "name": name, "size": size, "mime": mime, "created_at": created
            })
        })
        .collect();
    Json(serde_json::json!({
        "folders": folders,
        "files": files,
        "quota": {"used": used, "free": free},
    }))
    .into_response()
}

pub async fn create_folder(
    State(s): State<AppState>,
    user: SessionUser,
    Json(rb): Json<FolderReq>,
) -> Response {
    if let Some(pid) = &rb.parent_id
        && !owns(&s.pool, &user.user_id, "folders", pid).await
    {
        return err(StatusCode::NOT_FOUND, "cartella padre sconosciuta");
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO folders (id, user_id, parent_id, name, created_at) VALUES (?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&user.user_id)
    .bind(rb.parent_id.as_deref())
    .bind(&rb.name)
    .bind(now())
    .execute(&s.pool)
    .await
    {
        if e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
        {
            return err(StatusCode::CONFLICT, "nome già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    Json(serde_json::json!({"id": id})).into_response()
}

pub async fn patch_folder(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
    Json(pb): Json<FolderPatch>,
) -> Response {
    if !owns(&s.pool, &user.user_id, "folders", &id).await {
        return err(StatusCode::NOT_FOUND, "cartella sconosciuta");
    }
    if let Some(pid) = &pb.parent_id {
        if !owns(&s.pool, &user.user_id, "folders", pid).await {
            return err(StatusCode::NOT_FOUND, "cartella padre sconosciuta");
        }
        // anti-ciclo: risale i parent del nuovo padre; se tocca la cartella stessa → 400
        let mut cur = pid.clone();
        loop {
            if cur == id {
                return err(StatusCode::BAD_REQUEST, "spostamento a catena vietato");
            }
            let nxt: Option<Option<String>> =
                sqlx::query_scalar("SELECT parent_id FROM folders WHERE id = ?")
                    .bind(&cur)
                    .fetch_optional(&s.pool)
                    .await
                    .unwrap();
            match nxt {
                Some(Some(c)) => cur = c,
                _ => break,
            }
        }
    }
    if let Err(e) = sqlx::query(
        "UPDATE folders SET name = COALESCE(?, name), parent_id = COALESCE(?, parent_id)
         WHERE id = ? AND user_id = ?",
    )
    .bind(pb.name.as_deref())
    .bind(pb.parent_id.as_deref())
    .bind(&id)
    .bind(&user.user_id)
    .execute(&s.pool)
    .await
    {
        if e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
        {
            return err(StatusCode::CONFLICT, "nome già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn delete_folder(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Response {
    if !owns(&s.pool, &user.user_id, "folders", &id).await {
        return err(StatusCode::NOT_FOUND, "cartella sconosciuta");
    }
    let folders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM folders WHERE parent_id = ?")
        .bind(&id)
        .fetch_one(&s.pool)
        .await
        .unwrap();
    let files: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE folder_id = ? AND deleted_at IS NULL")
            .bind(&id)
            .fetch_one(&s.pool)
            .await
            .unwrap();
    if folders > 0 || files > 0 {
        return err(StatusCode::CONFLICT, "cartella non vuota");
    }
    sqlx::query("DELETE FROM folders WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&s.pool)
        .await
        .unwrap();
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn patch_file(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
    Json(pb): Json<FilePatch>,
) -> Response {
    if !owns(&s.pool, &user.user_id, "files", &id).await {
        return err(StatusCode::NOT_FOUND, "file sconosciuto");
    }
    if let Some(fid) = &pb.folder_id
        && !owns(&s.pool, &user.user_id, "folders", fid).await
    {
        return err(
            StatusCode::NOT_FOUND,
            "cartella di destinazione sconosciuta",
        );
    }
    if let Err(e) = sqlx::query(
        "UPDATE files SET name = COALESCE(?, name), folder_id = COALESCE(?, folder_id)
         WHERE id = ? AND user_id = ?",
    )
    .bind(pb.name.as_deref())
    .bind(pb.folder_id.as_deref())
    .bind(&id)
    .bind(&user.user_id)
    .execute(&s.pool)
    .await
    {
        if e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
        {
            return err(StatusCode::CONFLICT, "nome già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn delete_file(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Response {
    let Some((uid, stored)): Option<(String, String)> = sqlx::query_as(
        "SELECT user_id, stored_name FROM files WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(&user.user_id)
    .fetch_optional(&s.pool)
    .await
    .unwrap()
    else {
        return err(StatusCode::NOT_FOUND, "file sconosciuto");
    };
    sqlx::query("DELETE FROM files WHERE id = ?")
        .bind(&id)
        .execute(&s.pool)
        .await
        .unwrap();
    // hard delete del blob: se manca già non è un errore
    let _ = tokio::fs::remove_file(format!("{}/storage/{}/{}", s.data_dir, uid, stored)).await;
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn download(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT name, mime, stored_name FROM files
         WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(&user.user_id)
    .fetch_optional(&s.pool)
    .await
    .unwrap();
    let Some((name, mime, stored)) = row else {
        return err(StatusCode::NOT_FOUND, "file non trovato");
    };
    // ServeFile gestisce Range/416/accept-ranges sui byte a disco; qui sopra solo
    // l'owner-check: il path è derivato da stored_name di proprietà confermata
    let svc = tower_http::services::ServeFile::new(format!(
        "{}/storage/{}/{}",
        s.data_dir, user.user_id, stored
    ));
    let mut resp = svc.oneshot(req).await.unwrap();
    // ascii: fallback sicuro per browser senza RFC 5987; pct: RFC 9110 token bytes
    let ascii_name: String = name
        .chars()
        .map(|c| if c.is_ascii() && c != '"' { c } else { '_' })
        .collect();
    let pct: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let cd = format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{pct}");
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&cd).unwrap(),
    );
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static("default-src 'none'"),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap());
    resp
}
