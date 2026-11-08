use serde::Deserialize;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::error::AppError;
use crate::{AppState, auth, handlers_auth::SessionUser};

const WEEK: i64 = 7 * 24 * 3600;

#[derive(sqlx::FromRow)]
struct Upload {
    chunk_size: i64,
    total_chunks: i64,
    size: i64,
    name: String,
}

impl Upload {
    // atteso: chunk_size; l'ultimo è size - (total-1)*chunk_size
    fn expected_len(&self, n: i64) -> i64 {
        if n + 1 == self.total_chunks {
            self.size - (self.total_chunks - 1) * self.chunk_size
        } else {
            self.chunk_size
        }
    }
}

async fn uploads_row(
    pool: &sqlx::SqlitePool,
    upload_id: &str,
    user_id: &str,
) -> Result<Option<Upload>, AppError> {
    Ok(sqlx::query_as::<_, Upload>(
        "SELECT chunk_size, total_chunks, size, name FROM uploads WHERE id = ? AND user_id = ?",
    )
    .bind(upload_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

// pulizia best-effort di un upload morto: se le righe restano, lo sweep a 7 giorni
// ripulisce comunque; tmp rimosso a prescindere
async fn discard_upload(s: &AppState, id: &str) {
    let _ = sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?")
        .bind(id)
        .execute(&s.pool)
        .await;
    let _ = sqlx::query("DELETE FROM uploads WHERE id = ?")
        .bind(id)
        .execute(&s.pool)
        .await;
    let _ = tokio::fs::remove_dir_all(s.upload_dir(id)).await;
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
) -> Result<Response, AppError> {
    // sweep: tmp oltre 7 giorni → directory e righe cancellate
    let old: Vec<String> = sqlx::query_scalar("SELECT id FROM uploads WHERE created_at < ?")
        .bind(auth::now() - WEEK)
        .fetch_all(&s.pool)
        .await?;
    for id in &old {
        discard_upload(&s, id).await;
    }
    // check 507: spazio libero sul disco dati
    let free = fs4::available_space(&*s.data_dir).unwrap_or(u64::MAX);
    if rb.size > free {
        return Err(AppError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "spazio insufficiente",
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    // ceil intero: size 0 → total 0 (nessun chunk atteso, finish accetta somma 0)
    let total: i64 = rb.size.div_ceil(s.chunk_size) as i64;
    let updir = s.upload_dir(&id);
    tokio::fs::create_dir_all(&updir).await?;
    let f = tokio::fs::File::create(updir.join("data")).await?;
    f.set_len(rb.size).await?;
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
    .bind(auth::now())
    .execute(&s.pool)
    .await?;
    Ok(Json(serde_json::json!({"upload_id": id, "chunk_size": s.chunk_size})).into_response())
}

pub async fn chunk(
    State(s): State<AppState>,
    user: SessionUser,
    Path((id, n)): Path<(String, i32)>,
    body: axum::body::Body,
) -> Result<Response, AppError> {
    // l'id è un UUID generato dal server: non è mai un percorso
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(AppError::new(StatusCode::NOT_FOUND, "upload sconosciuto"));
    }
    // owner verificato PRIMA di toccare il disco
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await? else {
        return Err(AppError::new(StatusCode::NOT_FOUND, "upload sconosciuto"));
    };
    if n < 0 || n as i64 >= up.total_chunks {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "chunk fuori intervallo",
        ));
    }
    let expected: i64 = up.expected_len(n as i64);
    let mut f = match tokio::fs::OpenOptions::new()
        .write(true)
        .open(s.upload_dir(&id).join("data"))
        .await
    {
        Ok(f) => f,
        Err(_) => {
            return Err(AppError::new(StatusCode::NOT_FOUND, "upload scomparso"));
        }
    };
    f.seek(std::io::SeekFrom::Start(n as u64 * up.chunk_size as u64))
        .await?;
    let mut stream = body.into_data_stream();
    let mut written: i64 = 0;
    let mut overflow = false;
    while let Some(piece) = stream.next().await {
        let p = match piece {
            Ok(p) => p,
            Err(_) => {
                return Err(AppError::new(StatusCode::BAD_REQUEST, "body illeggibile"));
            }
        };
        if written + p.len() as i64 > expected {
            overflow = true;
            break;
        }
        f.write_all(&p).await?;
        written += p.len() as i64;
    }
    if overflow {
        return Err(AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "chunk troppo lungo",
        ));
    }
    if written != expected {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "chunk incompleto o corto",
        ));
    }
    f.flush().await?;
    f.sync_all().await?;
    // la riga va inserita SOLO con la verifica esatta: un chunk interrotto
    // non risulta mai ricevuto e il resume lo rinvia (idempotente: OR REPLACE)
    sqlx::query(
        "INSERT OR REPLACE INTO upload_chunks (upload_id, chunk_n, len, received_at)
         VALUES (?,?,?,?)",
    )
    .bind(&id)
    .bind(n)
    .bind(written)
    .bind(auth::now())
    .execute(&s.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn status(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    if uploads_row(&s.pool, &id, &user.user_id).await?.is_none() {
        return Err(AppError::new(StatusCode::NOT_FOUND, "upload sconosciuto"));
    }
    let received: Vec<i32> = sqlx::query_scalar(
        "SELECT chunk_n FROM upload_chunks WHERE upload_id = ? ORDER BY chunk_n",
    )
    .bind(&id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(serde_json::json!({"received": received})).into_response())
}

pub async fn finish(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await? else {
        return Err(AppError::new(StatusCode::NOT_FOUND, "upload sconosciuto"));
    };
    let got: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks WHERE upload_id = ?")
        .bind(&id)
        .fetch_one(&s.pool)
        .await?;
    let sum: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(len), 0) FROM upload_chunks WHERE upload_id = ?")
            .bind(&id)
            .fetch_one(&s.pool)
            .await?;
    if got != up.total_chunks || sum != up.size {
        return Err(AppError::new(StatusCode::CONFLICT, "upload incompleto"));
    }
    let src = s.upload_dir(&id).join("data");
    let dst_dir = s.storage_dir(&user.user_id);
    tokio::fs::create_dir_all(&dst_dir).await?;
    let stored = uuid::Uuid::new_v4().to_string();
    let meta_len = match tokio::fs::metadata(&src).await {
        Ok(m) => m.len(),
        Err(_) => {
            return Err(AppError::new(StatusCode::CONFLICT, "tmp mancante"));
        }
    };
    if meta_len != up.size as u64 {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "size finale disallineata",
        ));
    }
    // sync_all prima del rename: i dati sono a bordo
    if up.size > 0 {
        let f = tokio::fs::File::open(&src).await?;
        f.sync_all().await?;
    }
    // INSERT PRIMA del rename: il UNIQUE(user_id, folder_key, name) decide qui
    // il 409 e nessun blob resta in storage/ senza riga
    let mime = auth::mime_of(&up.name);
    let fid = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO files (id,user_id,folder_id,name,size,mime,stored_name,created_at)
         VALUES (?,?,NULL,?,?,?,?,?)",
    )
    .bind(&fid)
    .bind(&user.user_id)
    .bind(&up.name)
    .bind(up.size)
    .bind(mime)
    .bind(&stored)
    .bind(auth::now())
    .execute(&s.pool)
    .await
    {
        // nome già in uso nella stessa cartella: upload morto, pulizia completa
        if e.as_database_error()
            .is_some_and(|d| d.is_unique_violation())
        {
            discard_upload(&s, &id).await;
            return Err(AppError::new(StatusCode::CONFLICT, "nome già in uso"));
        }
        return Err(e.into());
    }
    // il rename adesso: se fallisce, la riga appena inserta non deve restare
    if tokio::fs::rename(&src, s.blob_path(&user.user_id, &stored))
        .await
        .is_err()
    {
        let _ = sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(&fid)
            .execute(&s.pool)
            .await;
        return Err(AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "errore interno",
        ));
    }
    discard_upload(&s, &id).await;
    Ok(Json(serde_json::json!({"id": fid})).into_response())
}

pub async fn cancel(
    State(s): State<AppState>,
    user: SessionUser,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    if uploads_row(&s.pool, &id, &user.user_id).await?.is_none() {
        return Err(AppError::new(StatusCode::NOT_FOUND, "upload sconosciuto"));
    }
    discard_upload(&s, &id).await;
    Ok(StatusCode::OK.into_response())
}
