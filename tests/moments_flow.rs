//! 说说（moments）集成测试：
//! 创建 + 附件关联 + 按天分组、前台 /moments 宫格渲染、删除级联。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{test_app, test_config, PNG_1x1};
use hancic::services::{moments, uploads};
use tower::ServiceExt;

/// 创建说说（两条附件）→ 关联按数组序落库；把第二条 created_at 改为昨天
/// 后按站点时区（Asia/Shanghai）分组应得 2 组。
#[tokio::test]
async fn create_and_group_by_day() {
    let cfg = test_config("moments-create");
    let (_app, pool) = test_app("moments-create").await;
    let uploads_dir = cfg.data_dir.join("uploads");
    let a1 = uploads::save_bytes(&pool, &cfg, &uploads_dir, "a.png", "image/png", PNG_1x1)
        .await
        .unwrap();
    let a2 = uploads::save_bytes(&pool, &cfg, &uploads_dir, "b.png", "image/png", PNG_1x1)
        .await
        .unwrap();

    let m1 = moments::create_moment(&pool, "今天天气不错", &[a1.id, a2.id])
        .await
        .unwrap();
    let m2 = moments::create_moment(&pool, "第二条说说", &[])
        .await
        .unwrap();

    // 附件按数组序关联，sort_order 0/1
    let atts = moments::list_moment_attachments(&pool, m1.id).await.unwrap();
    assert_eq!(atts.len(), 2);
    assert_eq!(atts[0].0.id, a1.id);
    assert_eq!(atts[0].1, 0);
    assert_eq!(atts[1].0.id, a2.id);
    assert_eq!(atts[1].1, 1);

    // 无附件说说：关联列表为空
    assert!(moments::list_moment_attachments(&pool, m2.id)
        .await
        .unwrap()
        .is_empty());

    // 手工把第二条 created_at 改为 25 小时前（Shanghai 时区下必为昨天）
    let yesterday = m1.created_at - chrono::Duration::hours(25);
    sqlx::query("UPDATE moments SET created_at = ? WHERE id = ?")
        .bind(yesterday.to_rfc3339())
        .bind(m2.id)
        .execute(&pool)
        .await
        .unwrap();

    let (items, total) = moments::list_moments(&pool, None, false, None, 1, 20).await.unwrap();
    assert_eq!(total, 2);
    let groups = moments::group_by_day(&pool, items).await.unwrap();
    assert_eq!(groups.len(), 2, "两条不同日期的说说应分为 2 组");

    // 日期键按站点时区（默认 Asia/Shanghai）换算本地日期
    let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
    let d1 = m1.created_at.with_timezone(&tz).format("%Y-%m-%d").to_string();
    let d2 = yesterday.with_timezone(&tz).format("%Y-%m-%d").to_string();
    assert_eq!(groups[0].0, d1);
    assert_eq!(groups[1].0, d2);
    assert_ne!(d1, d2, "两条说说的上海本地日期应不同");
    assert_eq!(groups[0].1.len() + groups[1].1.len(), 2);
}

/// 前台 GET /moments → 200，HTML 含说说内容与宫格 class。
#[tokio::test]
async fn moments_page_shows_grid() {
    let cfg = test_config("moments-page");
    let (app, pool) = test_app("moments-page").await;
    let uploads_dir = cfg.data_dir.join("uploads");
    let a1 = uploads::save_bytes(&pool, &cfg, &uploads_dir, "a.png", "image/png", PNG_1x1)
        .await
        .unwrap();
    moments::create_moment(&pool, "今天天气不错", &[a1.id])
        .await
        .unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .uri("/moments")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(html.contains("今天天气不错"), "页面应渲染说说内容");
    assert!(html.contains("moment-grid"), "页面应含宫格 class");
    assert!(html.contains("/uploads/image/"), "页面应引用附件地址");
}

/// delete_moment 后 moment_attachments 级联删除（关联列表为空），
/// attachments 记录本身保留。
#[tokio::test]
async fn delete_moment_cascades() {
    let cfg = test_config("moments-delete");
    let (_app, pool) = test_app("moments-delete").await;
    let uploads_dir = cfg.data_dir.join("uploads");
    let a1 = uploads::save_bytes(&pool, &cfg, &uploads_dir, "a.png", "image/png", PNG_1x1)
        .await
        .unwrap();
    let m = moments::create_moment(&pool, "待删除", &[a1.id]).await.unwrap();

    moments::delete_moment(&pool, m.id).await.unwrap();

    assert!(
        moments::list_moment_attachments(&pool, m.id)
            .await
            .unwrap()
            .is_empty(),
        "删除说说后关联应级联清空"
    );
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM attachments WHERE id = ?")
        .bind(a1.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "attachments 记录应保留");

    // 删除不存在的说说 → NotFound
    let err = moments::delete_moment(&pool, m.id).await.unwrap_err();
    assert!(matches!(err, hancic::error::AppError::NotFound(_)));
}
