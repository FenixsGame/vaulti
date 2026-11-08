use serde::Deserialize;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::{AppState, auth, handlers_auth::SessionUser};

const WEEK: i64 = 7 * 24 * 3600;

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn uploads_row(
    pool: &sqlx::SqlitePool,
    upload_id: &str,
    user_id: &str,
) -> Option<(i64, i64, i64, String)> {
    let r: Option<(i64, i64, i64, String)> = sqlx::query_as(
        "SELECT chunk_size, total_chunks, size, name FROM uploads WHERE id = ? AND user_id = ?",
    )
    .bind(upload_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap();
    r
}

#[derive(Deserialize)]
pub struct StartReq {
    pub name: String,
    pub size: u64,
}

pub async fn start(
    State(s): State<AppState>,
    user: SessionUser,
    Json(rb): Json<StartReq>,
) -> Response {
    // sweep: tmp oltre 7 giorni → directory e righe cancellate
    let old: Vec<String> = sqlx::query_scalar("SELECT id FROM uploads WHERE created_at < ?")
        .bind(now() - WEEK)
        .fetch_all(&s.pool)
        .await
        .unwrap();
    for id in &old {
        let _ = tokio::fs::remove_dir_all(s.upload_dir(id)).await;
        sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?")
            .bind(id)
            .execute(&s.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM uploads WHERE id = ?")
            .bind(id)
            .execute(&s.pool)
            .await
            .unwrap();
    }
    // check 507: spazio libero sul disco dati
    let free = fs4::available_space(&*s.data_dir).unwrap_or(u64::MAX);
    if rb.size > free {
        return err(StatusCode::INSUFFICIENT_STORAGE, "spazio insufficiente");
    }
    let id = uuid::Uuid::new_v4().to_string();
    // ceil intero: size 0 → total 0 (nessun chunk atteso, finish accetta somma 0)
    let total: i64 = rb.size.div_ceil(s.chunk_size) as i64;
    let updir = s.upload_dir(&id);
    tokio::fs::create_dir_all(&updir).await.unwrap();
    let f = tokio::fs::File::create(updir.join("data")).await.unwrap();
    f.set_len(rb.size).await.unwrap();
    sqlx::query(
        "INSERT INTO uploads (id,user_id,name,chunk_size,total_chunks,size,created_at)
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&user.user_id)
    .bind(&rb.name)
    .bind(s.chunk_size as i64)
    .bind(total)
    .bind(rb.size as i64)
    .bind(now())
    .execute(&s.pool)
    .await
    .unwrap();
    Json(serde_json::json!({"upload_id": id, "chunk_size": s.chunk_size})).into_response()
}

pub async fn chunk(
    State(s): State<AppState>,
    user: SessionUser,
    Path((id, n)): Path<(String, i32)>,
    body: axum::body::Body,
) -> Response {
    // l'id è un UUID generato dal server: non è mai un percorso
    if uuid::Uuid::parse_str(&id).is_err() {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    }
    // owner verificato PRIMA di toccare il disco
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await else {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    };
    if n < 0 || n as i64 >= up.1 {
        return err(StatusCode::BAD_REQUEST, "chunk fuori intervallo");
    }
    // atteso: chunk_size; l'ultimo è size - (total-1)*chunk_size
    let expected: i64 = if n as i64 + 1 == up.1 {
        up.2 - (up.1 - 1) * up.0
    } else {
        up.0
    };
    let mut f = match tokio::fs::OpenOptions::new()
        .write(true)
        .open(s.upload_dir(&id).join("data"))
        .await
    {
        Ok(f) => f,
        Err(_) => return err(StatusCode::NOT_FOUND, "upload scomparso"),
    };
    f.seek(std::io::SeekFrom::Start(n as u64 * up.0 as u64))
        .await
        .unwrap();
    let mut stream = body.into_data_stream();
    let mut written: i64 = 0;
    let mut overflow = false;
    while let Some(piece) = stream.next().await {
        let p = match piece {
            Ok(p) => p,
            Err(_) => return err(StatusCode::BAD_REQUEST, "body illeggibile"),
        };
        if written + p.len() as i64 > expected {
            overflow = true;
            break;
        }
        f.write_all(&p).await.unwrap();
        written += p.len() as i64;
    }
    if overflow {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "chunk troppo lungo");
    }
    if written != expected {
        return err(StatusCode::BAD_REQUEST, "chunk incompleto o corto");
    }
    f.flush().await.unwrap();
    f.sync_all().await.unwrap();
    // la riga va inserita SOLO con la verifica esatta: un chunk interrotto
    // non risulta mai ricevuto e il resume lo rinvia (idempotente: OR REPLACE)
    sqlx::query(
        "INSERT OR REPLACE INTO upload_chunks (upload_id, chunk_n, len, received_at)
         VALUES (?,?,?,?)",
    )
    .bind(&id)
    .bind(n)
    .bind(written)
    .bind(now())
    .execute(&s.pool)
    .await
    .unwrap();
    StatusCode::NO_CONTENT.into_response()
}

pub async fn status(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Response {
    if uploads_row(&s.pool, &id, &user.user_id).await.is_none() {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    }
    let received: Vec<i32> = sqlx::query_scalar(
        "SELECT chunk_n FROM upload_chunks WHERE upload_id = ? ORDER BY chunk_n",
    )
    .bind(&id)
    .fetch_all(&s.pool)
    .await
    .unwrap();
    Json(serde_json::json!({"received": received})).into_response()
}

pub async fn finish(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Response {
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await else {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    };
    let got: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks WHERE upload_id = ?")
        .bind(&id)
        .fetch_one(&s.pool)
        .await
        .unwrap();
    let sum: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(len), 0) FROM upload_chunks WHERE upload_id = ?")
            .bind(&id)
            .fetch_one(&s.pool)
            .await
            .unwrap();
    if got != up.1 || sum != up.2 {
        return err(StatusCode::CONFLICT, "upload incompleto");
    }
    let src = s.upload_dir(&id).join("data");
    let dst_dir = s.storage_dir(&user.user_id);
    tokio::fs::create_dir_all(&dst_dir).await.unwrap();
    let stored = uuid::Uuid::new_v4().to_string();
    let meta_len = match tokio::fs::metadata(&src).await {
        Ok(m) => m.len(),
        Err(_) => return err(StatusCode::CONFLICT, "tmp mancante"),
    };
    if meta_len != up.2 as u64 {
        return err(StatusCode::CONFLICT, "size finale disallineata");
    }
    // sync_all prima del rename: i dati sono a bordo
    if up.2 > 0 {
        let f = tokio::fs::File::open(&src).await.unwrap();
        f.sync_all().await.unwrap();
    }
    // INSERT PRIMA del rename: il UNIQUE(user_id, folder_key, name) decide qui
    // il 409 e nessun blob resta in storage/ senza riga
    let mime = auth::mime_of(&up.3);
    let fid = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO files (id,user_id,folder_id,name,size,mime,stored_name,created_at)
         VALUES (?,?,NULL,?,?,?,?,?)",
    )
    .bind(&fid)
    .bind(&user.user_id)
    .bind(&up.3)
    .bind(up.2)
    .bind(mime)
    .bind(&stored)
    .bind(now())
    .execute(&s.pool)
    .await
    {
        if e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
        {
            // nome già in uso nella stessa cartella: upload morto, pulizia completa
            sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?")
                .bind(&id)
                .execute(&s.pool)
                .await
                .ok();
            sqlx::query("DELETE FROM uploads WHERE id = ?")
                .bind(&id)
                .execute(&s.pool)
                .await
                .ok();
            let _ = tokio::fs::remove_dir_all(s.upload_dir(&id)).await;
            return err(StatusCode::CONFLICT, "nome già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    // il rename adesso: se fallisce, la riga appena inserta non deve restare
    if tokio::fs::rename(&src, s.blob_path(&user.user_id, &stored))
        .await
        .is_err()
    {
        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(&fid)
            .execute(&s.pool)
            .await
            .ok();
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?")
        .bind(&id)
        .execute(&s.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM uploads WHERE id = ?")
        .bind(&id)
        .execute(&s.pool)
        .await
        .unwrap();
    tokio::fs::remove_dir(s.upload_dir(&id)).await.unwrap();
    Json(serde_json::json!({"id": fid})).into_response()
}

pub async fn cancel(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Response {
    if uploads_row(&s.pool, &id, &user.user_id).await.is_none() {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    }
    sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?")
        .bind(&id)
        .execute(&s.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM uploads WHERE id = ?")
        .bind(&id)
        .execute(&s.pool)
        .await
        .unwrap();
    let _ = tokio::fs::remove_dir_all(s.upload_dir(&id)).await;
    StatusCode::OK.into_response()
}
