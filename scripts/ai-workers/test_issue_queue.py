"""Focused legacy issue intake decisions; run with unittest discovery."""

import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("issue_queue", Path(__file__).with_name("issue_queue.py"))
queue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(queue)


class IntakeTests(unittest.TestCase):
    def test_acceptance_label_and_report_markers(self):
        examples = [
            {"labels": [{"name": "AcCePtAnCe-TeSt"}]},
            {"title": "  [ACCEPTANCE TEST] local check"},
            {"title": "Rbxport: [acceptance test] local check"},
            {"body": " \n [Acceptance Test] local check"},
            {"body": "Submitted from rbxport.\n\n  [AcCePtAnCe TeSt] local check\n\n---\narchive"},
        ]
        for issue in examples:
            with self.subTest(issue=issue):
                self.assertTrue(queue.acceptance_report(issue))
        for issue in ({}, {"body": "Crash after [acceptance test] text"},
                      {"body": "Submitted from rbxport.\n\nCrash mentions [acceptance test]"}):
            with self.subTest(issue=issue):
                self.assertFalse(queue.acceptance_report(issue))

    def test_pagination_excludes_acceptance_and_hold_before_screening(self):
        pages = [[{"number": 1, "title": "normal"},
                  {"number": 2, "labels": [{"name": "HOLD"}]},
                  {"number": 3, "pull_request": {"url": "pr"}}],
                 [{"number": 4, "body": "Submitted from rbxport.\n\n[acceptance test]"},
                  {"number": 5, "labels": [{"name": "acceptance-test"}]},
                  {"number": 6, "title": "another normal"}]]
        with patch.object(queue, "gh_json", return_value=pages) as gh:
            self.assertEqual([issue["number"] for issue in queue.list_issues()], [1, 6])
        gh.assert_called_once_with("api", "repos/chrisle/rbxport/issues?state=open&per_page=100", "--paginate", "--slurp")

    def test_check_reloads_current_issue_and_fails_closed(self):
        with patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", return_value={"state": "OPEN", "labels": [{"name": "acceptance-test"}]}):
            self.assertEqual(queue.main(), 1)
        with patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", side_effect=subprocess.CalledProcessError(1, "gh")):
            self.assertEqual(queue.main(), 2)
        with patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", return_value={"state": "OPEN", "title": "ordinary"}):
            self.assertEqual(queue.main(), 0)


if __name__ == "__main__":
    unittest.main()
