import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import hancic_mcp  # noqa: E402


class ListPostsSummaryTest(unittest.TestCase):
    def test_list_posts_omits_content_md(self):
        seen = {}

        def fake_request(method, path, **kwargs):
            seen["method"] = method
            seen["path"] = path
            seen["params"] = kwargs.get("params")
            return {
                "items": [
                    {"id": 1, "title": "一", "content_md": "很长的正文"},
                    {"id": 2, "title": "二", "content_md": "另一篇长文"},
                ],
                "total": 2,
            }

        original = hancic_mcp._request
        hancic_mcp._request = fake_request
        try:
            result = hancic_mcp.list_posts(page=2, page_size=5, category="科技")
        finally:
            hancic_mcp._request = original

        self.assertEqual(seen["path"], "/posts")
        self.assertEqual(seen["params"], {"page": 2, "page_size": 5, "category": "科技"})
        self.assertNotIn("content_md", result["items"][0])
        self.assertNotIn("content_md", result["items"][1])


if __name__ == "__main__":
    unittest.main()
