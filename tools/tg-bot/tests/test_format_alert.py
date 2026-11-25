import unittest

from oto_bot.format_alert import alert_line, has_solution, margin_minutes


class FormatAlertTest(unittest.TestCase):
    def test_early_with_name_and_margin(self):
        item = {
            "request_id": "req",
            "alternative_engineer_id": "22222222-2222-2222-2222-222222222222",
            "alternative_margin_seconds": 120,
        }
        text = alert_line(item, "№2")
        self.assertEqual(
            text,
            "не успевает, ближайший свободный – инженер №2, успевает с запасом 2 мин",
        )
        self.assertTrue(has_solution(item))

    def test_no_replacement(self):
        item = {
            "alternative_engineer_id": None,
            "alternative_margin_seconds": -30,
        }
        self.assertEqual(alert_line(item), "не успевает, замены в пуле нет")
        self.assertFalse(has_solution(item))

    def test_margin_rounds_up_from_seconds(self):
        self.assertEqual(margin_minutes(90), 2)
        self.assertIsNone(margin_minutes(0))
        self.assertIsNone(margin_minutes(-5))


if __name__ == "__main__":
    unittest.main()
