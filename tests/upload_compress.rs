//! 上传管线与图片压缩集成测试：
//! 小图上传落库落盘、拒绝白名单外类型、超限拒绝、大 JPEG 压缩缩小、GIF 原样返回。

mod common;

use common::{setup_password, start_server, PNG_1x1};
use hancic::services::uploads;
use image::GenericImageView;

/// multipart POST /api/uploads（admin 已登录 cookie）带 PNG_1x1：
/// 200、kind=image、DB 有记录、磁盘文件存在。
#[tokio::test]
async fn upload_small_png_creates_attachment() {
    let (addr, client, pool) = start_server("upload-png").await;
    assert!(setup_password(&client, &addr).await, "应能设置密码并自动登录");

    let form = reqwest::multipart::Form::new().part(
        "files",
        reqwest::multipart::Part::bytes(PNG_1x1.to_vec())
            .file_name("tiny.png")
            .mime_str("image/png")
            .unwrap(),
    );
    let res = client
        .post(format!("http://{addr}/api/uploads"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    let json: serde_json::Value = res.json().await.unwrap();
    let data = json["data"].as_array().expect("响应应含 data 数组");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["kind"], "image");
    let rel_path = data[0]["path"].as_str().expect("应含 path").to_string();
    assert!(rel_path.starts_with("image/"), "path 应以 image/ 开头: {rel_path}");

    // DB 有记录
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM attachments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // 磁盘文件存在（数据目录由 common::temp_data_dir 的命名约定决定）
    let data_dir = std::env::temp_dir().join(format!(
        "hancic-test-upload-png-{}",
        std::process::id()
    ));
    let full = data_dir.join("uploads").join(&rel_path);
    assert!(full.exists(), "磁盘文件应存在: {}", full.display());
}

/// 上传白名单外类型（application/x-msdownload / .exe）→ 400，消息含「不支持」。
#[tokio::test]
async fn upload_rejects_disallowed_type() {
    let (addr, client, _pool) = start_server("upload-reject").await;
    assert!(setup_password(&client, &addr).await, "应能设置密码并自动登录");

    let form = reqwest::multipart::Form::new().part(
        "files",
        reqwest::multipart::Part::bytes(vec![0x4d, 0x5a])
            .file_name("evil.exe")
            .mime_str("application/x-msdownload")
            .unwrap(),
    );
    let res = client
        .post(format!("http://{addr}/api/uploads"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);
    let text = res.text().await.unwrap();
    assert!(text.contains("不支持"), "错误消息应含'不支持': {text}");
}

/// 超过 upload_max_image（10MB）的假数据 → 400，消息含「大小上限」。
#[tokio::test]
async fn upload_rejects_oversize() {
    let (addr, client, _pool) = start_server("upload-oversize").await;
    assert!(setup_password(&client, &addr).await, "应能设置密码并自动登录");

    let big = vec![0u8; 10 * 1024 * 1024 + 1];
    let form = reqwest::multipart::Form::new().part(
        "files",
        reqwest::multipart::Part::bytes(big)
            .file_name("big.png")
            .mime_str("image/png")
            .unwrap(),
    );
    let res = client
        .post(format!("http://{addr}/api/uploads"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);
    let text = res.text().await.unwrap();
    assert!(text.contains("大小上限"), "错误消息应含'大小上限': {text}");
}

/// I6：构造声明 30000x30000 的小体积 PNG（仅签名 + IHDR，合法 CRC，无像素数据）
/// → 解码前尺寸检查直接拒绝 400「图片尺寸过大」，杜绝解压炸弹 OOM。
#[tokio::test]
async fn upload_rejects_oversized_dimensions() {
    let (addr, client, _pool) = start_server("upload-bomb").await;
    assert!(setup_password(&client, &addr).await, "应能设置密码并自动登录");

    let png = fake_large_png(30000, 30000);
    // 前置：声明尺寸能被读到（测试自身有效）
    let dims = image::ImageReader::new(std::io::Cursor::new(png.clone()))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .expect("IHDR 应可解析出尺寸");
    assert_eq!(dims, (30000, 30000), "测试前提：PNG 头部声明超大尺寸");

    let form = reqwest::multipart::Form::new().part(
        "files",
        reqwest::multipart::Part::bytes(png)
            .file_name("bomb.png")
            .mime_str("image/png")
            .unwrap(),
    );
    let res = client
        .post(format!("http://{addr}/api/uploads"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);
    let text = res.text().await.unwrap();
    assert!(text.contains("图片尺寸过大"), "应提示图片尺寸过大: {text}");
}

/// 手工构造声明超大尺寸的小体积 PNG：签名 + IHDR（RGBA，bit depth 8，合法 CRC）、
/// 最小 IDAT（zlib 头 + 空存储块，结构合法、不解码像素）。
///
/// `into_dimensions()` 只解析 IHDR/校验 chunk 结构，无需真实像素数据。
fn fake_large_png(width: u32, height: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth 8, 颜色类型 RGBA, 压缩/滤波/隔行默认
    out.extend_from_slice(&13u32.to_be_bytes()); // IHDR 数据长度固定 13
    out.extend_from_slice(&ihdr);
    out.extend_from_slice(&crc32(&ihdr).to_be_bytes());
    // IDAT：zlib 头 + 空存储块（BFINAL=1,BTYPE=00；LEN=0,NLEN=0xffff）+ adler32 占位
    let idat = vec![0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
    out.extend_from_slice(&(idat.len() as u32).to_be_bytes());
    out.extend_from_slice(b"IDAT");
    out.extend_from_slice(&idat);
    out.extend_from_slice(&crc32(&idat).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(b"IEND");
    out.extend_from_slice(&crc32(b"IEND").to_be_bytes());
    out
}

/// 4000x3000 JPEG 经 compress_image(max_edge=2000, quality=85) →
/// 输出 2000x1500 且字节更小。
#[tokio::test]
async fn compress_image_downscales_large_jpeg() {
    let img = image::RgbImage::from_pixel(4000, 3000, image::Rgb([180, 90, 40]));
    let mut buf = Vec::new();
    {
        let mut cur = std::io::Cursor::new(&mut buf);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cur, 92)
            .encode_image(&img)
            .unwrap();
    }
    let original_len = buf.len();

    let out = uploads::compress_image(std::path::Path::new("big.jpg"), &buf, 2000, 85)
        .expect("压缩应成功");
    assert!(
        out.len() < original_len,
        "压缩后应更小: {} vs {original_len}",
        out.len()
    );

    let decoded = image::load_from_memory(&out).expect("压缩输出应可解码");
    assert_eq!(decoded.dimensions(), (2000, 1500));
}

/// 极小 GIF 经 compress_image → 原样返回（长度与字节不变）。
#[tokio::test]
async fn gif_is_not_compressed() {
    let mut gif_bytes = Vec::new();
    {
        let mut cur = std::io::Cursor::new(&mut gif_bytes);
        let mut encoder =
            image::codecs::gif::GifEncoder::new(&mut cur);
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([255, 255, 255, 255]),
            )))
            .unwrap();
    }

    let out = uploads::compress_image(std::path::Path::new("tiny.gif"), &gif_bytes, 2000, 85)
        .expect("GIF 应原样返回");
    assert_eq!(out.len(), gif_bytes.len(), "GIF 长度不应变化");
    assert_eq!(out, gif_bytes, "GIF 字节应原样返回");
}
