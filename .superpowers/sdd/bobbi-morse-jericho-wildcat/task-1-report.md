# Task 1 Report

## What I changed
- Added `site_url` to `src/config.rs` and wired it into `Config::defaults()` as `String::new()`.
- Added `site_url = "https://hancic.site"` to `config.example.toml`.
- Updated `tests/common/mod.rs` so test configs populate `site_url` with `https://example.test`.
- Added the Task 1 integration test in `tests/front_pages.rs` for absolute share URL metadata expectations.

## Verification
- Ran: `cargo test post_page_renders_absolute_share_urls_from_config --test front_pages -- --exact`
- Result before config plumbing: failed as expected because `site_url` was missing.
- Result after config plumbing: still fails because canonical / `og:url` metadata rendering is not implemented yet (reserved for later task).

## Notes
- I did not implement share-card metadata generation in this task.
- Scope stayed limited to config plumbing + minimal test scaffolding, per brief.
