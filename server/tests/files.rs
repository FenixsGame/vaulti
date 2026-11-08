mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn list_files_and_folders_and_quota() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    // cartella Prova
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "POST",
            "/api/folders",
            Some(serde_json::json!({"name": "Prova"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let fol: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    let fol_id = fol["id"].as_str().unwrap().to_string();
    // file dentro "Prova"
    let id = start_upload(&t, &cookie, "doc.txt", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": fol_id})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    // list root → 1 subfolder pieno + 0 file
    let res = t
        .app
        .clone()
        .oneshot(authed_req("GET", "/api/files", None, &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["folders"].as_array().unwrap().len(), 1);
    assert_eq!(j["files"].as_array().unwrap().len(), 0);
    assert_eq!(j["quota"]["used"], 2);
    // list solo in "Prova" → 1 file con nome
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "GET",
            &format!("/api/files?folder_id={fol_id}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["files"][0]["name"], "doc.txt");
    // rename file
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"name": "rin.doc.txt"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let nm: String = sqlx::query_scalar("SELECT name FROM files WHERE id = ?")
        .bind(&fid)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(nm, "rin.doc.txt");
}

#[tokio::test]
async fn delete_hard_removes_row_and_blob() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let (uid, stored): (String, String) =
        sqlx::query_as("SELECT f.user_id, f.stored_name FROM files f WHERE f.id = ?")
            .bind(&fid)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    let blob = std::fs::read(blob_path(&t, &uid, &stored)).unwrap();
    assert_eq!(blob, b"ab");
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/files/{fid}"),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(
        !t.data_dir
            .path()
            .join("storage")
            .join(&uid)
            .join(&stored)
            .exists()
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn folder_delete_empty_only_cycles_and_dups() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let f1 = post_folder(&t, &cookie, "c1", None).await;
    assert_eq!(f1.1, 200);
    // duplicato nella stessa cartella → 409
    assert_eq!(post_folder(&t, &cookie, "c1", None).await.1, 409);
    // stesso nome in un'altra cartella è legale ("c1" dentro f2)
    let f2 = post_folder(&t, &cookie, "c2", None).await;
    let sub = post_folder(&t, &cookie, "c1", Some(&f2.0)).await;
    assert_eq!(sub.1, 200);
    // subfolder dentro f1: così DELETE f1 → 409 (non vuota)
    let sub1 = post_folder(&t, &cookie, "c1b", Some(&f1.0)).await;
    assert_eq!(sub1.1, 200);
    // f3 dentro f2 → 200; poi f2 dentro f3 (suo discendente) → 400; self → 400
    let f3 = post_folder(&t, &cookie, "c3", None).await;
    let ok = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f3.0),
            Some(serde_json::json!({"parent_id": f2.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f2.0),
            Some(serde_json::json!({"parent_id": f3.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/folders/{}", f2.0),
            Some(serde_json::json!({"parent_id": f2.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    // DELETE f1 → 409 (c1b dentro); f2 → 409 (f3 dentro); f3 → 409 (f2 parent... )
    // attenzione: f3 contiene il sub di f2? NO: f3 è IN f2, vuota? f3 vuota → 200
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f1.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f2.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let resp = t
        .app
        .clone()
        .oneshot(authed_req(
            "DELETE",
            &format!("/api/folders/{}", f3.0),
            None,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // parent inesistente → 404
    let resp = t
        .app
        .oneshot(authed_req(
            "POST",
            "/api/folders",
            Some(serde_json::json!({"name": "x", "parent_id": "no-such-folder"})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn move_file_to_root_with_empty_folder_id() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fol = post_folder(&t, &cookie, "c", None).await;
    let fid = make_file(&t, &cookie).await;
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": fol.0})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = t
        .app
        .clone()
        .oneshot(authed_req(
            "PATCH",
            &format!("/api/files/{fid}"),
            Some(serde_json::json!({"folder_id": ""})),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let f: Option<Option<String>> = sqlx::query_scalar("SELECT folder_id FROM files WHERE id = ?")
        .bind(&fid)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert!(f.is_none());
}
