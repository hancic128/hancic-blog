import unittest
from datetime import datetime, timezone, timedelta
from zoneinfo import ZoneInfo

from scripts.backfill_published_at import (
    is_work_window,
    plan_backfill,
    slots_for_count,
)


TZ = ZoneInfo("Asia/Shanghai")


class ScheduleTest(unittest.TestCase):
    def test_work_window_edges(self):
        self.assertFalse(is_work_window(datetime(2026, 9, 23, 9, 59, tzinfo=TZ)))
        self.assertTrue(is_work_window(datetime(2026, 9, 23, 10, 0, tzinfo=TZ)))
        self.assertTrue(is_work_window(datetime(2026, 9, 23, 21, 59, tzinfo=TZ)))
        self.assertFalse(is_work_window(datetime(2026, 9, 23, 22, 0, tzinfo=TZ)))
        self.assertFalse(is_work_window(datetime(2026, 9, 26, 12, 0, tzinfo=TZ)))

    def test_slots_preserve_chronological_order(self):
        self.assertEqual(slots_for_count(1), [(0, 32)])
        self.assertEqual(slots_for_count(2), [(0, 32), (1, 30)])
        self.assertEqual(slots_for_count(4), [(0, 32), (1, 30), (22, 30), (23, 31)])
        slots = slots_for_count(10)
        self.assertEqual(len(slots), 10)
        self.assertEqual(slots[0], (0, 32))
        self.assertEqual(slots[-2:], [(22, 30), (23, 31)])
        self.assertEqual(slots, sorted(slots))


class PlanBackfillTest(unittest.TestCase):
    def test_plan_moves_workday_posts_and_keeps_weekend(self):
        items = [
            {"id": 3, "published_at": "2026-09-23T13:00:00+08:00"},
            {"id": 1, "published_at": "2026-09-23T10:00:00+08:00"},
            {"id": 2, "published_at": "2026-09-23T11:00:00+08:00"},
            {"id": 4, "published_at": "2026-09-26T12:00:00+08:00"},
            {"id": 5, "published_at": "2026-09-23T22:30:00+08:00"},
        ]
        plan = plan_backfill(items, TZ)
        self.assertEqual([p["id"] for p in plan], [1, 2, 3])
        self.assertEqual([p["new"].hour for p in plan], [0, 1, 22])
        self.assertEqual([p["new"].minute for p in plan], [32, 30, 30])
        self.assertTrue(all(p["old"].date() == p["new"].date() for p in plan))

    def test_plan_is_idempotent_for_moved_times(self):
        already = [
            {"id": 1, "published_at": "2026-09-23T00:32:00+08:00"},
            {"id": 2, "published_at": "2026-09-23T22:30:00+08:00"},
            {"id": 3, "published_at": "2026-09-26T12:00:00+08:00"},
        ]
        self.assertEqual(plan_backfill(already, TZ), [])

    def test_plan_handles_more_than_four_posts_per_day(self):
        items = [
            {"id": i, "published_at": f"2026-09-23T{10 + i}:00:00+08:00"}
            for i in range(6)
        ]
        plan = plan_backfill(items, TZ)
        self.assertEqual(len(plan), 6)
        self.assertEqual([p["new"].hour for p in plan], [0, 1, 2, 3, 22, 23])


if __name__ == "__main__":
    unittest.main()
